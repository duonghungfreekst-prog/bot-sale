use std::sync::Arc;
use std::time::Duration;
use anyhow::{anyhow, Result};
use regex::Regex;
use reqwest::{redirect, Client};
use serde::Deserialize;
use tracing::{info, warn};
use url::Url;
use uuid::Uuid;

use super::accesstrade::{AccessTradeClient, ApprovedCampaigns};
use super::shopee_affiliate::{
    append_social_params_to_shopee_link, build_direct_an_redir_link, generate_open_api_short_link,
    is_valid_shopee_affiliate_id,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Platform {
    Shopee,
    Lazada,
    Tiki,
    TikTokShop,
    Alibaba,
    Dynamic {
        name: String,
        campaign_id: String,
    },
    Unknown,
}

impl Platform {
    pub fn as_str(&self) -> &str {
        match self {
            Platform::Shopee => "Shopee",
            Platform::Lazada => "Lazada",
            Platform::Tiki => "Tiki",
            Platform::TikTokShop => "TikTokShop",
            Platform::Alibaba => "Alibaba",
            Platform::Dynamic { name, .. } => name.as_str(),
            Platform::Unknown => "Unknown",
        }
    }
}

/// Cấu hình đầu vào để tạo link Affiliate
pub struct AffiliateSettings<'a> {
    pub deeplink_template: &'a str,
    pub accesstrade_token: Option<&'a str>,
    pub campaign_shopee: Option<&'a str>,
    pub campaign_lazada: Option<&'a str>,
    pub campaign_tiki: Option<&'a str>,
    pub campaign_tiktok: Option<&'a str>,
    pub shopee_affiliate_id: Option<&'a str>,
    pub shopee_app_id: Option<&'a str>,
    pub shopee_app_secret: Option<&'a str>,
    pub shopee_social_params: Option<&'a str>,
}

/// Thông tin voucher/mã sale nhặt được tự động
#[derive(Debug, Clone)]
pub struct SaleVoucher {
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub discount_desc: String,
    #[allow(dead_code)]
    pub min_spend_desc: String,
    #[allow(dead_code)]
    pub remaining_percent: u8,
    pub back_hours: String,
}

#[derive(Debug, Clone)]
pub struct ParsedDeal {
    pub platform: Platform,
    pub product_title: String,
    pub product_price_vnd: Option<f64>,
    pub product_shop: Option<String>,
    pub product_brand: Option<String>,
    pub product_image_url: Option<String>,
    pub original_url: String,
    #[allow(dead_code)]
    pub resolved_url: String,
    pub clean_url: String,
    pub affiliate_url: String,
    pub is_affiliate: bool,
    pub affiliate_note: Option<String>,
    pub click_id: String,
    pub sub_id: String,
    pub primary_voucher: SaleVoucher,
    #[allow(dead_code)]
    pub estimated_cashback_text: String,
    pub tips: Vec<String>,
}

pub struct DealParser {
    http_client: Client,
    accesstrade: Arc<AccessTradeClient>,
    url_regex: Regex,
    shopee_item_regex: Regex,
    lazada_item_regex: Regex,
    lazada_origin_regex: Regex,
    lazada_js_regex: Regex,
    lazada_app_item_regex: Regex,
    tiki_item_regex: Regex,
}

