use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use regex::Regex;
use reqwest::Client;
use teloxide::prelude::*;
use teloxide::types::{ChatId, ParseMode};
use tracing::{info, warn};

use crate::config::Config;
use crate::db::repository::{NewDealInput, Repository};
use crate::db::AppDb;
use crate::services::shopee_affiliate::{
    extract_origin_link_from_an_redir, extract_social_passthrough_params, social_param_strength,
};
use crate::utils::escape_html;

const DEFAULT_CHANNELS: &[&str] = &[
    "https://t.me/s/magiamgiashopee",
    "https://t.me/s/meosansale",
    "https://t.me/s/dealngonmoingay",
    "https://t.me/s/xomsansale",
];

const DEFAULT_WEB_DEAL_PAGES: &[&str] = &[
    "https://magiamgiashopee.vn",
    "https://bloggiamgia.vn/",
];

const SOCIAL_SYNC_INTERVAL_SECS: u64 = 300;
const MOBILE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

#[derive(Clone, Debug)]
pub struct SocialHarvesterStatus {
    pub is_running: bool,
    pub is_syncing: bool,
    pub sync_cycles: u64,
    pub interval_secs: u64,
    pub last_sync_vn: String,
    pub harvested_params: Option<String>,
    pub sig_strength: u8,
    pub deals_found: usize,
}

impl Default for SocialHarvesterStatus {
    fn default() -> Self {
        Self {
            is_running: false,
            is_syncing: false,
            sync_cycles: 0,
            interval_secs: SOCIAL_SYNC_INTERVAL_SECS,
            last_sync_vn: String::new(),
            harvested_params: None,
            sig_strength: 0,
            deals_found: 0,
        }
    }
}

static SOCIAL_STATUS: OnceLock<Mutex<SocialHarvesterStatus>> = OnceLock::new();

fn social_status_lock() -> &'static Mutex<SocialHarvesterStatus> {
    SOCIAL_STATUS.get_or_init(|| Mutex::new(SocialHarvesterStatus::default()))
}

