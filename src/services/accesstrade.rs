use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use chrono::Utc;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::deal_parser::Platform;

const CACHE_TTL_SECS: i64 = 300; // Cache danh sách chiến dịch 5 phút để tránh rate limit AccessTrade
const VOUCHER_CACHE_TTL_SECS: i64 = 600; // Cache kho mã giảm giá 10 phút

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoucherOffer {
    pub platform: String,
    pub shop_name: String,
    pub title: String,
    pub coupon_code: String,
    pub discount_percent: u32,
    pub discount_desc: String,
    pub min_spend: f64,
    pub raw_url: String,
    pub aff_url: Option<String>,
    pub end_time: String,
}

impl VoucherOffer {
    /// Chỉ dùng tracking link ACCESSTRADE khi chiến dịch sàn tương ứng đã duyệt (tránh lỗi HTTP 500 từ go.isclix.com)
    pub fn best_target_url(&self, approved: Option<&ApprovedCampaigns>) -> &str {
        let is_approved = match self.platform.as_str() {
            "Shopee" => approved.and_then(|a| a.shopee.as_ref()).is_some(),
            "Lazada" => approved.and_then(|a| a.lazada.as_ref()).is_some(),
            "Tiki" => approved.and_then(|a| a.tiki.as_ref()).is_some(),
            "TikTok Shop" => approved.and_then(|a| a.tiktok.as_ref()).is_some(),
            _ => false,
        };
        if is_approved {
            if let Some(ref aff) = self.aff_url {
                if !aff.trim().is_empty() {
                    return aff;
                }
            }
        }
        &self.raw_url
    }
}

#[derive(Deserialize, Debug)]
struct OffersApiResponse {
    data: Option<Vec<OfferRaw>>,
}

#[derive(Deserialize, Debug)]
struct OfferRaw {
    name: Option<String>,
    merchant: Option<String>,
    domain: Option<String>,
    link: Option<String>,
    aff_link: Option<String>,
    end_time: Option<String>,
    discount_percentage: Option<serde_json::Value>,
    coupons: Option<Vec<OfferCouponRaw>>,
}

#[derive(Deserialize, Debug)]
struct OfferCouponRaw {
    coupon_code: Option<String>,
    coupon_desc: Option<String>,
}

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
    #[allow(dead_code)]
    pub campaign_id: String,
    pub product_name: Option<String>,
    #[allow(dead_code)]
    pub product_price_vnd: Option<f64>,
    #[allow(dead_code)]
    pub commission_amount_vnd: Option<f64>,
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
    product_name: Option<String>,
    product_price: Option<TikTokAmountInfo>,
    product_commission: Option<TikTokAmountInfo>,
}

#[derive(Deserialize, Debug)]
struct TikTokAmountInfo {
    amount: Option<serde_json::Value>,
    minimum_amount: Option<serde_json::Value>,
}

/// Client giao tiếp với ACCESSTRADE Publisher API v1/v2
pub struct AccessTradeClient {
    http: Client,
    cache: Arc<RwLock<Option<(String, ApprovedCampaigns)>>>,
    voucher_cache: Arc<RwLock<Option<(String, i64, u32, Vec<VoucherOffer>)>>>,
}

