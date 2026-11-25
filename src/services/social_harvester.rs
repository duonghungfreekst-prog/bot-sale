use std::collections::HashSet;
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
];

const DEFAULT_WEB_DEAL_PAGES: &[&str] = &[
    "https://magiamgiashopee.vn",
];

const MOBILE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

#[derive(Clone, Debug, Default)]
pub struct SocialSyncReport {
    pub harvested_params: Option<String>,
    pub deals_found: usize,
    pub deals_inserted: usize,
    pub deals_updated: usize,
}

/// Trích xuất URL đích từ JS-bridge HTML (`var CONFIG={httpUrl:"..."}`) của `s.shopee.vn`.
pub fn extract_shopee_config_http_url(html: &str) -> Option<String> {
    let re = Regex::new(r#"httpUrl\s*:\s*"([^"]+)""#).ok()?;
    let caps = re.captures(html)?;
    let raw = caps.get(1)?.as_str();
    let decoded = raw
        .replace("\\/", "/")
        .replace("\\u0026", "&")
        .replace("\\u003d", "=")
        .replace("\\u003D", "=")
        .replace("&amp;", "&");
    if decoded.starts_with("https://") || decoded.starts_with("http://") {
        Some(decoded)
    } else {
        None
    }
}

fn strip_html_entities_and_tags(raw: &str) -> String {
    let re_tag = Regex::new(r"<[^>]+>").unwrap();
    let no_tags = re_tag.replace_all(raw, " ");
    let decoded = no_tags
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .replace("&#8211;", "-")
        .replace("&#8212;", "-")
        .replace("&gt;", ">")
        .replace("&lt;", "<");
    let re_ws = Regex::new(r"\s+").unwrap();
    re_ws.replace_all(decoded.trim(), " ").to_string()
}

fn parse_vnd_amount_hint(text: &str) -> f64 {
    let lower = text.to_ascii_lowercase();
    if lower.contains("đơn 0đ") || lower.contains("từ 0đ") {
        return 0.0;
    }
    let re = Regex::new(r"(?i)(?:đơn\s*từ|từ)\s*(\d+(?:[.,]\d+)?)\s*(k|tr|triệu|đ|d)?").unwrap();
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

/// Phân tích HTML trang `magiamgiashopee.vn` để nhặt thật Mã Instagram, Mã Facebook KOL & top voucher Shopee sạch.
pub fn parse_magiamgiashopee_deals(html: &str) -> Vec<NewDealInput> {
    let re_p = Regex::new(r"(?is)<p\b[^>]*>(.*?)</p>").unwrap();
    let re_href = Regex::new(r#"(?i)href="([^"]+)""#).unwrap();
    let re_pct = Regex::new(r"(\d{1,2})\s*%").unwrap();

    let mut section_url: Option<String> = None;
    let mut kol_social_deals: Vec<NewDealInput> = Vec::new();
    let mut top_voucher_deals: Vec<NewDealInput> = Vec::new();
    let mut seen_codes: HashSet<String> = HashSet::new();

    for cap in re_p.captures_iter(html) {
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
            if let Some(origin) = extract_origin_link_from_an_redir(href) {
                if origin.contains("shopee.vn") {
                    inline_clean_url = Some(origin);
                    break;
                }
            }
        }

        if lower.contains("mã shopee kol")
            || lower.contains("ưu đãi shopeevip")
            || lower.contains("mã hot sẵn ví")
            || lower.contains("mã bách hoá")
            || lower.contains("mã shopee video")
        {
            if let Some(ref u) = inline_clean_url {
                section_url = Some(u.clone());
            }
        }

        let is_ig = lower.contains("mã instagram") || lower.contains("instagram");
        let is_fb = lower.contains("mã facebook") || lower.contains("facebook");
        let pct_val = re_pct
            .captures(&lower)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(0);

        // Ưu tiên 1: Mã Instagram / Facebook từ block MÃ SHOPEE KOL (không bị cắt bởi giới hạn 10 mã phía trên)
        if (is_ig || is_fb) && pct_val >= 15 {
            let deal_url = inline_clean_url
                .clone()
                .or_else(|| section_url.clone())
                .unwrap_or_else(|| "https://shopee.vn/m/kol-affiliate".to_string());
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
                    discount_desc: text.chars().take(120).collect(),
                    min_spend: parse_vnd_amount_hint(&text),
                    deal_url,
                    expire_at: None,
                    is_exclusive: true,
                });
            }
            continue;
        }

        // Ưu tiên 2: Các mã % cao kèm link `shopee.vn/search?evcode=...` thật trên trang
        if pct_val >= 20 && top_voucher_deals.len() < 8 {
            if let Some(ref target_url) = inline_clean_url {
                if target_url.contains("shopee.vn/search?") || target_url.contains("shopee.vn/m/") {
                    let code = stable_deal_code_suffix("SHP_AUTO", target_url);
                    if seen_codes.insert(code.clone()) {
                        let min_spend = parse_vnd_amount_hint(&text);
                        top_voucher_deals.push(NewDealInput {
                            platform: "Shopee".to_string(),
                            title: format!("[Toàn sàn] Voucher Shopee {}%", pct_val),
                            coupon_code: Some(code),
                            discount_desc: text.chars().take(120).collect(),
                            min_spend,
                            deal_url: target_url.clone(),
                            expire_at: None,
                            is_exclusive: true,
                        });
                    }
                }
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

/// Quét tự động các kênh Telegram + Web Deal công khai để nhặt chữ ký số Shopee FB/IG Reels và mã giảm giá Shopee FB/IG thật.
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

    let mut best_params = extract_social_passthrough_params(&current_saved);
    let mut candidate_links: Vec<String> = Vec::new();
    let mut seen_urls: HashSet<String> = HashSet::new();

    // 1. Quét HTML các kênh Telegram công khai (lấy bài MỚI NHẤT ở cuối trang trước)
    for &channel_url in DEFAULT_CHANNELS {
        let resp = match http
            .get(channel_url)
            .header("User-Agent", MOBILE_UA)
            .timeout(Duration::from_secs(8))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };

        let Ok(html) = resp.text().await else {
            continue;
        };

        let mut channel_links: Vec<String> = Vec::new();
        let mut start_idx = 0;
        while let Some(pos) = html[start_idx..].find("s.shopee.vn/") {
            let actual_pos = start_idx + pos;
            let end_pos = html[actual_pos..]
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']')
                })
                .map(|p| actual_pos + p)
                .unwrap_or_else(|| html.len());
            let raw_link = &html[actual_pos..end_pos];
            let full_url = if !raw_link.starts_with("http") {
                format!("https://{}", raw_link)
            } else {
                raw_link.replace("http://", "https://")
            };
            channel_links.push(full_url);
            start_idx = end_pos;
            if start_idx >= html.len() {
                break;
            }
        }

        // Quét ngược từ tin mới nhất lên để ưu tiên chữ ký còn hạn
        for full_url in channel_links.into_iter().rev() {
            if seen_urls.insert(full_url.clone()) {
                consider_best_params(&mut best_params, &full_url);
                candidate_links.push(full_url);
                if candidate_links.len() >= 60 {
                    break;
                }
            }
        }
        if candidate_links.len() >= 60 {
            break;
        }
    }

    // 2. Truy vấn song song theo đợt để đọc cả HTTP Redirect URL lẫn HTML JS-bridge `CONFIG.httpUrl`
    for chunk in candidate_links.chunks(10) {
        let mut join_set = tokio::task::JoinSet::new();
        for link in chunk {
            if let Some(origin) = extract_origin_link_from_an_redir(link) {
                consider_best_params(&mut best_params, &origin);
            }
            let client = http.clone();
            let target = link.clone();
            join_set.spawn(async move {
                let probe_resp = client
                    .get(&target)
                    .header("User-Agent", MOBILE_UA)
                    .timeout(Duration::from_secs(5))
                    .send()
                    .await
                    .ok()?;
                let final_url = probe_resp.url().as_str().to_string();
                let body_html = probe_resp.text().await.unwrap_or_default();
                Some((final_url, body_html))
            });
        }
        while let Some(res) = join_set.join_next().await {
            if let Ok(Some((final_url, body_html))) = res {
                consider_best_params(&mut best_params, &final_url);
                if let Some(cfg_url) = extract_shopee_config_http_url(&body_html) {
                    consider_best_params(&mut best_params, &cfg_url);
                }
            }
        }
        if best_params.as_deref().map(social_param_strength).unwrap_or(0) >= 3 {
            break;
        }
    }

    // 3. Nhặt thêm Mã Giảm Giá Shopee FB / Instagram / KOL / Toàn Sàn thật từ trang deal công khai
    let mut harvested_deals: Vec<NewDealInput> = Vec::new();
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
            let mut parsed = parse_magiamgiashopee_deals(&page_html);
            harvested_deals.append(&mut parsed);
        }
    }

    if let Some(ref params) = best_params {
        if params != &current_saved {
            let _ = Repository::set_setting(&conn, "shopee_social_params", params).await;
            info!(
                "🎉 [SOCIAL_HARVESTER] Đã nhặt chữ ký Shopee FB/IG mới (mức {}): {}",
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
                    "🎁 [SOCIAL_HARVESTER] Đồng bộ mã Shopee FB/IG/KOL: +{} mã mới, ~{} cập nhật (tổng {} mã).",
                    ins, upd, report.deals_found
                );
            }
            Err(err) => {
                warn!("[SOCIAL_HARVESTER] Lỗi lưu mã Shopee FB/IG vào DB: {}", err);
            }
        }
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

/// Khởi chạy tác vụ nền tự động đi nhặt tham số & mã Shopee FB/IG định kỳ mỗi 30 phút
pub fn spawn_social_harvester_task(db: AppDb, config: Config) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;

        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap_or_else(|_| Client::new());

        let mut ticker = tokio::time::interval(Duration::from_secs(1800)); // Quét mỗi 30 phút
        loop {
            ticker.tick().await;
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
}