pub fn snapshot_social_status() -> SocialHarvesterStatus {
    social_status_lock()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

fn current_vn_timestamp_str() -> String {
    let vn_now = chrono::Utc::now() + chrono::Duration::hours(7);
    vn_now.format("%H:%M:%S %d/%m/%Y").to_string()
}

#[derive(Clone, Debug, Default)]
pub struct SocialSyncReport {
    pub harvested_params: Option<String>,
    pub deals_found: usize,
    pub deals_inserted: usize,
    pub deals_updated: usize,
}

/// Giải escape chuỗi HTML nhúng trong JSON / Nuxt payload (ví dụ trên bloggiamgia.vn).
pub fn decode_embedded_html_escapes(raw: &str) -> String {
    raw.replace("\\u003c", "<")
        .replace("\\u003C", "<")
        .replace("\\u003e", ">")
        .replace("\\u003E", ">")
        .replace("\\u0026", "&")
        .replace("\\u003d", "=")
        .replace("\\u003D", "=")
        .replace("\\/", "/")
}

/// Trích xuất URL đích từ JS-bridge HTML (`httpUrl:"..."`, `window.location.replace(...)` hoặc `<meta http-equiv="refresh">`).
pub fn extract_shopee_config_http_url(html: &str) -> Option<String> {
    static RE_PATTERNS: OnceLock<[Regex; 3]> = OnceLock::new();
    let compiled = RE_PATTERNS.get_or_init(|| {
        [
            Regex::new(r#"httpUrl\s*:\s*"([^"]+)""#).unwrap(),
            Regex::new(r#"window\.location\.(?:replace|assign)\(\s*["']([^"']+)["']\s*\)"#).unwrap(),
            Regex::new(r#"(?is)<meta[^>]+http-equiv=["']refresh["'][^>]+content=["'][^"']*?url=([^"'\s>]+)["']"#).unwrap(),
        ]
    });
    for re in compiled {
        if let Some(caps) = re.captures(html)
            && let Some(m) = caps.get(1)
        {
            let decoded = decode_embedded_html_escapes(m.as_str()).replace("&amp;", "&");
            if decoded.starts_with("https://") || decoded.starts_with("http://") {
                return Some(decoded);
            }
        }
    }
    None
}

fn upgrade_shopee_short_url(raw: &str) -> String {
    let trimmed = raw.trim().replace("&amp;", "&");
    for prefix in ["http://s.shopee.vn/", "http://shp.ee/", "http://vn.shp.ee/"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return format!("https://{}", &prefix["http://".len()..]) + rest;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with(prefix) {
            return format!("https://{}", &trimmed["http://".len()..]);
        }
    }
    trimmed
}

/// Kiểm tra đoạn mô tả có phải bài hướng dẫn chung (không phải ưu đãi áp trực tiếp).
pub fn is_guide_noise_deal_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("hướng dẫn lấy mã")
        || lower.contains("hướng dẫn săn mã")
        || lower.contains("hướng dẫn áp mã")
        || lower.contains("khi ấn link lấy mã")
        || lower.contains("nhớ vào live")
        || lower.contains("sếp nên lựa")
        || lower.contains("cách lấy mã")
}

/// Kiểm tra chuỗi `coupon_code` có phải mã định danh nội bộ (`KOL_*`, `SHP_AUTO_*`, `AUTO_*`) dùng để lưu link chứ không phải mã gõ tay.
pub fn is_synthetic_tracking_code(code: &str) -> bool {
    let upper = code.trim().to_ascii_uppercase();
    upper.is_empty()
        || upper.starts_with("KOL_")
        || upper.starts_with("SHP_AUTO_")
        || upper.starts_with("AUTO_")
}

/// Lần tối đa N chặng redirect / HTML JS-bridge để lấy URL đích thực sự chứa chữ ký Shopee FB/IG.
async fn probe_url_for_social_candidates(http: &Client, url: &str, max_hops: u8) -> Vec<String> {
    let mut out = Vec::new();
    let mut current_url = upgrade_shopee_short_url(url);
    for _ in 0..max_hops {
        let Ok(resp) = http
            .get(&current_url)
            .header("User-Agent", MOBILE_UA)
            .timeout(Duration::from_secs(6))
            .send()
            .await
        else {
            break;
        };
        let final_url = resp.url().as_str().to_string();
        out.push(final_url.clone());
        if extract_social_passthrough_params(&final_url)
            .as_deref()
            .map(social_param_strength)
            .unwrap_or(0)
            >= 3
        {
            break;
        }
        let body = resp.text().await.unwrap_or_default();
        if let Some(cfg) = extract_shopee_config_http_url(&body) {
            let upgraded_cfg = upgrade_shopee_short_url(&cfg);
            out.push(upgraded_cfg.clone());
            if extract_social_passthrough_params(&upgraded_cfg)
                .as_deref()
                .map(social_param_strength)
                .unwrap_or(0)
                >= 3
            {
                break;
            }
            if upgraded_cfg != current_url
                && (upgraded_cfg.contains("s.shopee.vn/") || upgraded_cfg.contains("shp.ee/"))
            {
                current_url = upgraded_cfg;
                continue;
            }
        }
        out.push(body);
        break;
    }
    out
}

fn re_ws() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").unwrap())
}

/// Bóc tách bộ tham số Shopee FB/IG từ input thô hoặc tự giải mã nếu Admin dán link rút gọn / link cầu nối.
pub async fn resolve_social_params_from_input(http: &Client, raw_input: &str) -> Option<String> {
    if let Some(direct) = extract_social_passthrough_params(raw_input)
        && (social_param_strength(&direct) >= 3 || !raw_input.trim().starts_with("http"))
    {
        return Some(direct);
    }
    static RE_URL: OnceLock<Regex> = OnceLock::new();
    let re_url = RE_URL.get_or_init(|| Regex::new(r#"https?://[^\s<>"']+"#).unwrap());
    let url = re_url.find(raw_input)?.as_str();
    let mut best: Option<String> = None;
    for candidate in probe_url_for_social_candidates(http, url, 2).await {
        consider_best_params(&mut best, &candidate);
        if best.as_deref().map(social_param_strength).unwrap_or(0) >= 3 {
            return best;
        }
    }
    best.or_else(|| extract_social_passthrough_params(raw_input))
}

fn clean_harvested_desc(text: &str) -> String {
    static RE_URLS: OnceLock<Regex> = OnceLock::new();
    static RE_NOISE: OnceLock<Regex> = OnceLock::new();
    let re_urls = RE_URLS.get_or_init(|| {
        Regex::new(
            r"(?i)(?:https?://\S+|(?:s\.shopee\.vn|shp\.ee|vn\.shp\.ee|nghien\.co|noti\.sale|sandeal\.co|go\.isclix\.com|bit\.ly)/\S+)",
        )
        .unwrap()
    });
    let no_urls = re_urls.replace_all(text, " ");
    let re_noise = RE_NOISE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:►+|list\s*áp\s*dụng\s*[>:]*|áp\s*(?:cho\s*)?list\s*[>:]*|lưu\s*banner\s*|\[c\s+link=[^\]]*\]?)",
        )
        .unwrap()
    });
    let no_noise = re_noise.replace_all(&no_urls, " ");
    re_ws()
        .replace_all(no_noise.trim(), " ")
        .trim_matches(|c: char| matches!(c, '>' | '=' | ':' | '-' | '.' | ' '))
        .chars()
        .take(90)
        .collect()
}