impl AccessTradeClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(12))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            http,
            cache: Arc::new(RwLock::new(None)),
            voucher_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Xóa cache khi Admin cập nhật token mới
    pub async fn invalidate_cache(&self) {
        let mut guard = self.cache.write().await;
        *guard = None;
        let mut v_guard = self.voucher_cache.write().await;
        *v_guard = None;
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

        // Với TikTok Shop: Ưu tiên gọi API v2/tiktokshop_product_feeds/create_link trước
        // vì v2 sinh ra link TikTok Universal (/t/...) kèm chain_key & enable_shop_tab_popup=1 giúp nhảy thẳng vào sản phẩm trên App TikTok
        if *platform == Platform::TikTokShop {
            match self.call_v2_tiktok_feed(token, clean_url, &campaign_id, user_id, click_id).await {
                Ok(res) => return Ok(res),
                Err(v2_err) => {
                    info!("API v2 TikTok Shop không áp dụng cho URL này ({}), chuyển sang v1: {}", clean_url, v2_err);
                }
            }
        }

        // Gọi v1/product_link/create (áp dụng cho Shopee, Lazada, Tiki và fallback cho TikTok)
        match self
            .call_v1_product_link(token, &campaign_id, clean_url, user_id, click_id)
            .await
        {
            Ok(url) => Ok(AffiliateLinkResult {
                url,
                campaign_id,
                product_name: None,
                product_price_vnd: None,
                commission_amount_vnd: None,
            }),
            Err(err) => {
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
        campaign_id: &str,
        user_id: i64,
        click_id: &str,
    ) -> Result<AffiliateLinkResult, String> {
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
                let target_url = data
                    .aff_short_url
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| data.aff_url.as_deref().filter(|s| !s.trim().is_empty()))
                    .map(|s| s.to_string());

                if let Some(url) = target_url {
                    let product_price_vnd = data
                        .product_price
                        .as_ref()
                        .and_then(|p| parse_json_f64(p.minimum_amount.as_ref()));
                    let commission_amount_vnd = data
                        .product_commission
                        .as_ref()
                        .and_then(|c| parse_json_f64(c.amount.as_ref()));
                    let product_name = data
                        .product_name
                        .filter(|s| !s.trim().is_empty());

                    return Ok(AffiliateLinkResult {
                        url,
                        campaign_id: campaign_id.to_string(),
                        product_name,
                        product_price_vnd,
                        commission_amount_vnd,
                    });
                }
            }
        }

        Err("Sản phẩm không thuộc danh sách TikTok Product Feed".to_string())
    }

    /// Xem nhanh số lượng voucher giảm sâu đang nằm sẵn trong RAM cache (không gọi mạng)
    pub async fn peek_cached_high_discount_vouchers(&self) -> Vec<VoucherOffer> {
        let guard = self.voucher_cache.read().await;
        guard
            .as_ref()
            .map(|(_, _, _, list)| list.clone())
            .unwrap_or_default()
    }

    /// Quét danh sách mã giảm giá thật >= min_discount_pct% từ ACCESSTRADE v1/offers_informations (có cache 10 phút)
    pub async fn fetch_high_discount_vouchers(
        &self,
        api_token: &str,
        min_discount_pct: u32,
        force_refresh: bool,
    ) -> Result<Vec<VoucherOffer>, String> {
        let token = api_token.trim();
        if token.is_empty() {
            return Err("Chưa cấu hình ACCESSTRADE API Token".to_string());
        }

        let now = Utc::now().timestamp();
        if !force_refresh {
            let guard = self.voucher_cache.read().await;
            if let Some((cached_token, fetched_at, cached_min, list)) = guard.as_ref() {
                if cached_token == token
                    && *cached_min <= min_discount_pct
                    && (now - *fetched_at) < VOUCHER_CACHE_TTL_SECS
                {
                    let filtered: Vec<VoucherOffer> = list
                        .iter()
                        .filter(|v| v.discount_percent >= min_discount_pct)
                        .cloned()
                        .collect();
                    return Ok(filtered);
                }
            }
        }

        let vn_now = Utc::now() + chrono::Duration::hours(7);
        let today_ymd = vn_now.format("%Y-%m-%d").to_string();

        let mut collected: Vec<VoucherOffer> = Vec::new();
        let mut seen_codes: HashSet<(String, String)> = HashSet::new();

        // Quét tối đa 12 trang (600 offers mới nhất), có tự động nghỉ nhẹ chống Rate Limit HTTP 429
        for page in 1..=12 {
            let url = format!(
                "https://api.accesstrade.vn/v1/offers_informations?status=1&limit=50&page={}",
                page
            );
            let mut body_text_opt = None;
            for attempt in 1..=2 {
                let resp = self
                    .http
                    .get(&url)
                    .header("Authorization", format!("Token {}", token))
                    .header("Content-Type", "application/json")
                    .send()
                    .await
                    .map_err(|e| format!("Lỗi kết nối kho mã ACCESSTRADE: {}", e))?;

                if resp.status().as_u16() == 429 && attempt == 1 {
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                    continue;
                }

                if !resp.status().is_success() {
                    return Err(format!(
                        "ACCESSTRADE Offers API trả về lỗi HTTP {}",
                        resp.status()
                    ));
                }

                body_text_opt = Some(
                    resp.text()
                        .await
                        .map_err(|e| format!("Không thể đọc JSON offers: {}", e))?,
                );
                break;
            }

            let Some(body_text) = body_text_opt else {
                break;
            };

            let (page_offers, raw_count) =
                parse_offers_informations_page(&body_text, &today_ymd, min_discount_pct)?;

            for offer in page_offers {
                let key = (offer.platform.clone(), offer.coupon_code.to_uppercase());
                if seen_codes.insert(key) {
                    collected.push(offer);
                }
            }

            if raw_count < 50 || collected.len() >= 100 {
                break;
            }

            tokio::time::sleep(Duration::from_millis(350)).await;
        }

        // Sắp xếp % giảm cao -> đơn tối thiểu thấp -> tên shop
        collected.sort_by(|a, b| {
            b.discount_percent
                .cmp(&a.discount_percent)
                .then_with(|| {
                    a.min_spend
                        .partial_cmp(&b.min_spend)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.shop_name.cmp(&b.shop_name))
        });

        collected.truncate(100);

        {
            let mut guard = self.voucher_cache.write().await;
            *guard = Some((token.to_string(), now, min_discount_pct, collected.clone()));
        }

        info!(
            "🎟️ Đã đồng bộ kho {} mã giảm giá >={}% thật từ ACCESSTRADE API",
            collected.len(),
            min_discount_pct
        );

        Ok(collected)
    }
}

