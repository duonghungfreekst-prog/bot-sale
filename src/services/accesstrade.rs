use std::sync::Arc;
use std::time::Duration;
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::deal_parser::Platform;

const CACHE_TTL_SECS: i64 = 300; // Cache danh sách chiến dịch 5 phút để tránh rate limit AccessTrade

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ApprovedCampaignSummary {
    pub id: String,
    pub merchant: String,
    pub name: String,
    pub platform_group: String,
}

#[derive(Clone, Debug, Default)]
pub struct ApprovedCampaigns {
    pub shopee: Option<(String, String)>,
    pub lazada: Option<(String, String)>,
    pub tiki: Option<(String, String)>,
    pub tiktok: Option<(String, String)>,
    pub all_approved: Vec<ApprovedCampaignSummary>,
    pub fetched_at_epoch: i64,
}

#[derive(Clone, Debug)]
pub struct AffiliateLinkResult {
    pub url: String,
    pub campaign_id: String,
}

#[derive(Deserialize, Debug)]
struct CampaignsApiResponse {
    data: Option<Vec<CampaignRaw>>,
}

#[derive(Deserialize, Debug)]
struct CampaignRaw {
    id: serde_json::Value,
    name: Option<String>,
    merchant: Option<String>,
    approval: Option<String>,
    status: Option<i64>,
}

#[derive(Serialize)]
struct ProductLinkCreateReq<'a> {
    campaign_id: &'a str,
    urls: [&'a str; 1],
    url_enc: bool,
    utm_source: &'a str,
    utm_medium: &'a str,
    utm_campaign: &'a str,
    sub1: String,
    sub3: &'a str,
}

#[derive(Deserialize, Debug)]
struct ProductLinkCreateResp {
    success: Option<bool>,
    message: Option<String>,
    data: Option<ProductLinkData>,
}

#[derive(Deserialize, Debug)]
struct ProductLinkData {
    success_link: Option<Vec<SuccessLinkItem>>,
}

#[derive(Deserialize, Debug)]
struct SuccessLinkItem {
    aff_link: Option<String>,
    short_link: Option<String>,
}

#[derive(Serialize)]
struct TikTokFeedCreateReq<'a> {
    product_url: &'a str,
    utm_source: &'a str,
    utm_medium: &'a str,
    utm_campaign: &'a str,
    sub1: String,
    sub3: &'a str,
}

#[derive(Deserialize, Debug)]
struct TikTokFeedCreateResp {
    status: Option<bool>,
    data: Option<TikTokFeedLinkData>,
}

#[derive(Deserialize, Debug)]
struct TikTokFeedLinkData {
    aff_short_url: Option<String>,
    aff_url: Option<String>,
}

/// Client giao tiếp với ACCESSTRADE Publisher API v1/v2
pub struct AccessTradeClient {
    http: Client,
    cache: Arc<RwLock<Option<(String, ApprovedCampaigns)>>>,
}