fn strip_html_entities_and_tags(raw: &str) -> String {
    static RE_TAG: OnceLock<Regex> = OnceLock::new();
    let re_tag = RE_TAG.get_or_init(|| Regex::new(r"<[^>]+>").unwrap());
    let no_tags = re_tag.replace_all(raw, " ");
    let decoded = no_tags
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .replace("&#8211;", "-")
        .replace("&#8212;", "-")
        .replace("&gt;", ">")
        .replace("&lt;", "<");
    re_ws().replace_all(decoded.trim(), " ").to_string()
}

fn parse_vnd_amount_hint(text: &str) -> f64 {
    let lower = text.to_ascii_lowercase();
    if lower.contains("đơn 0đ") || lower.contains("từ 0đ") {
        return 0.0;
    }
    static RE_VND: OnceLock<Regex> = OnceLock::new();
    let re = RE_VND.get_or_init(|| {
        Regex::new(r"(?i)(?:đơn\s*từ|từ)\s*(\d+(?:[.,]\d+)?)\s*(k|tr|triệu|đ|d)?").unwrap()
    });
    if let Some(caps) = re.captures(&lower) {
        let num = caps
            .get(1)
            .and_then(|m| m.as_str().replace(',', ".").parse::<f64>().ok())
            .unwrap_or(0.0);
        let unit = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let mult = match unit {
            "tr" | "triệu" => 1_000_000.0,
            "k" => 1_000.0,
            _ => 1.0,
        };
        return (num * mult).max(0.0);
    }
    0.0
}

fn stable_deal_code_suffix(prefix: &str, seed: &str) -> String {
    if let Ok(parsed) = url::Url::parse(seed) {
        for (k, v) in parsed.query_pairs() {
            if k == "promotionId" && !v.trim().is_empty() {
                let tail: String = v.trim().chars().rev().take(10).collect::<String>().chars().rev().collect();
                return format!("{}_{}", prefix, tail);
            }
        }
    }
    let mut h: u32 = 2166136261;
    for &b in seed.as_bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    format!("{}_{:08X}", prefix, h)
}