fn parse_json_f64(val: Option<&serde_json::Value>) -> Option<f64> {
    match val? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
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

    #[test]
    fn test_parse_offers_informations_page_filters_high_discount_and_expiry() {
        let sample = r#"{
            "data": [
                {
                    "merchant": "shopee",
                    "domain": "shopee.vn",
                    "name": "[Langsdom Vietnam Store]-Giảm 35%-tối đa 110,000 VNĐ cho đơn tối thiểu 300,000 VNĐ",
                    "link": "https://shopee.vn/search?voucherCode=LANGXDKKP",
                    "aff_link": "https://go.isclix.com/deep_link/123?url=abc",
                    "end_time": "2026-11-24",
                    "discount_percentage": 0,
                    "coupons": [{"coupon_code": "LANGXDKKP", "coupon_desc": "Giảm 35%-tối đa 110,000 VNĐ cho đơn tối thiểu 300,000 VNĐ"}]
                },
                {
                    "merchant": "shopee",
                    "domain": "shopee.vn",
                    "name": "[Old Expired Shop]-Giảm 30% đơn từ 100k",
                    "link": "https://shopee.vn/search?voucherCode=OLD30",
                    "aff_link": "https://go.isclix.com/deep_link/123?url=old",
                    "end_time": "2026-11-01",
                    "coupons": [{"coupon_code": "OLD30", "coupon_desc": "Giảm 30%"}]
                },
                {
                    "merchant": "shopee",
                    "domain": "shopee.vn",
                    "name": "[Small Voucher]-Giảm 10%-tối đa 10,000 VNĐ cho đơn tối thiểu 50,000 VNĐ",
                    "link": "https://shopee.vn/search?voucherCode=SMALL10",
                    "end_time": "2026-12-30",
                    "coupons": [{"coupon_code": "SMALL10", "coupon_desc": "Giảm 10%"}]
                },
                {
                    "merchant": "tikivn",
                    "domain": "tiki.vn",
                    "name": "[Tiki Partner]-Giảm 25% tối đa 200K đơn từ 500k",
                    "link": "https://tiki.vn/khuyen-mai",
                    "aff_link": "https://go.isclix.com/deep_link/tiki",
                    "end_time": "2026/12/31",
                    "coupons": [{"coupon_code": "TIKI25VIP", "coupon_desc": "Giảm 25% tối đa 200K"}]
                }
            ]
        }"#;

        let (offers, raw_count) = parse_offers_informations_page(sample, "2026-11-16", 20).unwrap();
        assert_eq!(raw_count, 4);
        assert_eq!(offers.len(), 2);
        assert_eq!(offers[0].coupon_code, "LANGXDKKP");
        assert_eq!(offers[0].discount_percent, 35);
        assert_eq!(offers[0].shop_name, "Langsdom Vietnam Store");
        assert_eq!(offers[0].min_spend, 300000.0);

        assert_eq!(offers[1].coupon_code, "TIKI25VIP");
        assert_eq!(offers[1].platform, "Tiki");
        assert_eq!(offers[1].discount_percent, 25);
        assert_eq!(offers[1].end_time, "2026-12-31");
    }
}

