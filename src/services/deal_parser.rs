use std::sync::Arc;
use std::time::Duration;
use anyhow::{anyhow, Result};
use regex::Regex;
use reqwest::Client;
use tracing::{info, warn};
use url::Url;
use uuid::Uuid;

use super::accesstrade::AccessTradeClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Platform {
    Shopee,
    Lazada,
    Tiki,
    TikTokShop,
    Unknown,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Shopee => "Shopee",
            Platform::Lazada => "Lazada",
            Platform::Tiki => "Tiki",
            Platform::TikTokShop => "TikTokShop",
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
}

/// Thông tin voucher/mã sale nhặt được tự động
#[derive(Debug, Clone)]
pub struct SaleVoucher {
    pub name: String,
    pub discount_desc: String,
    pub min_spend_desc: String,
    pub remaining_percent: u8,
    pub back_hours: String,
}

#[derive(Debug, Clone)]
pub struct ParsedDeal {
    pub platform: Platform,
    pub product_title: String,
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
    tiki_item_regex: Regex,
}

impl DealParser {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::with_accesstrade(Arc::new(AccessTradeClient::new()))
    }

    pub fn with_accesstrade(accesstrade: Arc<AccessTradeClient>) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(6))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| Client::new());

        let url_regex = Regex::new(r"https?://[^\s]+").unwrap();
        let shopee_item_regex = Regex::new(r"(?:-i\.|\/product\/)(\d+)\/(\d+)|\bi\.(\d+)\.(\d+)").unwrap();
        let lazada_item_regex = Regex::new(r"-i(\d+)(?:-s(\d+))?\.html").unwrap();
        let tiki_item_regex = Regex::new(r"-p(\d+)\.html").unwrap();

        Self {
            http_client,
            accesstrade,
            url_regex,
            shopee_item_regex,
            lazada_item_regex,
            tiki_item_regex,
        }
    }

    /// Trích xuất URL đầu tiên từ một đoạn tin nhắn
    pub fn extract_first_url(&self, text: &str) -> Option<String> {
        self.url_regex.find(text).map(|m| m.as_str().to_string())
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
        } else {
            Platform::Unknown
        }
    }

    /// Kiểm tra URL có phải là link rút gọn cần resolve không
    pub fn is_short_link(&self, url_str: &str) -> bool {
        let lower = url_str.to_lowercase();
        lower.contains("shope.ee")
            || lower.contains("shp.ee")
            || lower.contains("s.shopee.vn")
            || lower.contains("s.lazada.vn")
            || lower.contains("c.lazada.vn")
            || lower.contains("lzd.co")
            || lower.contains("ti.ki")
            || lower.contains("vt.tiktok.com")
            || lower.contains("vm.tiktok.com")
    }

    /// Resolve link rút gọn đến link đích thực tế
    pub async fn resolve_url(&self, raw_url: &str) -> (String, Option<String>) {
        if !self.is_short_link(raw_url) {
            return (raw_url.to_string(), None);
        }

        info!("Đang giải mã shortlink: {}", raw_url);
        match self.http_client.get(raw_url).send().await {
            Ok(resp) => {
                let final_url = resp.url().as_str().to_string();
                let fetched_title = match resp.text().await {
                    Ok(body) => self.extract_title_from_html(&body),
                    Err(_) => None,
                };
                info!("Giải mã thành công: {} -> {}", raw_url, final_url);
                (final_url, fetched_title)
            }
            Err(err) => {
                warn!("Không thể giải mã shortlink (dùng link gốc): {}", err);
                (raw_url.to_string(), None)
            }
        }
    }

    /// Trích xuất title từ HTML <title> hoặc <meta property="og:title">
    pub fn extract_title_from_html(&self, html: &str) -> Option<String> {
        let og_regex = Regex::new(r#"<meta\s+property=["']og:title["']\s+content=["']([^"']+)["']"#).ok()?;
        if let Some(caps) = og_regex.captures(html) {
            if let Some(m) = caps.get(1) {
                return Some(html_escape_decode(m.as_str()));
            }
        }

        let title_regex = Regex::new(r#"<title>([^<]+)</title>"#).ok()?;
        if let Some(caps) = title_regex.captures(html) {
            if let Some(m) = caps.get(1) {
                let t = m.as_str().trim();
                let t = t.replace("| Shopee Việt Nam", "").replace("- Lazada.vn", "");
                return Some(html_escape_decode(t.trim()));
            }
        }

        None
    }

    /// Bóc tách tên sản phẩm từ URL Slug nếu không có title từ HTML
    pub fn extract_title_from_slug(&self, url_str: &str) -> String {
        if let Ok(parsed) = Url::parse(url_str) {
            let path = parsed.path();
            let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            if let Some(first) = segments.first() {
                let slug = first.split("-i.").next().unwrap_or(first);
                let title = slug.replace(['-', '_'], " ");
                let title = title.trim();
                if title.len() > 3 && !title.starts_with("product") {
                    return capitalize_words(title);
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
                if let Some(caps) = self.lazada_item_regex.captures(url_str) {
                    if let Some(item_id) = caps.get(1) {
                        return format!("https://www.lazada.vn/products/-i{}.html", item_id.as_str());
                    }
                }
                format!("https://www.lazada.vn{}", parsed.path())
            }
            Platform::Tiki => {
                if let Some(caps) = self.tiki_item_regex.captures(url_str) {
                    if let Some(product_id) = caps.get(1) {
                        return format!("https://tiki.vn/product-p{}.html", product_id.as_str());
                    }
                }
                format!("https://tiki.vn{}", parsed.path())
            }
            Platform::TikTokShop | Platform::Unknown => {
                format!("{}://{}{}", parsed.scheme(), parsed.host_str().unwrap_or(""), parsed.path())
            }
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
            (Platform::TikTokShop, _) | (Platform::Unknown, _) => (
                SaleVoucher {
                    name: "Voucher Livestream / Giỏ Hàng TikTok Shop".to_string(),
                    discount_desc: "Áp mã trợ giá trực tiếp tại giỏ hàng TikTok Shop".to_string(),
                    min_spend_desc: "Đơn từ 80k".to_string(),
                    remaining_percent: 85,
                    back_hours: "Cập nhật liên tục 24/7".to_string(),
                },
                vec![
                    "Mở sản phẩm bằng đường link của bot để kiểm tra voucher trợ giá trong giỏ hàng".to_string(),
                    "Chọn voucher freeship extra để được giảm thêm phí vận chuyển".to_string(),
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

        let initial_platform = self.detect_platform(&raw_url);
        let (resolved_url, html_title) = self.resolve_url(&raw_url).await;

        let platform = if initial_platform == Platform::Unknown {
            self.detect_platform(&resolved_url)
        } else {
            initial_platform
        };

        if platform == Platform::Unknown {
            return Err(anyhow!("Hiện tại bot hỗ trợ xử lý link cho: Shopee, Lazada, Tiki, TikTok Shop. Vui lòng gửi link thuộc các sàn này."));
        }

        // Ưu tiên: Tiêu đề gửi kèm từ tin nhắn -> Tiêu đề bóc từ HTML -> Tiêu đề bóc từ Slug URL
        let product_title = if let Some(title) = self.extract_title_from_text(input_text, &raw_url) {
            title
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

        let override_campaign_id = match platform {
            Platform::Shopee => settings.campaign_shopee,
            Platform::Lazada => settings.campaign_lazada,
            Platform::Tiki => settings.campaign_tiki,
            Platform::TikTokShop => settings.campaign_tiktok,
            Platform::Unknown => None,
        };

        // 1. Ưu tiên gọi ACCESSTRADE API thật nếu có API Token
        let (affiliate_url, sub_id, is_affiliate) = if let Some(token) = settings.accesstrade_token.map(str::trim).filter(|s| !s.is_empty()) {
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
                Ok(res) => (res.url, default_sub_id, true),
                Err(err_msg) => {
                    warn!("Fallback khỏi ACCESSTRADE API sang link dự phòng: {}", err_msg);
                    affiliate_note = Some(err_msg);
                    self.build_affiliate_url(
                        &clean_url,
                        settings.deeplink_template,
                        user_id,
                        referrer_id,
                        &click_id,
                    )
                }
            }
        } else {
            // 2. Fallback sang Deeplink Template hoặc Clean URL
            self.build_affiliate_url(
                &clean_url,
                settings.deeplink_template,
                user_id,
                referrer_id,
                &click_id,
            )
        };

        let (primary_voucher, tips) = self.pick_best_voucher(&platform, is_affiliate);

        let estimated_cashback_text = match platform {
            Platform::Shopee => "Hoàn thêm 3.0% - 10.5% (Tối đa 150,000₫/đơn)",
            Platform::Lazada => "Hoàn thêm 2.5% - 8.0% (Tối đa 100,000₫/đơn)",
            Platform::Tiki => "Hoàn thêm 1.5% - 5.0% (Tối đa 80,000₫/đơn)",
            Platform::TikTokShop => "Hoàn thêm 3.0% - 12.0% (Tối đa 120,000₫/đơn)",
            Platform::Unknown => "Hoàn tiền lên tới 10%",
        }
        .to_string();

        Ok(ParsedDeal {
            platform,
            product_title,
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
}