impl AccessTradeClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            http,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Xóa cache khi Admin cập nhật token mới
    pub async fn invalidate_cache(&self) {
        let mut guard = self.cache.write().await;
        *guard = None;
    }

    /// Lấy danh sách các chiến dịch đã được duyệt trên tài khoản ACCESSTRADE (có cache 5 phút)
    pub async fn get_approved_campaigns(&self, api_token: &str, force_refresh: bool) -> Result<ApprovedCampaigns, String> {
        let token = api_token.trim();
        if token.is_empty() {
            return Err("Chưa cấu hình ACCESSTRADE API Token".to_string());
        }

        let now = Utc::now().timestamp();
        if !force_refresh {
            let guard = self.cache.read().await;
            if let Some((cached_token, cached)) = guard.as_ref() {
                if cached_token == token && (now - cached.fetched_at_epoch) < CACHE_TTL_SECS {
                    return Ok(cached.clone());
                }
            }
        }

        let mut combined = ApprovedCampaigns {
            fetched_at_epoch: now,
            ..Default::default()
        };

        // ACCESSTRADE API giới hạn tối đa 50 bản ghi mỗi trang (limit <= 50)
        for page in 1..=10 {
            let url = format!(
                "https://api.accesstrade.vn/v1/campaigns?approval=successful&limit=50&page={}",
                page
            );
            let resp = self
                .http
                .get(&url)
                .header("Authorization", format!("Token {}", token))
                .header("Content-Type", "application/json")
                .send()
                .await
                .map_err(|e| format!("Lỗi kết nối ACCESSTRADE API: {}", e))?;

            if !resp.status().is_success() {
                return Err(format!("ACCESSTRADE trả về mã lỗi HTTP {}", resp.status()));
            }

            let body_text = resp
                .text()
                .await
                .map_err(|e| format!("Không thể đọc phản hồi từ ACCESSTRADE: {}", e))?;

            let (page_res, raw_count) = parse_approved_campaigns_page(&body_text, now)?;
            merge_approved_campaigns(&mut combined, page_res);

            if raw_count < 50 {
                break;
            }
        }

        let parsed = combined;
        {
            let mut guard = self.cache.write().await;
            *guard = Some((token.to_string(), parsed.clone()));
        }

        info!(
            "✅ Đã đồng bộ ACCESSTRADE: {} chiến dịch đã duyệt (Shopee={}, Lazada={}, Tiki={}, TikTok={})",
            parsed.all_approved.len(),
            parsed.shopee.is_some(),
            parsed.lazada.is_some(),
            parsed.tiki.is_some(),
            parsed.tiktok.is_some()
        );

        Ok(parsed)
    }

    /// Tạo link Affiliate thực tế qua ACCESSTRADE API (gắn nguồn utm_source=facebook + SubID)
    pub async fn create_affiliate_link(
        &self,
        api_token: &str,
        platform: &Platform,
        clean_url: &str,
        user_id: i64,
        click_id: &str,
        override_campaign_id: Option<&str>,
    ) -> Result<AffiliateLinkResult, String> {
        let token = api_token.trim();
        if token.is_empty() {
            return Err("Chưa có ACCESSTRADE API Token".to_string());
        }

        // Ưu tiên Campaign ID do Admin nhập tay; nếu trống thì tự lấy từ các chiến dịch đã duyệt
        let campaign_id = if let Some(cid) = override_campaign_id.map(str::trim).filter(|s| !s.is_empty()) {
            cid.to_string()
        } else {
            let approved = self.get_approved_campaigns(token, false).await?;
            let matched = match platform {
                Platform::Shopee => approved.shopee,
                Platform::Lazada => approved.lazada,
                Platform::Tiki => approved.tiki,
                Platform::TikTokShop => approved.tiktok,
                Platform::Unknown => None,
            };

            match matched {
                Some((id, _)) => id,
                None => {
                    return Err(format!(
                        "Chiến dịch {} chưa được duyệt trên tài khoản ACCESSTRADE của bạn",
                        platform.as_str()
                    ));
                }
            }
        };

        // Gọi v1/product_link/create (áp dụng chung cho Shopee, Lazada, Tiki và TikTok Shop CPS)
        match self
            .call_v1_product_link(token, &campaign_id, clean_url, user_id, click_id)
            .await
        {
            Ok(url) => Ok(AffiliateLinkResult { url, campaign_id }),
            Err(err) => {
                // Nếu là TikTok Shop và v1 không nhận URL, thử gọi v2/tiktokshop_product_feeds/create_link
                if *platform == Platform::TikTokShop {
                    if let Ok(v2_url) = self.call_v2_tiktok_feed(token, clean_url, user_id, click_id).await {
                        return Ok(AffiliateLinkResult {
                            url: v2_url,
                            campaign_id,
                        });
                    }
                }
                warn!("⚠️ Tạo link ACCESSTRADE thất bại cho {}: {}", platform.as_str(), err);
                Err(err)
            }
        }
    }

    async fn call_v1_product_link(
        &self,
        token: &str,
        campaign_id: &str,
        clean_url: &str,
        user_id: i64,
        click_id: &str,
    ) -> Result<String, String> {
        let payload = ProductLinkCreateReq {
            campaign_id,
            urls: [clean_url],
            url_enc: true,
            utm_source: "facebook",
            utm_medium: "social",
            utm_campaign: "dealx_bot",
            sub1: user_id.to_string(),
            sub3: click_id,
        };

        let resp = self
            .http
            .post("https://api.accesstrade.vn/v1/product_link/create")
            .header("Authorization", format!("Token {}", token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Lỗi gọi API product_link/create: {}", e))?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| format!("Lỗi đọc body: {}", e))?;

        if !status.is_success() {
            return Err(format!("HTTP {}: {}", status, text));
        }

        let parsed: ProductLinkCreateResp =
            serde_json::from_str(&text).map_err(|e| format!("Lỗi parse JSON: {}", e))?;

        if parsed.success == Some(false) {
            return Err(parsed.message.unwrap_or_else(|| "Tạo link không thành công".to_string()));
        }

        if let Some(data) = parsed.data {
            if let Some(links) = data.success_link {
                if let Some(first) = links.first() {
                    if let Some(short) = first.short_link.as_deref().filter(|s| !s.trim().is_empty()) {
                        return Ok(short.to_string());
                    }
                    if let Some(aff) = first.aff_link.as_deref().filter(|s| !s.trim().is_empty()) {
                        return Ok(aff.to_string());
                    }
                }
            }
        }

        Err("ACCESSTRADE không trả về đường link hợp lệ".to_string())
    }

    async fn call_v2_tiktok_feed(
        &self,
        token: &str,
        product_url: &str,
        user_id: i64,
        click_id: &str,
    ) -> Result<String, String> {
        let payload = TikTokFeedCreateReq {
            product_url,
            utm_source: "facebook",
            utm_medium: "social",
            utm_campaign: "dealx_bot",
            sub1: user_id.to_string(),
            sub3: click_id,
        };

        let resp = self
            .http
            .post("https://api.accesstrade.vn/v2/tiktokshop_product_feeds/create_link")
            .header("Authorization", format!("Token {}", token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Lỗi gọi v2 tiktokshop: {}", e))?;

        let text = resp.text().await.map_err(|e| format!("Lỗi đọc body v2: {}", e))?;
        let parsed: TikTokFeedCreateResp =
            serde_json::from_str(&text).map_err(|e| format!("Lỗi parse JSON v2: {}", e))?;

        if parsed.status == Some(true) {
            if let Some(data) = parsed.data {
                if let Some(short) = data.aff_short_url.as_deref().filter(|s| !s.trim().is_empty()) {
                    return Ok(short.to_string());
                }
                if let Some(aff) = data.aff_url.as_deref().filter(|s| !s.trim().is_empty()) {
                    return Ok(aff.to_string());
                }
            }
        }

        Err("Sản phẩm không thuộc danh sách TikTok Product Feed".to_string())
    }
}

/// Phân tích 1 trang JSON danh sách chiến dịch đã duyệt và trả về (Kết quả, tổng số item thô trong trang)
pub fn parse_approved_campaigns_page(json_str: &str, now_epoch: i64) -> Result<(ApprovedCampaigns, usize), String> {
    let resp: CampaignsApiResponse =
        serde_json::from_str(json_str).map_err(|e| format!("Lỗi phân tích JSON chiến dịch: {}", e))?;

    let mut result = ApprovedCampaigns {
        fetched_at_epoch: now_epoch,
        ..Default::default()
    };

    let items = resp.data.unwrap_or_default();
    let raw_count = items.len();

    for item in items {
        let approval = item.approval.as_deref().unwrap_or("");
        let status = item.status.unwrap_or(0);
        if approval != "successful" || status != 1 {
            continue;
        }

        let id_str = match &item.id {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => continue,
        };
        let merchant = item.merchant.unwrap_or_default();
        let name = item.name.unwrap_or_default();
        let m_lower = merchant.to_lowercase();
        let n_lower = name.to_lowercase();

        let platform_group = if is_shopee_campaign(&m_lower, &n_lower) {
            if result.shopee.is_none() || m_lower == "shopee" || m_lower == "shopee_kolnew" {
                result.shopee = Some((id_str.clone(), name.clone()));
            }
            "Shopee"
        } else if is_lazada_campaign(&m_lower, &n_lower) {
            if result.lazada.is_none() || m_lower == "lazada_kol" || m_lower == "lazada_cashback" {
                result.lazada = Some((id_str.clone(), name.clone()));
            }
            "Lazada"
        } else if is_tiki_campaign(&m_lower, &n_lower) {
            if result.tiki.is_none() || m_lower == "tikivn" {
                result.tiki = Some((id_str.clone(), name.clone()));
            }
            "Tiki"
        } else if is_tiktok_campaign(&m_lower, &n_lower) {
            if result.tiktok.is_none() || m_lower == "tiktok_cps" {
                result.tiktok = Some((id_str.clone(), name.clone()));
            }
            "TikTok Shop"
        } else {
            "Khác"
        };

        result.all_approved.push(ApprovedCampaignSummary {
            id: id_str,
            merchant,
            name,
            platform_group: platform_group.to_string(),
        });
    }

    Ok((result, raw_count))
}

fn merge_approved_campaigns(target: &mut ApprovedCampaigns, mut page: ApprovedCampaigns) {
    if target.shopee.is_none() {
        target.shopee = page.shopee.take();
    }
    if target.lazada.is_none() {
        target.lazada = page.lazada.take();
    }
    if target.tiki.is_none() {
        target.tiki = page.tiki.take();
    }
    if target.tiktok.is_none() {
        target.tiktok = page.tiktok.take();
    }
    target.all_approved.append(&mut page.all_approved);
}

fn is_shopee_campaign(merchant: &str, _name: &str) -> bool {
    matches!(
        merchant,
        "shopee" | "shopee_kolnew" | "shopee_mcn" | "shopee_addlostvn08" | "shopee_kol" | "shopee_cps" | "shopee_smartlink"
    )
}

fn is_lazada_campaign(merchant: &str, _name: &str) -> bool {
    matches!(
        merchant,
        "lazada_kol" | "lazada_cashback" | "lazada" | "lazada_cps" | "lazada_vn"
    )
}

fn is_tiki_campaign(merchant: &str, _name: &str) -> bool {
    matches!(merchant, "tikivn" | "tiki_creator" | "tiki_cashback" | "tiki")
}

fn is_tiktok_campaign(merchant: &str, name: &str) -> bool {
    matches!(merchant, "tiktok_cps" | "tiktokshop_cps" | "tiktok_shop")
        || (merchant.contains("tiktok") && !merchant.contains("business") && name.contains("shop"))
}

/// Che giấu API Token khi hiển thị trên giao diện Web Admin (chỉ hiện 4 ký tự đầu + cuối)
pub fn mask_token(token: &str) -> String {
    let chars: Vec<char> = token.trim().chars().collect();
    if chars.len() <= 8 {
        return "********".to_string();
    }
    let start: String = chars[..4].iter().collect();
    let end: String = chars[chars.len() - 4..].iter().collect();
    format!("{}••••••••{}", start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_approved_campaigns() {
        let sample = r#"{
            "data": [
                {"id": 6648523843406889655, "merchant": "tiktok_cps", "name": "TIKTOK SHOP CPS", "approval": "successful", "status": 1},
                {"id": "4751584435713464237", "merchant": "shopee", "name": "Shopee Việt Nam", "approval": "unregistered", "status": 1},
                {"id": "7105125015509882615", "merchant": "romand_shopee", "name": "Romand Shopee PUB", "approval": "successful", "status": 1},
                {"id": "5087153089503673507", "merchant": "lazada_kol", "name": "Lazada Việt Nam", "approval": "successful", "status": 1}
            ]
        }"#;

        let (parsed, count) = parse_approved_campaigns_page(sample, 1000).unwrap();
        assert_eq!(count, 4);
        assert_eq!(parsed.tiktok.as_ref().unwrap().0, "6648523843406889655");
        assert!(parsed.shopee.is_none()); // unregistered & brand-only shopee must NOT match
        assert_eq!(parsed.lazada.as_ref().unwrap().0, "5087153089503673507");
        assert_eq!(parsed.all_approved.len(), 3);
    }

    #[test]
    fn test_mask_token() {
        assert_eq!(mask_token("1234567890abcdef"), "1234••••••••cdef");
        assert_eq!(mask_token("short"), "********");
    }
}