/// Phân tích 1 trang JSON từ ACCESSTRADE v1/offers_informations, giữ các mã còn hạn >= min_discount_pct%
pub fn parse_offers_informations_page(
    json_str: &str,
    today_ymd: &str,
    min_discount_pct: u32,
) -> Result<(Vec<VoucherOffer>, usize), String> {
    let resp: OffersApiResponse = serde_json::from_str(json_str)
        .map_err(|e| format!("Lỗi phân tích JSON offers ACCESSTRADE: {}", e))?;

    let items = resp.data.unwrap_or_default();
    let raw_count = items.len();
    let mut results = Vec::new();

    for item in items {
        let end_time_norm = item
            .end_time
            .as_deref()
            .unwrap_or("")
            .trim()
            .replace('/', "-");
        let end_date_prefix = end_time_norm.get(..10).unwrap_or(&end_time_norm);
        if end_date_prefix.len() == 10 && end_date_prefix < today_ymd {
            continue;
        }

        let raw_url = match item.link.as_deref().map(str::trim) {
            Some(u) if u.starts_with("https://") || u.starts_with("http://") => u.to_string(),
            _ => continue,
        };
        let aff_url = item
            .aff_link
            .as_deref()
            .map(str::trim)
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
            .map(|u| u.to_string());

        let platform = detect_offer_platform(
            item.merchant.as_deref().unwrap_or(""),
            item.domain.as_deref().unwrap_or(""),
            &raw_url,
        );
        let raw_name = item.name.as_deref().unwrap_or("").trim();
        let (shop_name, clean_desc) = split_shop_and_desc(raw_name, &platform);
        let api_pct = parse_json_f64(item.discount_percentage.as_ref())
            .map(|v| v.round() as u32)
            .unwrap_or(0);

        let coupons = item.coupons.unwrap_or_default();
        for c in coupons {
            let code = c.coupon_code.unwrap_or_default().trim().to_string();
            if code.is_empty() {
                continue;
            }
            let desc_candidate = c
                .coupon_desc
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(clean_desc);

            let pct = if (1..=100).contains(&api_pct) {
                api_pct
            } else {
                extract_discount_percent(desc_candidate)
                    .or_else(|| extract_discount_percent(raw_name))
                    .unwrap_or(0)
            };

            if pct < min_discount_pct {
                continue;
            }

            let min_spend = extract_min_spend(raw_name)
                .or_else(|| extract_min_spend(desc_candidate))
                .unwrap_or(0.0);

            results.push(VoucherOffer {
                platform: platform.clone(),
                shop_name: shop_name.clone(),
                title: clean_desc.to_string(),
                coupon_code: code,
                discount_percent: pct,
                discount_desc: desc_candidate.to_string(),
                min_spend,
                raw_url: raw_url.clone(),
                aff_url: aff_url.clone(),
                end_time: if end_date_prefix.is_empty() {
                    "Đang hoạt động".to_string()
                } else {
                    end_date_prefix.to_string()
                },
            });
        }
    }

    Ok((results, raw_count))
}

fn detect_offer_platform(merchant: &str, domain: &str, url: &str) -> String {
    let joined = format!("{} {} {}", merchant, domain, url).to_lowercase();
    if joined.contains("shopee") {
        "Shopee".to_string()
    } else if joined.contains("tiki") {
        "Tiki".to_string()
    } else if joined.contains("lazada") {
        "Lazada".to_string()
    } else if joined.contains("tiktok") {
        "TikTok Shop".to_string()
    } else {
        "Khác".to_string()
    }
}

fn split_shop_and_desc<'a>(name: &'a str, fallback_platform: &str) -> (String, &'a str) {
    if let Some(rest) = name.strip_prefix('[') {
        if let Some(end_idx) = rest.find(']') {
            let shop = rest[..end_idx].trim();
            let desc = rest[end_idx + 1..].trim().trim_start_matches('-').trim();
            if !shop.is_empty() {
                return (shop.to_string(), if desc.is_empty() { name } else { desc });
            }
        }
    }
    (fallback_platform.to_string(), name)
}

fn extract_discount_percent(text: &str) -> Option<u32> {
    static RE_PCT: std::sync::OnceLock<Option<Regex>> = std::sync::OnceLock::new();
    let re = RE_PCT
        .get_or_init(|| Regex::new(r"(?i)(?:giảm|sale|off)\s*(\d{1,2})\s*%").ok())
        .as_ref()?;
    let caps = re.captures(text)?;
    caps.get(1)?.as_str().parse::<u32>().ok().filter(|&p| p <= 95)
}

fn extract_min_spend(text: &str) -> Option<f64> {
    static RE_MIN: std::sync::OnceLock<Option<Regex>> = std::sync::OnceLock::new();
    let re = RE_MIN
        .get_or_init(|| Regex::new(r"(?i)(?:tối\s*thiểu|đơn\s*từ|từ)\s*([\d,\.]+)\s*(k\b|triệu|tr\b|vnđ|đ)?").ok())
        .as_ref()?;
    let caps = re.captures(text)?;
    let num_str = caps.get(1)?.as_str();
    let unit = caps.get(2).map(|m| m.as_str().to_lowercase()).unwrap_or_default();

    if unit == "k" {
        let base = num_str.replace(',', ".").parse::<f64>().ok()?;
        return Some(base * 1000.0);
    }
    if unit == "triệu" || unit == "tr" {
        let base = num_str.replace(',', ".").parse::<f64>().ok()?;
        return Some(base * 1_000_000.0);
    }
    let clean_digits: String = num_str.chars().filter(|c| c.is_ascii_digit()).collect();
    clean_digits.parse::<f64>().ok()
}