impl DealParser {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::with_accesstrade(Arc::new(AccessTradeClient::new()))
    }

    pub fn with_accesstrade(accesstrade: Arc<AccessTradeClient>) -> Self {
        let redirect_policy = redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 8 {
                return attempt.stop();
            }
            let next_url = attempt.url().as_str().to_lowercase();
            // Ngăn chặn chuyển hướng sang trang đăng nhập hoặc captcha (ví dụ Shopee /verify/traffic hay /buyer/login) làm mất ID sản phẩm
            if next_url.contains("/verify/")
                || next_url.contains("/buyer/login")
                || next_url.contains("/login")
                || next_url.contains("captcha")
                || next_url.contains("security_check")
            {
                return attempt.stop();
            }
            attempt.follow()
        });

        let http_client = Client::builder()
            .timeout(Duration::from_secs(6))
            .redirect(redirect_policy)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| Client::new());

        let url_regex = Regex::new(r"https?://[^\s]+").unwrap();
        let shopee_item_regex = Regex::new(r"(?:-i\.|\/product\/)(\d+)\/(\d+)|\bi\.(\d+)\.(\d+)").unwrap();
        let lazada_item_regex = Regex::new(r"-i(\d+)(?:-s(\d+))?\.html").unwrap();
        let lazada_origin_regex = Regex::new(r#"<link\s+[^>]*rel=["']origin["'][^>]*href=["']([^"']+)["']"#).unwrap();
        let lazada_js_regex = Regex::new(r#"window\.location\.href\s*=\s*['"](https?://[^'"]*lazada\.vn/products/[^'"]+)['"]"#).unwrap();
        let lazada_app_item_regex = Regex::new(r#"itemId=(\d+)"#).unwrap();
        let tiki_item_regex = Regex::new(r"-p(\d+)\.html").unwrap();

        Self {
            http_client,
            accesstrade,
            url_regex,
            shopee_item_regex,
            lazada_item_regex,
            lazada_origin_regex,
            lazada_js_regex,
            lazada_app_item_regex,
            tiki_item_regex,
        }
    }

    /// Trả về client ACCESSTRADE dùng chung (kèm cache chiến dịch & kho mã giảm giá)
    pub fn accesstrade(&self) -> &Arc<AccessTradeClient> {
        &self.accesstrade
    }

    /// Trích xuất URL đầu tiên từ một đoạn tin nhắn
    pub fn extract_first_url(&self, text: &str) -> Option<String> {
        self.url_regex.find(text).map(|m| {
            m.as_str()
                .trim_end_matches(&['.', ',', '!', '?', ')', ']', '>', '"', '\''][..])
                .to_string()
        })
    }

    /// Trích xuất danh sách tối đa `max_urls` URL duy nhất theo thứ tự xuất hiện (ưu tiên URL sàn TMĐT hỗ trợ)
    #[allow(dead_code)]
    pub fn extract_all_urls(&self, text: &str, max_urls: usize) -> Vec<String> {
        self.extract_all_urls_with_approved(text, max_urls, None)
    }

    /// Trích xuất danh sách URL có hỗ trợ cả các sàn/thương hiệu động đã duyệt trên ACCESSTRADE
    pub fn extract_all_urls_with_approved(
        &self,
        text: &str,
        max_urls: usize,
        approved: Option<&ApprovedCampaigns>,
    ) -> Vec<String> {
        if max_urls == 0 {
            return Vec::new();
        }
        let mut all_unique = Vec::new();
        for m in self.url_regex.find_iter(text) {
            let cleaned = m
                .as_str()
                .trim_end_matches(&['.', ',', '!', '?', ')', ']', '>', '"', '\''][..])
                .to_string();
            if !cleaned.is_empty() && !all_unique.contains(&cleaned) {
                all_unique.push(cleaned);
            }
        }

        let supported: Vec<String> = all_unique
            .iter()
            .filter(|u| self.detect_platform_with_approved(u, approved) != Platform::Unknown)
            .take(max_urls)
            .cloned()
            .collect();

        if !supported.is_empty() {
            supported
        } else if all_unique.len() == 1 {
            all_unique
        } else {
            Vec::new()
        }
    }

    /// Trích xuất tiêu đề sản phẩm từ tin nhắn của người dùng hoặc bóc tách từ link
    pub fn extract_title_from_text(&self, text: &str, raw_url: &str) -> Option<String> {
        let without_url = text.replace(raw_url, "");
        let clean = without_url
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<&str>>()
            .join(" ");

        let clean = clean.trim();
        if clean.len() >= 6 {
            Some(clean.to_string())
        } else {
            None
        }
    }

    /// Nhận diện nền tảng từ URL
    pub fn detect_platform(&self, url_str: &str) -> Platform {
        let lower = url_str.to_lowercase();
        if lower.contains("shopee.vn") || lower.contains("shope.ee") || lower.contains("shp.ee") {
            Platform::Shopee
        } else if lower.contains("lazada.vn") || lower.contains("s.lazada.vn") || lower.contains("lzd.co") {
            Platform::Lazada
        } else if lower.contains("tiki.vn") || lower.contains("ti.ki") {
            Platform::Tiki
        } else if lower.contains("tiktok.com") {
            Platform::TikTokShop
        } else if lower.contains("alibaba.com") {
            Platform::Alibaba
        } else {
            Platform::Unknown
        }
    }

    /// Nhận diện nền tảng từ URL, tự động đối chiếu cả các chiến dịch đã duyệt trên ACCESSTRADE
    pub fn detect_platform_with_approved(
        &self,
        url_str: &str,
        approved: Option<&ApprovedCampaigns>,
    ) -> Platform {
        let core = self.detect_platform(url_str);
        if core != Platform::Unknown {
            return core;
        }
        if let Some(camp) = approved.and_then(|a| a.find_dynamic_campaign_for_url(url_str)) {
            return Platform::Dynamic {
                name: camp.platform_group.clone(),
                campaign_id: camp.id.clone(),
            };
        }
        Platform::Unknown
    }

    /// Kiểm tra URL có phải là link rút gọn cần resolve không
    pub fn is_short_link(&self, url_str: &str) -> bool {
        let lower = url_str.to_lowercase();
        lower.contains("shope.ee")
            || lower.contains("shp.ee")
            || lower.contains("s.shopee.vn")
            || lower.contains("s.lazada.vn")
            || lower.contains("c.lazada.vn")
            || lower.contains("lazada.vn/s.")
            || lower.contains("lzd.co")
            || lower.contains("ti.ki")
            || lower.contains("vt.tiktok.com")
            || lower.contains("vm.tiktok.com")
            || lower.contains("tiktok.com/t/")
    }

    /// Chuẩn hóa các shortlink bị sai subdomain trước khi giải mã (ví dụ www.lazada.vn/s.XXX -> s.lazada.vn/s.XXX)
    fn normalize_short_url(&self, raw_url: &str) -> String {
        if let Ok(parsed) = Url::parse(raw_url)
            && let Some(host) = parsed.host_str() {
                let host_lower = host.to_lowercase();
                if host_lower.contains("lazada.vn")
                    && !host_lower.starts_with("s.lazada.vn")
                    && !host_lower.starts_with("c.lazada.vn")
                    && parsed.path().starts_with("/s.")
                {
                    return format!("https://s.lazada.vn{}", parsed.path());
                }
            }
        raw_url.to_string()
    }

    /// Resolve link rút gọn đến link đích thực tế (giữ tương thích ngược)
    #[allow(dead_code)]
    pub async fn resolve_url(&self, raw_url: &str) -> (String, Option<String>) {
        let (final_url, title, _) = self.resolve_url_with_meta(raw_url).await;
        (final_url, title)
    }

    /// Resolve link rút gọn đồng thời lấy tên sản phẩm đầy đủ & ảnh đại diện thực tế (qua TelegramBot UA cho Shopee)
    pub async fn resolve_url_with_meta(&self, raw_url: &str) -> (String, Option<String>, Option<String>) {
        if !self.is_short_link(raw_url) {
            return (raw_url.to_string(), None, None);
        }

        let fetch_url = self.normalize_short_url(raw_url);
        let lower_fetch = fetch_url.to_ascii_lowercase();
        let is_shopee_short = lower_fetch.contains("shope.ee")
            || lower_fetch.contains("shp.ee")
            || lower_fetch.contains("s.shopee.vn");
        info!("Đang giải mã shortlink: {} (fetch: {})", raw_url, fetch_url);

        let main_fut = self.http_client.get(&fetch_url).send();
        let social_fut = async {
            if is_shopee_short {
                self.fetch_social_preview_metadata(&fetch_url).await
            } else {
                (None, None)
            }
        };

        let (main_res, (social_title, social_img)) = tokio::join!(main_fut, social_fut);

        match main_res {
            Ok(resp) => {
                let mut final_url = resp.url().as_str().to_string();
                let mut html_title = None;
                let mut html_img = None;
                if let Ok(body) = resp.text().await {
                    if final_url.contains("lazada") || fetch_url.contains("lazada") || fetch_url.contains("lzd.co") {
                        if let Some(target) = self.extract_lazada_target_from_html(&body) {
                            info!("Đã trích xuất link đích thực tế từ trang chuyển tiếp Lazada: {}", target);
                            final_url = target;
                        }
                    } else if is_shopee_short
                        && let Some(cfg_url) = crate::services::social_harvester::extract_shopee_config_http_url(&body) {
                            final_url = cfg_url;
                        }
                    html_title = self.extract_title_from_html(&body);
                    html_img = extract_image_from_html(&body);
                }
                let best_title = social_title.or(html_title);
                let best_img = social_img.or(html_img);
                info!("Giải mã thành công: {} -> {} (title={})", raw_url, final_url, best_title.as_deref().unwrap_or("-"));
                (final_url, best_title, best_img)
            }
            Err(err) => {
                warn!("Không thể giải mã shortlink (dùng link chuẩn hóa): {}", err);
                (fetch_url, social_title, social_img)
            }
        }
    }

    async fn fetch_social_preview_metadata(&self, url: &str) -> (Option<String>, Option<String>) {
        let Ok(resp) = self
            .http_client
            .get(url)
            .header("User-Agent", "TelegramBot (like TwitterBot)")
            .timeout(Duration::from_secs(5))
            .send()
            .await
        else {
            return (None, None);
        };
        if !resp.status().is_success() {
            return (None, None);
        }
        let Ok(body) = resp.text().await else {
            return (None, None);
        };
        let title = self.extract_title_from_html(&body);
        let img = extract_image_from_html(&body);
        (title, img)
    }

    /// Trích xuất URL sản phẩm thực tế từ trang HTML bridge của Lazada
    pub fn extract_lazada_target_from_html(&self, html: &str) -> Option<String> {
        // 1. Thẻ <link rel="origin" href="...">
        if let Some(caps) = self.lazada_origin_regex.captures(html)
            && let Some(href) = caps.get(1) {
                let u = href.as_str();
                if u.contains("lazada.vn/products/") {
                    if let Ok(p) = Url::parse(u) {
                        return Some(format!("https://www.lazada.vn{}", p.path()));
                    }
                    return Some(u.to_string());
                }
            }

        // 2. window.location.href = 'https://www.lazada.vn/products/...'
        if let Some(caps) = self.lazada_js_regex.captures(html)
            && let Some(loc) = caps.get(1) {
                let u = loc.as_str();
                if let Ok(p) = Url::parse(u) {
                    return Some(format!("https://www.lazada.vn{}", p.path()));
                }
                return Some(u.to_string());
            }

        // 3. App link itemId
        if let Some(caps) = self.lazada_app_item_regex.captures(html)
            && let Some(id) = caps.get(1) {
                return Some(format!("https://www.lazada.vn/products/-i{}.html", id.as_str()));
            }

        None
    }

    /// Trích xuất title từ `<meta name="description">` đầy đủ, `og:title` hoặc `<title>`
    pub fn extract_title_from_html(&self, html: &str) -> Option<String> {
        let is_junk_title = |t: &str| {
            let lower = t.to_lowercase();
            lower.is_empty()
                || lower.contains("security check")
                || lower.contains("captcha")
                || lower.contains("just a moment")
                || lower.contains("access denied")
                || lower.contains("cloudflare")
                || lower.contains("không tồn tại")
                || lower.contains("mua và bán trên ứng dụng")
                || lower == "shopee"
                || lower == "shopee việt nam"
                || lower == "lazada"
                || lower == "tiktok"
        };

        // Ưu tiên bóc từ meta description của Shopee (`Mua <Tên Đầy Đủ> giá tốt...`) vì og:title thường bị cắt `...`
        if let Some(desc_raw) = extract_meta_content(html, "name", "description")
            .or_else(|| extract_meta_content(html, "property", "og:description"))
            && let Some(cleaned) = clean_shopee_meta_description(&desc_raw)
                && !is_junk_title(&cleaned) {
                    return Some(cleaned);
                }

        if let Some(og_raw) = extract_meta_content(html, "property", "og:title") {
            let decoded = clean_meta_title_suffix(&html_escape_decode(og_raw.trim()));
            if !is_junk_title(&decoded) {
                return Some(decoded);
            }
        }

        let title_regex = Regex::new(r#"(?is)<title[^>]*>([^<]+)</title>"#).ok()?;
        if let Some(caps) = title_regex.captures(html)
            && let Some(m) = caps.get(1) {
                let decoded = clean_meta_title_suffix(&html_escape_decode(m.as_str().trim()));
                if !is_junk_title(&decoded) {
                    return Some(decoded);
                }
            }

        None
    }

    /// Bóc tách tên sản phẩm từ URL Slug nếu không có title từ HTML
    pub fn extract_title_from_slug(&self, url_str: &str) -> String {
        if let Ok(parsed) = Url::parse(url_str) {
            let path = parsed.path();
            let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            let is_container_seg = |s: &str| {
                let l = s.to_lowercase();
                matches!(
                    l.as_str(),
                    "products"
                        | "product-detail"
                        | "product"
                        | "vi"
                        | "vi-vn"
                        | "en"
                        | "vn"
                        | "hotel"
                        | "hotels"
                        | "p"
                        | "dp"
                        | "c"
                        | "dien-thoai"
                        | "laptop"
                        | "may-tinh-bang"
                        | "phu-kien"
                        | "khuyen-mai"
                )
            };

            let is_numeric_only = |s: &str| {
                !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.' || b == b'-')
            };

            let candidate = segments
                .iter()
                .copied()
                .filter(|s| !is_container_seg(s) && !is_numeric_only(s))
                .max_by_key(|s| s.len());

            if let Some(seg) = candidate {
                let slug = seg.split("-i.").next().unwrap_or(seg);
                let slug = self
                    .tiki_item_regex
                    .find(slug)
                    .map(|m| &slug[..m.start()])
                    .unwrap_or(slug);
                let slug = slug.split("-i").next().unwrap_or(slug);
                let slug = slug.strip_suffix(".html").unwrap_or(slug);
                // Loại bỏ hậu tố mã số (ví dụ Shein `-p-12345` hoặc Alibaba `_1600500000000`)
                let slug = match slug.rsplit_once("-p-") {
                    Some((prefix, suffix))
                        if !prefix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()) =>
                    {
                        prefix
                    }
                    _ => slug,
                };
                let slug = match slug.rsplit_once('_') {
                    Some((prefix, suffix))
                        if !prefix.is_empty()
                            && !suffix.is_empty()
                            && suffix.bytes().all(|b| b.is_ascii_digit()) =>
                    {
                        prefix
                    }
                    _ => slug,
                };
                let title = slug.replace(['-', '_'], " ");
                let title = title.trim();
                let has_letters = title.chars().any(|c| c.is_alphabetic());
                if has_letters && title.len() > 3 && !title.starts_with("product") && !title.starts_with("s.") {
                    return capitalize_words(title);
                }
            }
            if let Some(host) = parsed.host_str().map(|h| h.to_lowercase()) {
                if host.contains("shopee") || host.contains("shp.ee") {
                    return "Sản Phẩm Shopee".to_string();
                } else if host.contains("lazada") || host.contains("lzd.co") {
                    return "Sản Phẩm Lazada".to_string();
                } else if host.contains("tiki") || host.contains("ti.ki") {
                    return "Sản Phẩm Tiki".to_string();
                } else if host.contains("tiktok") {
                    return "Sản Phẩm TikTok Shop".to_string();
                }
            }
        }
        "Sản Phẩm Khuyến Mãi".to_string()
    }

    /// Làm sạch URL: loại bỏ toàn bộ tracking rác, đưa về URL chuẩn
    pub fn clean_url(&self, url_str: &str, platform: &Platform) -> String {
        let parsed = match Url::parse(url_str) {
            Ok(u) => u,
            Err(_) => return url_str.to_string(),
        };

        match platform {
            Platform::Shopee => {
                if let Some(caps) = self.shopee_item_regex.captures(url_str) {
                    let (shop_id, item_id) = if let (Some(s), Some(i)) = (caps.get(1), caps.get(2)) {
                        (s.as_str(), i.as_str())
                    } else if let (Some(s), Some(i)) = (caps.get(3), caps.get(4)) {
                        (s.as_str(), i.as_str())
                    } else {
                        ("", "")
                    };

                    if !shop_id.is_empty() && !item_id.is_empty() {
                        return format!("https://shopee.vn/product/{}/{}", shop_id, item_id);
                    }
                }
                format!("https://shopee.vn{}", parsed.path())
            }
            Platform::Lazada => {
                if let Some(caps) = self.lazada_item_regex.captures(url_str)
                    && let Some(item_id) = caps.get(1) {
                        return format!("https://www.lazada.vn/products/-i{}.html", item_id.as_str());
                    }
                // Nếu vẫn là shortlink Lazada (s.lazada.vn, c.lazada.vn, lzd.co), tuyệt đối giữ nguyên host và path, không đổi thành www.lazada.vn
                if let Some(host) = parsed.host_str() {
                    let host_lower = host.to_lowercase();
                    if host_lower.contains("s.lazada.vn") || host_lower.contains("c.lazada.vn") || host_lower.contains("lzd.co") {
                        return format!("https://{}{}", host, parsed.path());
                    }
                }
                format!("https://www.lazada.vn{}", parsed.path())
            }
            Platform::Tiki => {
                if let Some(caps) = self.tiki_item_regex.captures(url_str)
                    && let Some(product_id) = caps.get(1) {
                        return format!("https://tiki.vn/product-p{}.html", product_id.as_str());
                    }
                format!("https://tiki.vn{}", parsed.path())
            }
            Platform::TikTokShop | Platform::Alibaba | Platform::Unknown => {
                format!("{}://{}{}", parsed.scheme(), parsed.host_str().unwrap_or(""), parsed.path())
            }
            Platform::Dynamic { .. } => strip_tracking_query_params(&parsed),
        }
    }

    /// Xây dựng link Affiliate gắn SubID từ template (trả về cờ is_affiliate thực tế)
    pub fn build_affiliate_url(
        &self,
        clean_url: &str,
        template: &str,
        user_id: i64,
        referrer_id: Option<i64>,
        click_id: &str,
    ) -> (String, String, bool) {
        let ref_str = referrer_id.map(|id| id.to_string()).unwrap_or_else(|| "0".to_string());
        let sub_id = format!("u{}_r{}_{}", user_id, ref_str, &click_id[..8]);

        // Nếu template đang là mã mẫu chưa cấu hình (chứa 630000000000 hoặc rỗng)
        // thì trả về chính link sản phẩm sạch để khách mở thẳng Shopee/Lazada không bị lỗi 404
        if template.contains("630000000000") || template.trim().is_empty() {
            return (clean_url.to_string(), sub_id, false);
        }

        let encoded_clean = urlencoding::encode(clean_url);
        let aff_url = template
            .replace("{CLEAN_URL}", &encoded_clean)
            .replace("{USER_ID}", &user_id.to_string())
            .replace("{REF_ID}", &ref_str)
            .replace("{CLICK_ID}", click_id);

        (aff_url, sub_id, true)
    }

    /// Trả về thông tin hướng dẫn voucher trung thực theo trạng thái kết nối Affiliate thực tế
    pub fn pick_best_voucher(&self, platform: &Platform, is_affiliate: bool) -> (SaleVoucher, Vec<String>) {
        match (platform, is_affiliate) {
            (Platform::Shopee, true) => (
                SaleVoucher {
                    name: "Mã Facebook / Affiliate 20%".to_string(),
                    discount_desc: "Link đã gắn luồng Facebook (áp dụng tùy tài khoản & shop)".to_string(),
                    min_spend_desc: "Đơn từ 50k".to_string(),
                    remaining_percent: 83,
                    back_hours: "0H · 9H · 15H · 20H".to_string(),
                },
                vec![
                    "Nếu không thấy mã Facebook -> bấm nút Lấy Mã Shopee / Shopee Video bên dưới trước rồi quay lại giỏ hàng".to_string(),
                    "Xoá sản phẩm cũ trong giỏ hàng trước khi bấm mua để tránh lỗi xung đột mã".to_string(),
                    "Áp dụng được đồng thời với mã Miễn Phí Vận Chuyển Freeship Xtra".to_string(),
                ],
            ),
            (Platform::Shopee, false) => (
                SaleVoucher {
                    name: "Link Sạch Sản Phẩm + Cổng Săn Mã Shopee".to_string(),
                    discount_desc: "Bấm lưu mã tại Cổng Voucher / Shopee Video bên dưới trước khi mua".to_string(),
                    min_spend_desc: "Đơn từ 0đ - 50k".to_string(),
                    remaining_percent: 83,
                    back_hours: "0H · 9H · 15H · 20H".to_string(),
                },
                vec![
                    "Bấm nút [Lưu Mã Shopee] hoặc [Săn Mã Shopee Video] bên dưới để lưu voucher vào ví trước".to_string(),
                    "Sau đó bấm nút [Mở Shopee Xem Sản Phẩm] và chọn Shopee Voucher lúc thanh toán".to_string(),
                    "Xoá sản phẩm cũ trong giỏ hàng trước khi mua để áp được mã mới nhất".to_string(),
                ],
            ),
            (Platform::Lazada, _) => (
                SaleVoucher {
                    name: "Voucher Tích Lũy 100K + Hoàn Tiền MAX".to_string(),
                    discount_desc: "Thu thập voucher tại cổng Lazada trước khi thanh toán".to_string(),
                    min_spend_desc: "Đơn từ 150k".to_string(),
                    remaining_percent: 88,
                    back_hours: "0H · 9H · 12H · 20H".to_string(),
                },
                vec![
                    "Bấm nút [Thu Thập Voucher Lazada] bên dưới để lưu toàn bộ mã tích lũy trước".to_string(),
                    "Tự động cộng dồn với mã freeship và mã thanh toán ví điện tử".to_string(),
                ],
            ),
            (Platform::Tiki, _) => (
                SaleVoucher {
                    name: "Mã Giảm Toàn Sàn + TikiNOW".to_string(),
                    discount_desc: "Lưu mã tại Cổng Khuyến Mãi Tiki hoặc trang thanh toán".to_string(),
                    min_spend_desc: "Đơn từ 100k".to_string(),
                    remaining_percent: 78,
                    back_hours: "8H · 12H · 16H · 20H".to_string(),
                },
                vec![
                    "Áp dụng tốt nhất cho các sản phẩm Tiki Trading và Official Store".to_string(),
                    "Bấm lưu mã tại cổng Tiki bên dưới rồi quay lại sản phẩm".to_string(),
                ],
            ),
            (Platform::Alibaba, _) => (
                SaleVoucher {
                    name: "Ưu Đãi Nguồn Hàng Giá Sỉ Alibaba".to_string(),
                    discount_desc: "Giá sỉ tận xưởng & ưu đãi vận chuyển quốc tế Alibaba.com".to_string(),
                    min_spend_desc: "Đơn sỉ / lẻ quốc tế".to_string(),
                    remaining_percent: 90,
                    back_hours: "Hoạt động 24/7".to_string(),
                },
                vec![
                    "Bấm mở liên kết để truy cập trực tiếp trang sản phẩm chính thức trên Alibaba.com".to_string(),
                    "Áp dụng thêm phiếu giảm giá nhà cung cấp (Supplier Coupon) trước khi thanh toán Trade Assurance".to_string(),
                ],
            ),
            (Platform::Dynamic { name, .. }, _) => (
                SaleVoucher {
                    name: format!("Link Đối Tác Chính Thức {}", name),
                    discount_desc: "Đã gắn link tiếp thị trực tiếp qua hệ thống đối tác ACCESSTRADE".to_string(),
                    min_spend_desc: "Theo điều kiện đơn tại trang hãng".to_string(),
                    remaining_percent: 92,
                    back_hours: "Cập nhật liên tục 24/7".to_string(),
                },
                vec![
                    "Bấm nút bên dưới để truy cập trực tiếp sản phẩm & thu thập mã giảm giá / ưu đãi thẻ thanh toán ngay trên trang hãng".to_string(),
                    "Không mở lại link từ nguồn khác trước khi hoàn tất thanh toán để tránh xung đột mã".to_string(),
                ],
            ),
            (Platform::TikTokShop, _) | (Platform::Unknown, _) => (
                SaleVoucher {
                    name: "Voucher Livestream / Giỏ Hàng TikTok Shop".to_string(),
                    discount_desc: "Áp mã trợ giá trực tiếp tại giỏ hàng TikTok Shop".to_string(),
                    min_spend_desc: "Đơn từ 80k".to_string(),
                    remaining_percent: 85,
                    back_hours: "Cập nhật liên tục 24/7".to_string(),
                },
                vec![
                    "Khi bấm mở trong Telegram: nhấn [Mở trong ứng dụng] hoặc dấu 3 chấm góc trên -> [Mở bằng trình duyệt] để nhảy thẳng vào App TikTok".to_string(),
                    "Chọn thêm voucher trợ giá & freeship extra tại giỏ hàng trước khi đặt đơn".to_string(),
                ],
            ),
        }
    }

    /// Xử lý toàn bộ chu trình phân tích deal và tạo link ACCESSTRADE thật
    pub async fn process_link(
        &self,
        input_text: &str,
        user_id: i64,
        referrer_id: Option<i64>,
        settings: AffiliateSettings<'_>,
    ) -> Result<ParsedDeal> {
        let raw_url = self
            .extract_first_url(input_text)
            .ok_or_else(|| anyhow!("Không tìm thấy đường link hợp lệ trong tin nhắn"))?;

        let approved = self
            .accesstrade
            .get_or_load_approved_campaigns(settings.accesstrade_token)
            .await;

        let initial_platform = self.detect_platform_with_approved(&raw_url, approved.as_ref());
        let (resolved_url, html_title, mut product_image_url) = self.resolve_url_with_meta(&raw_url).await;

        let platform = if initial_platform == Platform::Unknown {
            self.detect_platform_with_approved(&resolved_url, approved.as_ref())
        } else {
            initial_platform
        };

        if platform == Platform::Unknown {
            let dyn_hint = approved
                .as_ref()
                .map(|a| {
                    let extras: Vec<String> = a
                        .dynamic_campaigns()
                        .into_iter()
                        .take(6)
                        .map(|c| c.platform_group.clone())
                        .collect();
                    if extras.is_empty() {
                        String::new()
                    } else {
                        format!(", {}", extras.join(", "))
                    }
                })
                .unwrap_or_default();
            return Err(anyhow!(
                "Hiện tại bot hỗ trợ Shopee, Lazada, Tiki, TikTok Shop, Alibaba{}. Vui lòng gửi link thuộc các sàn/trang đã được duyệt!",
                dyn_hint
            ));
        }

        // Ưu tiên: Tiêu đề gửi kèm từ tin nhắn -> Tiêu đề bóc từ HTML / TelegramBot Meta -> Tiêu đề bóc từ Slug URL
        let user_text_title = self.extract_title_from_text(input_text, &raw_url);
        let mut product_title = if let Some(ref title) = user_text_title {
            title.clone()
        } else if let Some(title) = html_title {
            title
        } else {
            self.extract_title_from_slug(&resolved_url)
        };

        let clean_url = self.clean_url(&resolved_url, &platform);
        let click_id = Uuid::new_v4().simple().to_string();
        let ref_str = referrer_id.map(|id| id.to_string()).unwrap_or_else(|| "0".to_string());
        let default_sub_id = format!("u{}_r{}_{}", user_id, ref_str, &click_id[..8]);

        let mut affiliate_note: Option<String> = None;
        let mut product_price_vnd: Option<f64> = None;
        let mut product_shop: Option<String> = None;
        let mut product_brand: Option<String> = None;

        if platform == Platform::Tiki {
            let tiki_pid = self
                .tiki_item_regex
                .captures(&clean_url)
                .or_else(|| self.tiki_item_regex.captures(&resolved_url))
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));

            if let Some(pid) = tiki_pid
                && let Some((api_title, api_price, api_shop, api_brand, api_thumb)) = self.fetch_tiki_product_info(&pid).await {
                    if user_text_title.is_none() && !api_title.is_empty() {
                        product_title = api_title;
                    }
                    if api_price.is_some() {
                        product_price_vnd = api_price;
                    }
                    product_shop = api_shop;
                    product_brand = api_brand;
                    if product_image_url.is_none() {
                        product_image_url = api_thumb;
                    }
                }
        }

        let override_campaign_id = match &platform {
            Platform::Shopee => settings.campaign_shopee,
            Platform::Lazada => settings.campaign_lazada,
            Platform::Tiki => settings.campaign_tiki,
            Platform::TikTokShop => settings.campaign_tiktok,
            Platform::Dynamic { campaign_id, .. } => Some(campaign_id.as_str()),
            Platform::Alibaba | Platform::Unknown => None,
        };

        // 0. Riêng Shopee: Ưu tiên Shopee Affiliate Chính Chủ (Open API -> Direct Affiliate ID an_redir), sau đó mới fallback ACCESSTRADE
        let mut direct_shopee_result: Option<(String, String, bool)> = None;
        if platform == Platform::Shopee {
            let app_id_opt = settings.shopee_app_id.map(str::trim).filter(|s| !s.is_empty());
            let app_secret_opt = settings.shopee_app_secret.map(str::trim).filter(|s| !s.is_empty());
            if let (Some(app_id), Some(app_secret)) = (app_id_opt, app_secret_opt) {
                let user_str = user_id.to_string();
                let click_short = &click_id[..8];
                let sub_ids = [user_str.as_str(), ref_str.as_str(), click_short];
                match generate_open_api_short_link(
                    &self.http_client,
                    app_id,
                    app_secret,
                    &clean_url,
                    &sub_ids,
                )
                .await
                {
                    Ok(short_url) => {
                        let final_shopee = append_social_params_to_shopee_link(
                            &short_url,
                            settings.shopee_social_params,
                        );
                        direct_shopee_result = Some((final_shopee, default_sub_id.clone(), true));
                    }
                    Err(err_msg) => {
                        warn!("Fallback khỏi Shopee Open API: {}", err_msg);
                        affiliate_note = Some(err_msg);
                    }
                }
            }

            if direct_shopee_result.is_none()
                && let Some(aff_id) = settings
                    .shopee_affiliate_id
                    .map(str::trim)
                    .filter(|id| is_valid_shopee_affiliate_id(id))
                {
                    let an_redir = build_direct_an_redir_link(&clean_url, aff_id, &default_sub_id);
                    let final_shopee = append_social_params_to_shopee_link(
                        &an_redir,
                        settings.shopee_social_params,
                    );
                    affiliate_note = None;
                    direct_shopee_result = Some((final_shopee, default_sub_id.clone(), true));
                }
        }

        // 1. Ưu tiên dùng kết quả Shopee Chính Chủ nếu có; nếu không gọi ACCESSTRADE API thật
        let (affiliate_url, sub_id, is_affiliate) = if let Some(res) = direct_shopee_result {
            res
        } else if let Some(token) = settings.accesstrade_token.map(str::trim).filter(|s| !s.is_empty()) {
            match self
                .accesstrade
                .create_affiliate_link(
                    token,
                    &platform,
                    &clean_url,
                    user_id,
                    &click_id,
                    override_campaign_id,
                )
                .await
            {
                Ok(res) => {
                    if user_text_title.is_none()
                        && let Some(api_name) = res.product_name {
                            product_title = api_name;
                        }
                    if res.product_price_vnd.is_some() {
                        product_price_vnd = res.product_price_vnd;
                    }
                    (res.url, default_sub_id, true)
                }
                Err(err_msg) => {
                    warn!("Fallback khỏi ACCESSTRADE API sang link dự phòng: {}", err_msg);
                    affiliate_note = Some(err_msg);
                    let (fb_url, fb_sub, fb_is_aff) = self.build_affiliate_url(
                        &clean_url,
                        settings.deeplink_template,
                        user_id,
                        referrer_id,
                        &click_id,
                    );
                    let final_fb_url = if platform == Platform::Shopee {
                        append_social_params_to_shopee_link(&fb_url, settings.shopee_social_params)
                    } else {
                        fb_url
                    };
                    (final_fb_url, fb_sub, fb_is_aff)
                }
            }
        } else {
            // 2. Fallback sang Deeplink Template hoặc Clean URL
            let (fb_url, fb_sub, fb_is_aff) = self.build_affiliate_url(
                &clean_url,
                settings.deeplink_template,
                user_id,
                referrer_id,
                &click_id,
            );
            let final_fb_url = if platform == Platform::Shopee {
                append_social_params_to_shopee_link(&fb_url, settings.shopee_social_params)
            } else {
                fb_url
            };
            (final_fb_url, fb_sub, fb_is_aff)
        };

        let (primary_voucher, tips) = self.pick_best_voucher(&platform, is_affiliate);

        let estimated_cashback_text = match &platform {
            Platform::Shopee => "Hoàn thêm 3.0% - 10.5% (Tối đa 150,000₫/đơn)",
            Platform::Lazada => "Hoàn thêm 2.5% - 8.0% (Tối đa 100,000₫/đơn)",
            Platform::Tiki => "Hoàn thêm 1.5% - 5.0% (Tối đa 80,000₫/đơn)",
            Platform::TikTokShop => "Hoàn thêm 3.0% - 12.0% (Tối đa 120,000₫/đơn)",
            Platform::Alibaba => "Hoa hồng đối tác quốc tế 3.92%/đơn",
            Platform::Dynamic { .. } => "Ưu đãi chính hãng & hoa hồng đối tác ACCESSTRADE",
            Platform::Unknown => "Hoàn tiền lên tới 10%",
        }
        .to_string();

        Ok(ParsedDeal {
            platform,
            product_title,
            product_price_vnd,
            product_shop,
            product_brand,
            product_image_url,
            original_url: raw_url,
            resolved_url,
            clean_url,
            affiliate_url,
            is_affiliate,
            affiliate_note,
            click_id,
            sub_id,
            primary_voucher,
            estimated_cashback_text,
            tips,
        })
    }

    /// Lấy tên sản phẩm, giá, shop bán, thương hiệu và ảnh thumbnail thật từ Tiki Public Product API v2
    async fn fetch_tiki_product_info(
        &self,
        product_id: &str,
    ) -> Option<(String, Option<f64>, Option<String>, Option<String>, Option<String>)> {
        let api_url = format!("https://tiki.vn/api/v2/products/{}", product_id);
        let resp = self.http_client.get(&api_url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body = resp.text().await.ok()?;
        let (title, price, shop, brand) = parse_tiki_product_json(&body)?;
        let thumb = serde_json::from_str::<TikiProductApiResp>(&body)
            .ok()
            .and_then(|r| r.thumbnail_url)
            .map(|u| u.trim().to_string())
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"));
        Some((title, price, shop, brand, thumb))
    }
}