/// Phân tích HTML trang web deal Shopee (hỗ trợ cả HTML thường & HTML nhúng JSON) để nhặt Mã Instagram, Mã Facebook KOL & top voucher Shopee sạch.
pub fn parse_magiamgiashopee_deals(raw_html: &str) -> Vec<NewDealInput> {
    let html = decode_embedded_html_escapes(raw_html);
    static RE_P: OnceLock<Regex> = OnceLock::new();
    static RE_HREF: OnceLock<Regex> = OnceLock::new();
    static RE_PCT: OnceLock<Regex> = OnceLock::new();
    let re_p = RE_P.get_or_init(|| Regex::new(r"(?is)<p\b[^>]*>(.*?)</p>").unwrap());
    let re_href = RE_HREF.get_or_init(|| Regex::new(r#"(?i)href="([^"]+)""#).unwrap());
    let re_pct = RE_PCT.get_or_init(|| Regex::new(r"(\d{1,2})\s*%").unwrap());

    let mut section_url: Option<String> = None;
    let mut kol_social_deals: Vec<NewDealInput> = Vec::new();
    let mut top_voucher_deals: Vec<NewDealInput> = Vec::new();
    let mut seen_codes: HashSet<String> = HashSet::new();

    for cap in re_p.captures_iter(&html) {
        let block_html = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let text = strip_html_entities_and_tags(block_html);
        if text.is_empty() {
            continue;
        }
        let lower = text.to_lowercase();

        // Tìm link Shopee an_redir trong đoạn hiện tại và giải mã lấy origin_link sạch (bỏ affiliate_id của web nguồn)
        let mut inline_clean_url: Option<String> = None;
        for hcap in re_href.captures_iter(block_html) {
            let href = hcap.get(1).map(|m| m.as_str()).unwrap_or("");
            if let Some(origin) = extract_origin_link_from_an_redir(href)
                && origin.contains("shopee.vn") {
                    inline_clean_url = Some(origin);
                    break;
                }
        }

        if (lower.contains("mã shopee kol")
            || lower.contains("mã link kol")
            || lower.contains("mã lưu từ banner")
            || lower.contains("ưu đãi shopeevip")
            || lower.contains("mã hot sẵn ví")
            || lower.contains("mã bách hoá")
            || lower.contains("mã shopee video"))
            && let Some(ref u) = inline_clean_url {
                section_url = Some(u.clone());
            }

        let is_ig = lower.contains("mã instagram") || lower.contains("instagram");
        let is_fb = lower.contains("mã facebook") || lower.contains("shopee x facebook");
        let pct_val = re_pct
            .captures(&lower)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(0);

        // Bỏ qua các đoạn văn bản hướng dẫn chung chung (không phải deal săn mã cụ thể)
        let is_explicit_guide = lower.contains("hướng dẫn lấy mã")
            || lower.contains("hướng dẫn săn mã")
            || lower.contains("hướng dẫn áp mã")
            || lower.contains("khi ấn link lấy mã")
            || lower.contains("cách lấy mã");
        let is_guide_only = is_explicit_guide
            || (is_guide_noise_deal_text(&text)
                && !inline_clean_url
                    .as_deref()
                    .map(|u| u.contains("shopee.vn/search?") || u.contains("evcode=") || u.contains("promotionId="))
                    .unwrap_or(false));
        if is_guide_only {
            continue;
        }

        // Ưu tiên 1: Mã Instagram / Facebook từ block MÃ SHOPEE KOL
        if (is_ig || is_fb) && pct_val >= 15 {
            let Some(deal_url) = inline_clean_url.clone().or_else(|| section_url.clone()) else {
                continue;
            };
            let (code, title) = if is_ig {
                (
                    format!("KOL_IG_{}", pct_val),
                    "[Toàn sàn] Mã Instagram KOL · Back 9H·15H·20H".to_string(),
                )
            } else {
                (
                    format!("KOL_FB_{}", pct_val),
                    "[Toàn sàn] Mã Facebook KOL · Back 9H·15H·20H".to_string(),
                )
            };
            if seen_codes.insert(code.clone()) {
                kol_social_deals.push(NewDealInput {
                    platform: "Shopee".to_string(),
                    title,
                    coupon_code: Some(code),
                    discount_desc: clean_harvested_desc(&text),
                    min_spend: parse_vnd_amount_hint(&text),
                    deal_url,
                    expire_at: None,
                    is_exclusive: true,
                });
            }
            continue;
        }

        // Ưu tiên 2: Các mã % cao kèm link `shopee.vn/search?evcode=...` hoặc `shopee.vn/m/...` thật trên trang
        if pct_val >= 20 && top_voucher_deals.len() < 8
            && let Some(ref target_url) = inline_clean_url
                && (target_url.contains("shopee.vn/search?") || target_url.contains("shopee.vn/m/")) {
                    let code = stable_deal_code_suffix("SHP_AUTO", target_url);
                    if seen_codes.insert(code.clone()) {
                        let min_spend = parse_vnd_amount_hint(&text);
                        top_voucher_deals.push(NewDealInput {
                            platform: "Shopee".to_string(),
                            title: format!("[Toàn sàn] Voucher Shopee {}%", pct_val),
                            coupon_code: Some(code),
                            discount_desc: clean_harvested_desc(&text),
                            min_spend,
                            deal_url: target_url.clone(),
                            expire_at: None,
                            is_exclusive: true,
                        });
                    }
                }
    }

    kol_social_deals.extend(top_voucher_deals);
    kol_social_deals.truncate(12);
    kol_social_deals
}

fn consider_best_params(best: &mut Option<String>, candidate_url_or_params: &str) {
    let Some(extracted) = extract_social_passthrough_params(candidate_url_or_params) else {
        return;
    };
    let cand_score = social_param_strength(&extracted);
    let cur_score = best.as_deref().map(social_param_strength).unwrap_or(0);
    if cand_score > cur_score || (cand_score == cur_score && best.is_none()) {
        *best = Some(extracted);
    }
}

/// Quyết định tham số FB/IG hiệu lực: luôn nâng cấp khi quét thấy chữ ký mức 3 mới, không hạ cấp chữ ký mạnh đang lưu.
pub fn choose_effective_social_params(scanned_best: Option<String>, current_saved: &str) -> Option<String> {
    let saved_valid = extract_social_passthrough_params(current_saved);
    let scanned_score = scanned_best.as_deref().map(social_param_strength).unwrap_or(0);
    let saved_score = saved_valid.as_deref().map(social_param_strength).unwrap_or(0);

    if scanned_score >= 3 {
        scanned_best
    } else if saved_score >= 3 {
        saved_valid
    } else {
        scanned_best.or(saved_valid)
    }
}

fn collect_shortlinks_from_html(
    html: &str,
    newest_first: bool,
    seen_urls: &mut HashSet<String>,
    out_links: &mut Vec<String>,
    scanned_best: &mut Option<String>,
    max_total: usize,
) {
    static RE_SHORT: OnceLock<Regex> = OnceLock::new();
    static RE_BARE: OnceLock<Regex> = OnceLock::new();
    let re_short = RE_SHORT.get_or_init(|| {
        Regex::new(r#"(?i)https?://(?:s\.shopee\.vn|shp\.ee|vn\.shp\.ee|sandeal\.co)/[^\s"'<>\\)\]]+"#)
            .unwrap()
    });
    let re_bare_shopee = RE_BARE.get_or_init(|| {
        Regex::new(r#"(?i)\bs\.shopee\.vn/[A-Za-z0-9_-]{5,}"#).unwrap()
    });

    let mut extracted = Vec::new();
    for m in re_short.find_iter(html) {
        let u = upgrade_shopee_short_url(m.as_str());
        if let Some(origin) = extract_origin_link_from_an_redir(&u) {
            consider_best_params(scanned_best, &origin);
        } else {
            extracted.push(u);
        }
    }
    for m in re_bare_shopee.find_iter(html) {
        let full = upgrade_shopee_short_url(&format!("https://{}", m.as_str()));
        extracted.push(full);
    }

    if newest_first {
        extracted.reverse();
    }
    for link in extracted {
        if out_links.len() >= max_total {
            break;
        }
        if seen_urls.insert(link.clone()) {
            consider_best_params(scanned_best, &link);
            out_links.push(link);
        }
    }
}

/// Quét tự động các kênh Telegram (kèm phân trang gần đây) + Web Deal công khai để nhặt chữ ký Shopee FB/IG và mã giảm giá FB/IG.
pub async fn sync_social_all_once(
    http: &Client,
    db: &AppDb,
    config: &Config,
) -> SocialSyncReport {
    let mut report = SocialSyncReport::default();
    let Ok(conn) = db.conn() else {
        return report;
    };

    let current_saved = Repository::get_setting(&conn, "shopee_social_params")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let mut scanned_best: Option<String> = None;
    let mut candidate_links: Vec<String> = Vec::new();
    let mut seen_urls: HashSet<String> = HashSet::new();
    let re_before = Regex::new(r#"href="/s/magiamgiashopee\?before=(\d+)""#).ok();

    // 1. Quét HTML các kênh Telegram công khai (lấy tin mới nhất ở cuối trang trước)
    for &channel_url in DEFAULT_CHANNELS {
        let Ok(resp) = http
            .get(channel_url)
            .header("User-Agent", MOBILE_UA)
            .timeout(Duration::from_secs(8))
            .send()
            .await
        else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(html) = resp.text().await else {
            continue;
        };
        collect_shortlinks_from_html(
            &html,
            true,
            &mut seen_urls,
            &mut candidate_links,
            &mut scanned_best,
            70,
        );

        // Với kênh chính magiamgiashopee, lấy thêm 1 trang trước đó để tăng cơ hội săn chữ ký FB/IG Reels
        if channel_url.ends_with("/magiamgiashopee") && candidate_links.len() < 70
            && let Some(before_id) = re_before
                .as_ref()
                .and_then(|re| re.captures(&html))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
            {
                let prev_url = format!("{}?before={}", channel_url, before_id);
                if let Ok(prev_resp) = http
                    .get(&prev_url)
                    .header("User-Agent", MOBILE_UA)
                    .timeout(Duration::from_secs(8))
                    .send()
                    .await
                    && prev_resp.status().is_success()
                        && let Ok(prev_html) = prev_resp.text().await {
                            collect_shortlinks_from_html(
                                &prev_html,
                                true,
                                &mut seen_urls,
                                &mut candidate_links,
                                &mut scanned_best,
                                70,
                            );
                        }
            }

        if candidate_links.len() >= 70 {
            break;
        }
    }

    // 2. Nhặt mã FB / Instagram / KOL / Toàn Sàn & shortlink từ các trang deal công khai
    let mut harvested_deals: Vec<NewDealInput> = Vec::new();
    let mut seen_deal_codes: HashSet<String> = HashSet::new();
    for &page_url in DEFAULT_WEB_DEAL_PAGES {
        let resp = match http
            .get(page_url)
            .header("User-Agent", MOBILE_UA)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                warn!("[SOCIAL_HARVESTER] Nguồn {} trả về HTTP {}", page_url, r.status());
                continue;
            }
            Err(err) => {
                warn!("[SOCIAL_HARVESTER] Không tải được {}: {}", page_url, err);
                continue;
            }
        };
        if let Ok(page_html) = resp.text().await {
            collect_shortlinks_from_html(
                &page_html,
                false,
                &mut seen_urls,
                &mut candidate_links,
                &mut scanned_best,
                80,
            );
            for deal in parse_magiamgiashopee_deals(&page_html) {
                let key = deal
                    .coupon_code
                    .clone()
                    .unwrap_or_else(|| deal.title.clone());
                if seen_deal_codes.insert(key) {
                    harvested_deals.push(deal);
                }
            }
        }
    }

    // 3. Truy vấn song song theo đợt để giải mã 2-hop redirect + HTML JS-bridge
    if scanned_best.as_deref().map(social_param_strength).unwrap_or(0) < 3 {
        for chunk in candidate_links.chunks(10) {
            let mut join_set = tokio::task::JoinSet::new();
            for link in chunk {
                let client = http.clone();
                let target = link.clone();
                join_set.spawn(async move { probe_url_for_social_candidates(&client, &target, 3).await });
            }
            while let Some(res) = join_set.join_next().await {
                if let Ok(candidates) = res {
                    for cand in candidates {
                        consider_best_params(&mut scanned_best, &cand);
                    }
                }
            }
            if scanned_best.as_deref().map(social_param_strength).unwrap_or(0) >= 3 {
                break;
            }
        }
    }

    let effective_params = choose_effective_social_params(scanned_best, &current_saved);
    if let Some(ref params) = effective_params {
        if params != &current_saved {
            let _ = Repository::set_setting(&conn, "shopee_social_params", params).await;
            info!(
                "🎉 [SOCIAL_HARVESTER] Đã tự động cập nhật chữ ký Shopee FB/IG (mức {}): {}",
                social_param_strength(params),
                params
            );
            notify_admin_harvested(params, config).await;
        }
        report.harvested_params = Some(params.clone());
    }

    report.deals_found = harvested_deals.len();
    if !harvested_deals.is_empty() {
        match Repository::add_deals_batch(&conn, &harvested_deals).await {
            Ok((ins, upd)) => {
                report.deals_inserted = ins;
                report.deals_updated = upd;
                info!(
                    "🎁 [SOCIAL_HARVESTER] Tự động đồng bộ mã Shopee FB/IG/KOL: +{} mã mới, ~{} cập nhật (tổng {} mã).",
                    ins, upd, report.deals_found
                );
            }
            Err(err) => {
                warn!("[SOCIAL_HARVESTER] Lỗi lưu mã Shopee FB/IG vào DB: {}", err);
            }
        }
    }

    let sig_strength = report
        .harvested_params
        .as_deref()
        .map(social_param_strength)
        .unwrap_or(0);
    if let Ok(mut st) = social_status_lock().lock() {
        st.is_running = true;
        st.is_syncing = false;
        st.sync_cycles += 1;
        st.interval_secs = SOCIAL_SYNC_INTERVAL_SECS;
        st.last_sync_vn = current_vn_timestamp_str();
        st.harvested_params = report.harvested_params.clone();
        st.sig_strength = sig_strength;
        st.deals_found = report.deals_found;
    }

    report
}

async fn notify_admin_harvested(params: &str, config: &Config) {
    let admin_id = config.admin_telegram_id;
    let bot_token = &config.telegram_token;
    if bot_token.contains("DEMO_TOKEN") || bot_token.trim().is_empty() {
        return;
    }
    let Some(chat_id) = admin_id else {
        return;
    };

    let bot = Bot::new(bot_token);
    let msg = format!(
        "🤖 <b>[TỰ ĐỘNG CẬP NHẬT CHỮ KÝ SHOPEE FB/INSTA]</b>\n\n\
         Hệ thống vừa tự động quét được bộ tham số phiên mạng xã hội mới:\n\
         <code>{}</code>\n\n\
         ✅ Mọi link Shopee cấp cho khách từ thời điểm này sẽ tự động gắn chữ ký này để mở khóa <b>Mã Facebook / Instagram 20% - 50%</b>!",
        escape_html(params)
    );

    let _ = bot
        .send_message(ChatId(chat_id), msg)
        .parse_mode(ParseMode::Html)
        .await;
}

/// Khởi chạy tác vụ nền tự động đi nhặt tham số & mã Shopee FB/IG ngay khi mở app và lặp mỗi 5 phút
pub fn spawn_social_harvester_task(db: AppDb, config: Config) {
    if let Ok(mut st) = social_status_lock().lock() {
        st.is_running = true;
        st.is_syncing = true;
        st.interval_secs = SOCIAL_SYNC_INTERVAL_SECS;
    }

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;

        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap_or_else(|_| Client::new());

        let mut ticker = tokio::time::interval(Duration::from_secs(SOCIAL_SYNC_INTERVAL_SECS));
        loop {
            ticker.tick().await;
            if let Ok(mut st) = social_status_lock().lock() {
                st.is_syncing = true;
            }
            let report = sync_social_all_once(&http, &db, &config).await;
            if report.harvested_params.is_none() && report.deals_found == 0 {
                warn!("⚠️ [SOCIAL_HARVESTER] Chưa tìm thấy chữ ký hoặc mã FB/IG mới trong chu kỳ này.");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_social_from_sample() {
        let sample = "https://s.shopee.vn/test?sm_sig_id=SIG_SAMPLE_123&channel_type=ig&content_type=REELS";
        let res = extract_social_passthrough_params(sample);
        assert!(res.is_some());
        let val = res.unwrap();
        assert!(val.contains("sm_sig_id=SIG_SAMPLE_123"));
        assert!(val.contains("channel_type=ig"));
    }

    #[test]
    fn test_extract_shopee_config_http_url_parses_fb_reels() {
        let js_html = r#"<script>var CONFIG={httpUrl:"https:\/\/shopee.vn\/m\/shopeehangviet?channel_type=fb\u0026content_source=fb\u0026content_type=REELS\u0026mmp_pid=an_173",deepLinkUrl:"..."};</script>"#;
        let resolved = extract_shopee_config_http_url(js_html).unwrap();
        assert_eq!(
            resolved,
            "https://shopee.vn/m/shopeehangviet?channel_type=fb&content_source=fb&content_type=REELS&mmp_pid=an_173"
        );
        let params = extract_social_passthrough_params(&resolved).unwrap();
        assert_eq!(params, "channel_type=fb&content_source=fb&content_type=REELS");
    }

    #[test]
    fn test_parse_magiamgiashopee_deals_extracts_kol_ig_fb_cleanly() {
        let mut many_prefix_deals = String::new();
        for idx in 1..=10 {
            many_prefix_deals.push_str(&format!(
                r#"<p>► Mã 25% đơn từ 100k: <a href="https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fsearch%3Fevcode%3DV{}%26promotionId%3D156001841987993{}&amp;affiliate_id=17312010562">l</a></p>"#,
                idx, idx
            ));
        }
        let html = format!(
            r#"{}
            <p><strong>🔴 MÃ SHOPEE KOL LƯU TẠI: </strong><a href="https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fm%2Fkol-affiliate%231625&amp;affiliate_id=17309660489">link</a></p>
            <p>► 9H, 15H, 20H: Mã giảm 50% tối đa 50K đơn từ 50K &#8211; Mã Instagram (mã back lượt)</p>
            <p>► 9H, 15H, 20H: Mã giảm 30% tối đa 50k đơn từ 50k &#8211; Mã Facebook (mã back lượt)</p>
        "#,
            many_prefix_deals
        );
        let deals = parse_magiamgiashopee_deals(&html);
        assert_eq!(deals.len(), 10);
        assert_eq!(deals[0].coupon_code.as_deref(), Some("KOL_IG_50"));
        assert_eq!(deals[0].deal_url, "https://shopee.vn/m/kol-affiliate");
        assert_eq!(deals[0].min_spend, 50000.0);
        assert_eq!(deals[1].coupon_code.as_deref(), Some("KOL_FB_30"));
        assert_eq!(deals[2].coupon_code.as_deref(), Some("SHP_AUTO_8419879931"));
        assert!(!deals[2].deal_url.contains("affiliate_id=173"));
    }

    #[test]
    fn test_clean_harvested_desc_and_js_replace_extract() {
        let dirty = "► Mã 23% tối đa 500k đơn từ 150K. List áp dụng > https://shp.ee/jmt17xy1";
        let clean = clean_harvested_desc(dirty);
        assert_eq!(clean, "Mã 23% tối đa 500k đơn từ 150K");
        assert!(!clean.contains("shp.ee"));

        let bridge = r#"<script>window.location.replace("https://s.shopee.vn/6L6Hj9wivc?fb_content_id=Q9&encrypted_payload=ENC&channel_type=fb&content_source=fb&content_type=REELS&sm_sig_id=SIG");</script>"#;
        let resolved = extract_shopee_config_http_url(bridge).unwrap();
        let params = extract_social_passthrough_params(&resolved).unwrap();
        assert!(params.contains("sm_sig_id=SIG"));
        assert!(params.contains("encrypted_payload=ENC"));
    }

    #[test]
    fn test_choose_effective_social_params_and_bloggiamgia_decode() {
        let old_sig = "sm_sig_id=OLD_SIG&channel_type=fb&content_source=fb&content_type=REELS";
        let new_sig = "fb_content_id=NEW&encrypted_payload=E2&sm_sig_id=NEW_SIG&channel_type=fb&content_source=fb&content_type=REELS";
        let weak_sig = "channel_type=fb&content_source=fb&content_type=REELS";

        assert_eq!(
            choose_effective_social_params(Some(new_sig.to_string()), old_sig).as_deref(),
            Some(new_sig)
        );
        assert_eq!(
            choose_effective_social_params(Some(weak_sig.to_string()), old_sig).as_deref(),
            Some(old_sig)
        );
        assert_eq!(
            choose_effective_social_params(Some(weak_sig.to_string()), "").as_deref(),
            Some(weak_sig)
        );

        let nuxt_escaped = r#"\u003cp\u003e\u003cstrong\u003eMã lưu từ banner: \u003c/strong\u003e\u003ca href="https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fm%2Fkol-affiliate%23123\u0026amp;affiliate_id=17309660489"\u003elink\u003c/a\u003e\u003c/p\u003e\u003cp\u003e► 0H: Lưu banner mã Shopee X Facebook giảm 30% tối đa 50K đơn từ 50K: https://nghien.co/abc123\u003c/p\u003e\u003cp\u003e📌 Hướng dẫn lấy mã độc quyền Shopee x Facebook 22%: \u003ca href="https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fm%2Fkol-affiliate\u0026amp;affiliate_id=17309660489"\u003elink\u003c/a\u003e\u003c/p\u003e"#;
        let deals = parse_magiamgiashopee_deals(nuxt_escaped);
        assert_eq!(deals.len(), 1);
        assert_eq!(deals[0].coupon_code.as_deref(), Some("KOL_FB_30"));
        assert_eq!(deals[0].deal_url, "https://shopee.vn/m/kol-affiliate");
        assert!(!deals[0].discount_desc.contains("nghien.co"));

        assert!(is_synthetic_tracking_code("KOL_FB_22"));
        assert!(is_synthetic_tracking_code("KOL_IG_50"));
        assert!(is_synthetic_tracking_code("SHP_AUTO_1981279232"));
        assert!(!is_synthetic_tracking_code("ATVIP30"));
        assert!(!is_synthetic_tracking_code("LANGXDKKP"));
    }
}
