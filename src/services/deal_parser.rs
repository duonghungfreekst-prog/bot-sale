use std::time::Duration;
use anyhow::{anyhow, Result};
use regex::Regex;
use reqwest::Client;
use tracing::{info, warn};
use url::Url;
use uuid::Uuid;

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

#[derive(Debug, Clone)]
pub struct ParsedDeal {
    pub platform: Platform,
    pub original_url: String,
    #[allow(dead_code)]
    pub resolved_url: String,
    pub clean_url: String,
    pub affiliate_url: String,
    pub click_id: String,
    pub sub_id: String,
    pub estimated_cashback_text: String,
}

pub struct DealParser {
    http_client: Client,
    url_regex: Regex,
    shopee_item_regex: Regex,
    lazada_item_regex: Regex,
    tiki_item_regex: Regex,
}

impl DealParser {
    pub fn new() -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(6))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| Client::new());

        // Regex phát hiện URL trong tin nhắn
        let url_regex = Regex::new(r"https?://[^\s]+").unwrap();
        // Regex bóc tách Shopee: shopid và itemid
        let shopee_item_regex = Regex::new(r"(?:-i\.|\/product\/)(\d+)\/(\d+)|\bi\.(\d+)\.(\d+)").unwrap();
        // Regex bóc tách Lazada item id: -i(\d+)
        let lazada_item_regex = Regex::new(r"-i(\d+)(?:-s(\d+))?\.html").unwrap();
        // Regex bóc tách Tiki product id: -p(\d+)
        let tiki_item_regex = Regex::new(r"-p(\d+)\.html").unwrap();

        Self {
            http_client,
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

    /// Nhận diện nền tảng từ URL
    pub fn detect_platform(&self, url_str: &str) -> Platform {
        let lower = url_str.to_lowercase();
        if lower.contains("shopee.vn") || lower.contains("shope.ee") || lower.contains("shp.ee") {
            Platform::Shopee
        } else if lower.contains("lazada.vn") || lower.contains("s.lazada.vn") {
            Platform::Lazada
        } else if lower.contains("tiki.vn") || lower.contains("ti.ki") {
            Platform::Tiki
        } else if lower.contains("tiktok.com") || lower.contains("vt.tiktok.com") {
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
            || lower.contains("s.lazada.vn")
            || lower.contains("ti.ki")
            || lower.contains("vt.tiktok.com")
    }

    /// Resolve link rút gọn đến link đích thực tế
    pub async fn resolve_url(&self, raw_url: &str) -> String {
        if !self.is_short_link(raw_url) {
            return raw_url.to_string();
        }

        info!("Đang giải mã shortlink: {}", raw_url);
        match self.http_client.get(raw_url).send().await {
            Ok(resp) => {
                let final_url = resp.url().as_str().to_string();
                info!("Giải mã thành công: {} -> {}", raw_url, final_url);
                final_url
            }
            Err(err) => {
                warn!("Không thể giải mã shortlink (sẽ dùng link gốc): {}", err);
                raw_url.to_string()
            }
        }
    }

    /// Làm sạch URL: loại bỏ toàn bộ tracking rác, đưa về URL chuẩn
    pub fn clean_url(&self, url_str: &str, platform: &Platform) -> String {
        let parsed = match Url::parse(url_str) {
            Ok(u) => u,
            Err(_) => return url_str.to_string(),
        };

        match platform {
            Platform::Shopee => {
                // Thử bóc tách shopid & itemid
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
                // Nếu không match regex shopid/itemid, xóa sạch query params
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
                // Xóa query param rác
                format!("{}://{}{}", parsed.scheme(), parsed.host_str().unwrap_or(""), parsed.path())
            }
        }
    }

    /// Xây dựng link Affiliate có gắn SubID người dùng và người giới thiệu (như Shopback)
    pub fn build_affiliate_url(
        &self,
        clean_url: &str,
        template: &str,
        user_id: i64,
        referrer_id: Option<i64>,
        click_id: &str,
    ) -> (String, String) {
        let ref_str = referrer_id.map(|id| id.to_string()).unwrap_or_else(|| "0".to_string());
        let sub_id = format!("u{}_r{}_{}", user_id, ref_str, &click_id[..8]);

        let encoded_clean = urlencoding::encode(clean_url);
        let aff_url = template
            .replace("{CLEAN_URL}", &encoded_clean)
            .replace("{USER_ID}", &user_id.to_string())
            .replace("{REF_ID}", &ref_str)
            .replace("{CLICK_ID}", click_id);

        (aff_url, sub_id)
    }

    /// Xử lý toàn bộ chu trình phân tích deal từ tin nhắn người dùng
    pub async fn process_link(
        &self,
        input_text: &str,
        user_id: i64,
        referrer_id: Option<i64>,
        template: &str,
    ) -> Result<ParsedDeal> {
        let raw_url = self
            .extract_first_url(input_text)
            .ok_or_else(|| anyhow!("Không tìm thấy đường link hợp lệ trong tin nhắn"))?;

        let initial_platform = self.detect_platform(&raw_url);
        let resolved_url = self.resolve_url(&raw_url).await;
        let platform = if initial_platform == Platform::Unknown {
            self.detect_platform(&resolved_url)
        } else {
            initial_platform
        };

        if platform == Platform::Unknown {
            return Err(anyhow!("Hiện tại bot hỗ trợ hoàn tiền cho các sàn: Shopee, Lazada, Tiki, TikTok Shop. Vui lòng gửi link thuộc các sàn này."));
        }

        let clean_url = self.clean_url(&resolved_url, &platform);
        let click_id = Uuid::new_v4().simple().to_string();

        let (affiliate_url, sub_id) = self.build_affiliate_url(
            &clean_url,
            template,
            user_id,
            referrer_id,
            &click_id,
        );

        let estimated_cashback_text = match platform {
            Platform::Shopee => "Hoàn 3.0% - 10.5% (Tối đa 150,000₫/đơn)",
            Platform::Lazada => "Hoàn 2.5% - 8.0% (Tối đa 100,000₫/đơn)",
            Platform::Tiki => "Hoàn 1.5% - 5.0% (Tối đa 80,000₫/đơn)",
            Platform::TikTokShop => "Hoàn 3.0% - 12.0% (Tối đa 120,000₫/đơn)",
            Platform::Unknown => "Hoàn tiền lên tới 10%",
        }
        .to_string();

        Ok(ParsedDeal {
            platform,
            original_url: raw_url,
            resolved_url,
            clean_url,
            affiliate_url,
            click_id,
            sub_id,
            estimated_cashback_text,
        })
    }
}

// Module hỗ trợ urlencoding nhẹ nhàng không cần kéo thêm crate lớn
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
    fn test_extract_url() {
        let parser = DealParser::new();
        let text = "Mọi người mua sản phẩm này nhé: https://shopee.vn/product/123/456 giá cực rẻ";
        assert_eq!(
            parser.extract_first_url(text),
            Some("https://shopee.vn/product/123/456".to_string())
        );
    }

    #[test]
    fn test_clean_shopee_url() {
        let parser = DealParser::new();
        let url = "https://shopee.vn/Ao-Thun-Nam-i.123456.7891011?spm=123&utm_source=fb";
        let clean = parser.clean_url(url, &Platform::Shopee);
        assert_eq!(clean, "https://shopee.vn/product/123456/7891011");
    }

    #[test]
    fn test_build_affiliate_url() {
        let parser = DealParser::new();
        let template = "https://aff.com/?url={CLEAN_URL}&u={USER_ID}&r={REF_ID}&c={CLICK_ID}";
        let (aff_url, sub_id) = parser.build_affiliate_url(
            "https://shopee.vn/product/1/2",
            template,
            9999,
            Some(8888),
            "testclick123",
        );
        assert!(aff_url.contains("u=9999"));
        assert!(aff_url.contains("r=8888"));
        assert!(sub_id.starts_with("u9999_r8888_testclic"));
    }
}