fn extract_meta_content(html: &str, attr_key: &str, attr_val: &str) -> Option<String> {
    let p1 = format!(
        r#"(?is)<meta[^>]*\b{}=["']{}["'][^>]*\bcontent=["']([^"']+)["']"#,
        regex::escape(attr_key),
        regex::escape(attr_val)
    );
    let p2 = format!(
        r#"(?is)<meta[^>]*\bcontent=["']([^"']+)["'][^>]*\b{}=["']{}["']"#,
        regex::escape(attr_key),
        regex::escape(attr_val)
    );
    for pat in [p1, p2] {
        let Ok(re) = Regex::new(&pat) else { continue };
        if let Some(caps) = re.captures(html)
            && let Some(m) = caps.get(1) {
                let val = m.as_str().trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
    }
    None
}

fn clean_meta_title_suffix(raw: &str) -> String {
    let stripped = raw
        .replace("| Shopee Việt Nam", "")
        .replace("| Lazada Việt Nam", "")
        .replace("- Lazada.vn", "");
    let trimmed = stripped.trim();
    let trimmed = trimmed.strip_suffix("|...").unwrap_or(trimmed);
    trimmed.trim_end_matches("...").trim().trim_end_matches('|').trim().to_string()
}

fn clean_shopee_meta_description(raw_desc: &str) -> Option<String> {
    let decoded = html_escape_decode(raw_desc.trim());
    let without_prefix = decoded
        .strip_prefix("Mua ")
        .or_else(|| decoded.strip_prefix("mua "))?;
    let cut_idx = without_prefix
        .find(" giá tốt")
        .or_else(|| without_prefix.find(". Mua hàng qua mạng"))?;
    let title = without_prefix[..cut_idx].trim();
    if title.chars().count() >= 4 {
        Some(title.to_string())
    } else {
        None
    }
}

fn extract_image_from_html(html: &str) -> Option<String> {
    let candidate = extract_meta_content(html, "property", "og:image")
        .or_else(|| extract_meta_content(html, "name", "twitter:image"))
        .or_else(|| extract_meta_content(html, "property", "twitter:image"))?;
    let decoded = html_escape_decode(candidate.trim());
    let lower = decoded.to_ascii_lowercase();
    if (decoded.starts_with("https://") || decoded.starts_with("http://"))
        && !lower.contains("icon_favicon")
        && !lower.contains("favicon.")
    {
        Some(decoded)
    } else {
        None
    }
}

#[derive(Deserialize, Debug)]
struct TikiProductApiResp {
    name: Option<String>,
    price: Option<serde_json::Value>,
    thumbnail_url: Option<String>,
    current_seller: Option<TikiNamedField>,
    brand: Option<TikiNamedField>,
}

#[derive(Deserialize, Debug)]
struct TikiNamedField {
    name: Option<String>,
}

/// Bộ dữ liệu (tên sản phẩm, giá VNĐ, tên shop, thương hiệu) trích xuất từ Tiki API
pub type ParsedTikiProductTuple = (String, Option<f64>, Option<String>, Option<String>);

/// Phân tích JSON phản hồi từ Tiki v2 product API thành (tên, giá VNĐ, tên shop, thương hiệu)
pub fn parse_tiki_product_json(
    body: &str,
) -> Option<ParsedTikiProductTuple> {
    let parsed: TikiProductApiResp = serde_json::from_str(body).ok()?;
    let title = parsed
        .name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    let price = parsed.price.and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }).filter(|&p| p > 0.0);
    let shop = parsed
        .current_seller
        .and_then(|s| s.name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let brand = parsed
        .brand
        .and_then(|b| b.name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if title.is_empty() && price.is_none() && shop.is_none() && brand.is_none() {
        return None;
    }
    Some((title, price, shop, brand))
}

fn strip_tracking_query_params(parsed: &Url) -> String {
    let base = format!(
        "{}://{}{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or(""),
        parsed.path()
    );
    let kept_pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| {
            let kl = k.to_lowercase();
            !kl.starts_with("utm_")
                && !kl.starts_with("aff_")
                && !kl.starts_with("at_")
                && !matches!(
                    kl.as_str(),
                    "fbclid"
                        | "gclid"
                        | "ttclid"
                        | "spm"
                        | "scm"
                        | "sub1"
                        | "sub2"
                        | "sub3"
                        | "sub4"
                        | "sub5"
                )
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept_pairs.is_empty() {
        base
    } else if let Ok(mut out_url) = Url::parse(&base) {
        out_url.query_pairs_mut().extend_pairs(kept_pairs);
        out_url.to_string()
    } else {
        base
    }
}

fn html_escape_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn capitalize_words(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

mod urlencoding {
    pub fn encode(data: &str) -> String {
        let mut encoded = String::with_capacity(data.len() * 3);
        for byte in data.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(byte as char);
                }
                _ => {
                    encoded.push_str(&format!("%{:02X}", byte));
                }
            }
        }
        encoded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_url_and_title() {
        let parser = DealParser::new();
        let text = "https://vn.shp.ee/QH2bBsrV Mua Dép Bông Lông Gấu Đi Trong Nhà , Chống Trượt , Dày Dặn";
        let url = parser.extract_first_url(text).unwrap();
        assert_eq!(url, "https://vn.shp.ee/QH2bBsrV");

        let title = parser.extract_title_from_text(text, &url).unwrap();
        assert!(title.contains("Dép Bông Lông Gấu"));

        let wrapped = "Xem deal nè (https://shopee.vn/product/123/456)!";
        assert_eq!(
            parser.extract_first_url(wrapped).unwrap(),
            "https://shopee.vn/product/123/456"
        );
        assert_eq!(
            parser.extract_title_from_slug("https://shopee.vn/product/1469098303/46017065230"),
            "Sản Phẩm Shopee"
        );
    }

    #[test]
    fn test_pick_best_voucher_shopee() {
        let parser = DealParser::new();
        let (voucher_aff, _) = parser.pick_best_voucher(&Platform::Shopee, true);
        assert!(voucher_aff.name.contains("Facebook"));
        assert_eq!(voucher_aff.remaining_percent, 83);
        assert!(voucher_aff.back_hours.contains("0H · 9H · 15H · 20H"));

        let (voucher_clean, _) = parser.pick_best_voucher(&Platform::Shopee, false);
        assert!(voucher_clean.name.contains("Cổng Săn Mã Shopee"));
    }

    #[test]
    fn test_lazada_shortlink_and_html_bridge() {
        let parser = DealParser::new();
        assert!(parser.is_short_link("https://s.lazada.vn/s.CDJeh"));
        assert!(parser.is_short_link("https://www.lazada.vn/s.CDJeh"));
        assert_eq!(
            parser.normalize_short_url("https://www.lazada.vn/s.CDJeh"),
            "https://s.lazada.vn/s.CDJeh"
        );
        assert_eq!(
            parser.clean_url("https://s.lazada.vn/s.CDJeh", &Platform::Lazada),
            "https://s.lazada.vn/s.CDJeh"
        );

        let bridge_html = r#"<html><head><link rel="origin" href="https://www.lazada.vn/products/gat-mua-bosch-i2198179077-s10463772943.html?dsource=share"></head></html>"#;
        let extracted = parser.extract_lazada_target_from_html(bridge_html).unwrap();
        assert_eq!(
            extracted,
            "https://www.lazada.vn/products/gat-mua-bosch-i2198179077-s10463772943.html"
        );
        assert_eq!(
            parser.clean_url(&extracted, &Platform::Lazada),
            "https://www.lazada.vn/products/-i2198179077.html"
        );
    }

    #[test]
    fn test_extract_title_ignores_antibot() {
        let parser = DealParser::new();
        assert_eq!(
            parser.extract_title_from_html("<html><head><title>Security Check</title></head></html>"),
            None
        );
        assert_eq!(
            parser.extract_title_from_html("<html><head><title>Gạt mưa Bosch | Lazada Việt Nam</title></head></html>"),
            Some("Gạt mưa Bosch".to_string())
        );
    }

    #[test]
    fn test_parse_tiki_product_api_json() {
        let json = r#"{
            "name": "Sữa bột Enfagrow Enspire 3 850g hỗ trợ đề kháng",
            "price": 822000,
            "current_seller": {"name": "Tiki Trading"},
            "brand": {"name": "Enfa"}
        }"#;
        let (title, price, shop, brand) = parse_tiki_product_json(json).unwrap();
        assert!(title.contains("Enfagrow"));
        assert_eq!(price, Some(822000.0));
        assert_eq!(shop.as_deref(), Some("Tiki Trading"));
        assert_eq!(brand.as_deref(), Some("Enfa"));
    }

    #[tokio::test]
    async fn test_process_link_shopee_direct_affiliate_id_priority() {
        let parser = DealParser::new();
        let settings = AffiliateSettings {
            deeplink_template: "",
            accesstrade_token: None,
            campaign_shopee: None,
            campaign_lazada: None,
            campaign_tiki: None,
            campaign_tiktok: None,
            shopee_affiliate_id: Some("17375520550"),
            shopee_app_id: None,
            shopee_app_secret: None,
            shopee_social_params: None,
        };
        let parsed = parser
            .process_link(
                "https://shopee.vn/product/123456/987654321?sp_atk=abc",
                42,
                None,
                settings,
            )
            .await
            .expect("process_link phải thành công cho Shopee URL sạch");
        assert!(parsed.is_affiliate);
        assert!(parsed
            .affiliate_url
            .starts_with("https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fproduct%2F123456%2F987654321&affiliate_id=17375520550"));

        // Kiểm tra nối bộ tham số phiên IG/FB Reels khi có mẫu cấu hình
        let settings_with_social = AffiliateSettings {
            deeplink_template: "",
            accesstrade_token: None,
            campaign_shopee: None,
            campaign_lazada: None,
            campaign_tiki: None,
            campaign_tiktok: None,
            shopee_affiliate_id: Some("17375520550"),
            shopee_app_id: None,
            shopee_app_secret: None,
            shopee_social_params: Some(
                "https://s.shopee.vn/2gCo1LuaEU?fb_content_id=Q9test&encrypted_payload=cM5f_test&channel_type=ig&content_source=ig&content_type=REELS&sm_sig_id=QfU_sig",
            ),
        };
        let parsed_social = parser
            .process_link(
                "https://shopee.vn/product/123456/987654321",
                42,
                None,
                settings_with_social,
            )
            .await
            .unwrap();
        assert!(parsed_social.affiliate_url.contains("&channel_type=ig&"));
        assert!(parsed_social.affiliate_url.contains("sm_sig_id=QfU_sig"));
    }

    #[test]
    fn test_extract_all_urls_dedup_and_limit() {
        let parser = DealParser::new();
        let text = "\
            Deal hot hôm nay tại https://example.com/not-supported nè!\n\
            1) Sữa Enfa: https://tiki.vn/product-p12345.html\n\
            2) Áo thun: https://shopee.vn/product/11/22,\n\
            3) Lặp lại: https://shopee.vn/product/11/22)\n\
            4) TikTok: https://www.tiktok.com/@shop/video/123456\n\
            5) Lazada: https://s.lazada.vn/s.CDJeh\n\
            6) Thứ năm: https://shopee.vn/product/33/44\n\
            7) Vượt giới hạn: https://shopee.vn/product/55/66";
        let urls = parser.extract_all_urls(text, 5);
        assert_eq!(urls.len(), 5);
        assert_eq!(urls[0], "https://tiki.vn/product-p12345.html");
        assert_eq!(urls[1], "https://shopee.vn/product/11/22");
        assert_eq!(urls[4], "https://shopee.vn/product/33/44");

        let only_unknown = parser.extract_all_urls("Check https://example.com/abc", 5);
        assert_eq!(only_unknown, vec!["https://example.com/abc".to_string()]);
    }

    #[test]
    fn test_alibaba_clean_url_and_slug_title() {
        let parser = DealParser::new();
        let raw = "https://www.alibaba.com/product-detail/Wireless-Bluetooth-Earbuds_1600500000000.html?spm=a2700";
        assert_eq!(parser.detect_platform(raw), Platform::Alibaba);
        assert_eq!(
            parser.clean_url(raw, &Platform::Alibaba),
            "https://www.alibaba.com/product-detail/Wireless-Bluetooth-Earbuds_1600500000000.html"
        );
        assert_eq!(parser.extract_title_from_slug(raw), "Wireless Bluetooth Earbuds");
    }

    #[test]
    fn test_dynamic_approved_platform_detection_clean_url_and_slug() {
        use super::super::accesstrade::parse_approved_campaigns_page;

        let parser = DealParser::new();
        let page_json = r#"{
            "data": [
                {"id": "6345454361061452947", "merchant": "fpt_shop", "name": "FPT Shop", "approval": "successful", "status": 1, "url": "https://fptshop.com.vn/"},
                {"id": "4704521809526929067", "merchant": "klookvn", "name": "Klook - SIM, Vé Tham Quan, Show, Tour", "approval": "successful", "status": 1, "url": "https://www.klook.com/vi/"}
            ]
        }"#;
        let (approved, _) = parse_approved_campaigns_page(page_json, 1000).unwrap();

        let fpt_url = "https://fptshop.com.vn/dien-thoai/iphone-16-pro-max?utm_source=fb&fbclid=xyz";
        let detected = parser.detect_platform_with_approved(fpt_url, Some(&approved));
        assert_eq!(
            detected,
            Platform::Dynamic {
                name: "FPT Shop".to_string(),
                campaign_id: "6345454361061452947".to_string(),
            }
        );
        assert_eq!(
            parser.clean_url(fpt_url, &detected),
            "https://fptshop.com.vn/dien-thoai/iphone-16-pro-max"
        );
        assert_eq!(parser.extract_title_from_slug(fpt_url), "Iphone 16 Pro Max");

        let klook_url = "https://www.klook.com/vi/activity/1234-vinwonders/?aid=99&utm_medium=social";
        let klook_det = parser.detect_platform_with_approved(klook_url, Some(&approved));
        assert_eq!(klook_det.as_str(), "Klook");
        assert_eq!(
            parser.clean_url(klook_url, &klook_det),
            "https://www.klook.com/vi/activity/1234-vinwonders/?aid=99"
        );

        let batch = "Xem máy https://fptshop.com.vn/dien-thoai/iphone-16-pro-max và tour https://www.klook.com/vi/activity/1234-vinwonders/";
        let extracted = parser.extract_all_urls_with_approved(batch, 5, Some(&approved));
        assert_eq!(extracted.len(), 2);
    }

    #[test]
    fn test_extract_shopee_social_meta_description_and_image() {
        let parser = DealParser::new();
        let shopee_tg_html = r#"<!doctype html><html prefix="og:http://ogp.me/ns#"><head><meta charset="utf-8"><title>Rửa xe pin Asake siêu khoẻ - mô tơ ko than - chạy pin phổ thông...</title><meta name="description" content="Mua Rửa xe pin Asake siêu khoẻ - mô tơ ko than - chạy pin phổ thông Makita M21 giá tốt. Mua hàng qua mạng uy tín, tiện lợi."><meta property="og:title" content="Rửa xe pin Asake siêu khoẻ - mô tơ ko than - chạy pin phổ thông..."><meta property="og:image" content="https://down-vn.img.susercontent.com/file/vn-11134201-81ztc-msj7oadg1ds87a"></head></html>"#;
        assert_eq!(
            parser.extract_title_from_html(shopee_tg_html).as_deref(),
            Some("Rửa xe pin Asake siêu khoẻ - mô tơ ko than - chạy pin phổ thông Makita M21")
        );
        assert_eq!(
            extract_image_from_html(shopee_tg_html).as_deref(),
            Some("https://down-vn.img.susercontent.com/file/vn-11134201-81ztc-msj7oadg1ds87a")
        );
    }
}
