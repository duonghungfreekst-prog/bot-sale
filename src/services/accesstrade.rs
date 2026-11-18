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

const CACHE_TTL_SECS: i64 = 180; // Cache danh sách chiến dịch 3 phút
const VOUCHER_CACHE_TTL_SECS: i64 = 55; // Tự động làm mới kho mã giảm giá mỗi 1 phút (55s)

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VoucherSyncMeta {
    pub last_sync_vn: String,
    pub sync_cycles: u64,
    pub total_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoucherOffer {
    pub platform: String,
    pub shop_name: String,
    pub title: String,
    pub coupon_code: String,
    pub discount_percent: u32,
    pub discount_amount: f64,
    pub discount_desc: String,
    pub min_spend: f64,
    pub raw_url: String,
    pub aff_url: Option<String>,
    pub end_time: String,
}

impl VoucherOffer {
    /// Trả về link đích chuẩn xác 100% (chỉ dùng aff_link khi chiến dịch có redirect hoạt động thực tế, tránh lỗi 404/500 từ go.isclix.com)
    pub fn best_target_url(&self, approved: Option<&ApprovedCampaigns>) -> &str {
        let active_camp_id = match self.platform.as_str() {
            "Shopee" => approved.and_then(|a| a.shopee.as_ref()).map(|(id, _)| id.as_str()),
            "Lazada" => approved.and_then(|a| a.lazada.as_ref()).map(|(id, _)| id.as_str()),
            "Tiki" => approved.and_then(|a| a.tiki.as_ref()).map(|(id, _)| id.as_str()),
            "TikTok Shop" => approved.and_then(|a| a.tiktok.as_ref()).map(|(id, _)| id.as_str()),
            _ => None,
        };
        if let (Some(cid), Some(aff)) = (active_camp_id, self.aff_url.as_deref()) {
            let aff_trimmed = aff.trim();
            // Chiến dịch TIKI (CPS) cũ (4348614231480407268 / v2/62) và link go.isclix.com thiếu campaign_id trong path bị lỗi HTTP 500 trên go.isclix.com
            let is_broken_legacy = cid == "4348614231480407268"
                || (aff_trimmed.contains("go.isclix.com/deep_link/") && !aff_trimmed.contains(cid));
            if !aff_trimmed.is_empty() && !is_broken_legacy {
                return aff_trimmed;
            }
        }
        &self.raw_url
    }

    /// Nhãn hiển thị mức giảm (ví dụ: "-35%", "-300K", "-45K", hoặc "ƯU ĐÃI")
    pub fn discount_badge(&self) -> String {
        if self.discount_percent > 0 {
            format!("-{}%", self.discount_percent)
        } else if self.discount_amount >= 1000.0 {
            let k = (self.discount_amount / 1000.0).round() as i64;
            format!("-{}K", k)
        } else if self.discount_amount > 0.0 {
            format!("-{}đ", self.discount_amount as i64)
        } else {
            "ƯU ĐÃI".to_string()
        }
    }
}

#[derive(Deserialize, Debug)]
struct OffersApiResponse {
    data: Option<Vec<OfferRaw>>,
}

#[derive(Deserialize, Debug)]
struct OfferRaw {
    name: Option<String>,
    content: Option<String>,
    merchant: Option<String>,
    domain: Option<String>,
    link: Option<String>,
    aff_link: Option<String>,
    end_time: Option<String>,
    discount_percentage: Option<serde_json::Value>,
    discount_value: Option<serde_json::Value>,
    min_spend: Option<serde_json::Value>,
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
    redirect_probe: Client,
    cache: Arc<RwLock<Option<(String, ApprovedCampaigns)>>>,
    voucher_cache: Arc<RwLock<Option<(String, i64, u32, Vec<VoucherOffer>)>>>,
    voucher_meta: Arc<RwLock<VoucherSyncMeta>>,
}

impl AccessTradeClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(12))
            .build()
            .unwrap_or_else(|_| Client::new());

        let probe_policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 4 {
                return attempt.stop();
            }
            let next_host = attempt.url().host_str().unwrap_or("");
            if is_tracking_gateway_host(next_host) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        });

        let redirect_probe = Client::builder()
            .timeout(Duration::from_secs(4))
            .redirect(probe_policy)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            http,
            redirect_probe,
            cache: Arc::new(RwLock::new(None)),
            voucher_cache: Arc::new(RwLock::new(None)),
            voucher_meta: Arc::new(RwLock::new(VoucherSyncMeta::default())),
        }
    }

    /// Xóa cache khi Admin cập nhật token mới
    pub async fn invalidate_cache(&self) {
        let mut guard = self.cache.write().await;
        *guard = None;
        let mut v_guard = self.voucher_cache.write().await;
        *v_guard = None;
    }

    /// Lấy thông tin thời gian đồng bộ kho mã gần nhất (phục vụ hiển thị thời gian thực trên Web & Bot)
    pub async fn get_voucher_sync_meta(&self) -> VoucherSyncMeta {
        self.voucher_meta.read().await.clone()
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
                Ok(res) => {
                    if self.verify_affiliate_redirect_ok(&res.url).await {
                        return Ok(res);
                    }
                    warn!("⚠️ Link TikTok v2 không vượt qua kiểm tra gateway, thử fallback v1.");
                }
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
            Ok(url) => {
                if !self.verify_affiliate_redirect_ok(&url).await {
                    return Err(format!(
                        "Gateway redirect ACCESSTRADE cho {} (campaign {}) tạm lỗi HTTP 404/500 — tự chuyển về link gốc an toàn",
                        platform.as_str(),
                        campaign_id
                    ));
                }
                Ok(AffiliateLinkResult {
                    url,
                    campaign_id,
                    product_name: None,
                    product_price_vnd: None,
                    commission_amount_vnd: None,
                })
            }
            Err(err) => {
                warn!("⚠️ Tạo link ACCESSTRADE thất bại cho {}: {}", platform.as_str(), err);
                Err(err)
            }
        }
    }

    /// Kiểm tra nhanh gateway redirect trung gian (shorten.asia -> go.isclix.com) có trả 302 hợp lệ hay lỗi 404/500
    pub async fn verify_affiliate_redirect_ok(&self, url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        let host = parsed.host_str().unwrap_or("");
        if !is_tracking_gateway_host(host) {
            return true;
        }

        match self.redirect_probe.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_client_error() || status.is_server_error() {
                    warn!(
                        "⚠️ Gateway {} trả về HTTP {} — chặn link hỏng để bảo vệ người dùng",
                        url, status
                    );
                    false
                } else {
                    true
                }
            }
            Err(err) => {
                warn!("⚠️ Không thể kiểm tra redirect của {}: {}", url, err);
                true // Không chặn nhầm nếu chỉ nghẽn timeout kiểm tra tạm thời
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

    /// Xem nhanh toàn bộ kho voucher thị trường đang nằm sẵn trong RAM cache (không gọi mạng)
    /// Lấy danh sách các chiến dịch đã duyệt đang nằm trong RAM cache (không gọi mạng)
    pub async fn peek_approved_campaigns(&self) -> Option<ApprovedCampaigns> {
        self.cache.read().await.as_ref().map(|(_, c)| c.clone())
    }

    pub async fn peek_cached_high_discount_vouchers(&self) -> Vec<VoucherOffer> {
        let guard = self.voucher_cache.read().await;
        guard
            .as_ref()
            .map(|(_, _, _, list)| list.clone())
            .unwrap_or_default()
    }

    /// Tìm các mã giảm giá trong kho khớp ĐÚNG với shop / thương hiệu / sản phẩm hoặc mã toàn sàn (tuyệt đối không nhét bừa mã shop khác)
    pub async fn search_matching_vouchers_for_deal(
        &self,
        platform: &str,
        product_title: &str,
        clean_url: &str,
        shop_name: Option<&str>,
        brand_name: Option<&str>,
        price_vnd: Option<f64>,
        max_items: usize,
    ) -> Vec<VoucherOffer> {
        let all = self.peek_cached_high_discount_vouchers().await;
        if all.is_empty() {
            return Vec::new();
        }
        match_vouchers_for_product(
            &all,
            platform,
            product_title,
            clean_url,
            shop_name,
            brand_name,
            price_vnd,
            max_items,
        )
    }

    /// Trả về link đích đã qua kiểm tra gateway redirect an toàn cho một mã giảm giá (không bao giờ đưa link chết 404/500)
    pub async fn verified_offer_url(
        &self,
        offer: &VoucherOffer,
        approved: Option<&ApprovedCampaigns>,
    ) -> String {
        let candidate = offer.best_target_url(approved);
        if candidate != offer.raw_url && !self.verify_affiliate_redirect_ok(candidate).await {
            return offer.raw_url.clone();
        }
        candidate.to_string()
    }

    /// Quét toàn bộ mã giảm giá trên thị trường từ tất cả các cổng ACCESSTRADE (hỗ trợ đồng bộ mỗi 60 giây)
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
                    let filtered: Vec<VoucherOffer> = if min_discount_pct == 0 {
                        list.clone()
                    } else {
                        list.iter()
                            .filter(|v| v.discount_percent >= min_discount_pct)
                            .cloned()
                            .collect()
                    };
                    return Ok(filtered);
                }
            }
        }

        let collected = self.sync_market_vouchers_internal(token, true).await?;
        let result = if min_discount_pct == 0 {
            collected
        } else {
            collected
                .into_iter()
                .filter(|v| v.discount_percent >= min_discount_pct)
                .collect()
        };
        Ok(result)
    }

    /// Chu kỳ đồng bộ nền mỗi 60 giây: quét nhanh trang mới nhất mỗi phút hoặc quét vét cạn mỗi 5 phút
    pub async fn sync_market_vouchers_tick(&self, api_token: &str, full_scan: bool) -> Result<usize, String> {
        let token = api_token.trim();
        if token.is_empty() {
            return Err("Chưa cấu hình ACCESSTRADE API Token".to_string());
        }
        let has_cache = {
            let guard = self.voucher_cache.read().await;
            guard.as_ref().map(|(t, _, _, list)| t == token && !list.is_empty()).unwrap_or(false)
        };
        let list = self.sync_market_vouchers_internal(token, full_scan || !has_cache).await?;
        Ok(list.len())
    }

    async fn sync_market_vouchers_internal(
        &self,
        token: &str,
        full_scan: bool,
    ) -> Result<Vec<VoucherOffer>, String> {
        let now = Utc::now().timestamp();
        let vn_now = Utc::now() + chrono::Duration::hours(7);
        let today_ymd = vn_now.format("%Y-%m-%d").to_string();

        let mut collected: Vec<VoucherOffer> = Vec::new();
        let mut seen_keys: HashSet<(String, String, String)> = HashSet::new();

        // Ở chế độ full_scan: quét vét cạn nhiều trang; ở chu kỳ nhanh 60s: quét trang mới nhất của tất cả cổng và gộp cache cũ
        let endpoint_groups: &[(&str, usize)] = if full_scan {
            &[
                ("https://api.accesstrade.vn/v1/offers_informations/coupon?limit=50", 3),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&is_hot=1&limit=50", 2),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&scope=expiring&limit=50", 2),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiki.vn&limit=50", 3),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=lazada.vn&limit=50", 2),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiktok.com&limit=50", 2),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&limit=50", 10),
            ]
        } else {
            &[
                ("https://api.accesstrade.vn/v1/offers_informations/coupon?limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&is_hot=1&limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiki.vn&limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=lazada.vn&limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiktok.com&limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&limit=50", 2),
            ]
        };

        for &(base_endpoint, max_pages) in endpoint_groups {
            for page in 1..=max_pages {
                let url = format!("{}&page={}", base_endpoint, page);
                let mut body_text_opt = None;

                for attempt in 1..=2 {
                    let resp = match self
                        .http
                        .get(&url)
                        .header("Authorization", format!("Token {}", token))
                        .header("Content-Type", "application/json")
                        .send()
                        .await
                    {
                        Ok(r) => r,
                        Err(e) => {
                            warn!("Lỗi kết nối endpoint {}: {}", url, e);
                            break;
                        }
                    };

                    if resp.status().as_u16() == 429 && attempt == 1 {
                        tokio::time::sleep(Duration::from_millis(1200)).await;
                        continue;
                    }

                    if !resp.status().is_success() {
                        break;
                    }

                    body_text_opt = resp.text().await.ok();
                    break;
                }

                let Some(body_text) = body_text_opt else {
                    break;
                };

                let Ok((page_offers, raw_count)) =
                    parse_offers_informations_page(&body_text, &today_ymd, 0)
                else {
                    break;
                };

                for offer in page_offers {
                    let key = (
                        offer.platform.to_lowercase(),
                        offer.coupon_code.to_uppercase(),
                        offer.shop_name.to_lowercase(),
                    );
                    if seen_keys.insert(key) {
                        collected.push(offer);
                    }
                }

                if raw_count < 50 || collected.len() >= 1500 {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(350)).await;
            }
            if collected.len() >= 1500 {
                break;
            }
        }

        // Nếu đang là quick sync 60s, gộp giữ lại các mã còn hạn đã thu được từ lần full scan gần nhất
        if !full_scan {
            let guard = self.voucher_cache.read().await;
            if let Some((cached_token, _, _, existing)) = guard.as_ref() {
                if cached_token == token {
                    for old in existing {
                        let exp = old.end_time.trim();
                        if exp.len() == 10 && exp < today_ymd.as_str() {
                            continue;
                        }
                        let key = (
                            old.platform.to_lowercase(),
                            old.coupon_code.to_uppercase(),
                            old.shop_name.to_lowercase(),
                        );
                        if seen_keys.insert(key) {
                            collected.push(old.clone());
                        }
                    }
                }
            }
        }

        // Sắp xếp ưu tiên: mã có % giảm sâu (>=20%) -> % giảm -> số tiền giảm VNĐ -> đơn tối thiểu thấp
        collected.sort_by(|a, b| {
            let a_tier = if a.discount_percent >= 20 { 2 } else if a.discount_percent > 0 { 1 } else { 0 };
            let b_tier = if b.discount_percent >= 20 { 2 } else if b.discount_percent > 0 { 1 } else { 0 };
            b_tier
                .cmp(&a_tier)
                .then_with(|| b.discount_percent.cmp(&a.discount_percent))
                .then_with(|| {
                    b.discount_amount
                        .partial_cmp(&a.discount_amount)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| {
                    a.min_spend
                        .partial_cmp(&b.min_spend)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.shop_name.cmp(&b.shop_name))
        });

        collected.truncate(1500);

        {
            let mut guard = self.voucher_cache.write().await;
            *guard = Some((token.to_string(), now, 0, collected.clone()));
        }

        let last_sync_vn = vn_now.format("%H:%M:%S %d/%m/%Y").to_string();
        {
            let mut meta = self.voucher_meta.write().await;
            meta.last_sync_vn = last_sync_vn;
            meta.sync_cycles += 1;
            meta.total_count = collected.len();
        }

        info!(
            "🎟️ Đã đồng bộ ({}) tổng cộng {} mã giảm giá toàn thị trường từ ACCESSTRADE API",
            if full_scan { "quét sâu" } else { "cập nhật 60s" },
            collected.len()
        );

        Ok(collected)
    }

    /// Lấy danh sách đơn hàng phát sinh gần đây từ ACCESSTRADE API (`/v1/orders`)
    pub async fn fetch_recent_orders(
        &self,
        token: &str,
        days_back: i64,
    ) -> Result<Vec<crate::db::models::TrackedOrder>, String> {
        let now = Utc::now();
        let since = now - chrono::Duration::days(days_back.clamp(1, 30));
        let since_iso: String = url::form_urlencoded::byte_serialize(
            since.format("%Y-%m-%dT%H:%M:%SZ").to_string().as_bytes(),
        )
        .collect();
        let until_iso: String = url::form_urlencoded::byte_serialize(
            now.format("%Y-%m-%dT%H:%M:%SZ").to_string().as_bytes(),
        )
        .collect();
        let orders_url = format!(
            "https://api.accesstrade.vn/v1/orders?since={}&until={}&limit=50",
            since_iso, until_iso
        );

        let resp = self
            .http
            .get(&orders_url)
            .header("Authorization", format!("Token {}", token.trim()))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| format!("Lỗi kết nối ACCESSTRADE Orders API: {}", e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| format!("Lỗi đọc phản hồi Orders API: {}", e))?;

        if !status.is_success() {
            return Err(format!("ACCESSTRADE Orders API trả về HTTP {}", status));
        }

        parse_accesstrade_orders_json(&body)
    }
}

/// Chuẩn hóa tên sàn từ merchant ACCESSTRADE
fn normalize_order_platform(raw_merchant: &str) -> String {
    let lower = raw_merchant.to_lowercase();
    if lower.contains("shopee") {
        "Shopee".to_string()
    } else if lower.contains("tiktok") {
        "TikTok Shop".to_string()
    } else if lower.contains("lazada") {
        "Lazada".to_string()
    } else if lower.contains("tiki") {
        "Tiki".to_string()
    } else if raw_merchant.trim().is_empty() {
        "ACCESSTRADE".to_string()
    } else {
        raw_merchant.trim().to_string()
    }
}

fn normalize_order_status_val(val: Option<&serde_json::Value>) -> String {
    match val {
        Some(serde_json::Value::Number(n)) => match n.as_i64() {
            Some(0) => "PENDING".to_string(),
            Some(1) => "APPROVED".to_string(),
            Some(2) => "REJECTED".to_string(),
            Some(other) => format!("STATUS_{}", other),
            None => "PENDING".to_string(),
        },
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            match trimmed.to_lowercase().as_str() {
                "0" | "pending" | "processing" => "PENDING".to_string(),
                "1" | "approved" | "completed" => "APPROVED".to_string(),
                "2" | "rejected" | "cancelled" | "canceled" => "REJECTED".to_string(),
                _ if trimmed.is_empty() => "PENDING".to_string(),
                _ => trimmed.to_uppercase(),
            }
        }
        _ => "PENDING".to_string(),
    }
}

/// Giải mã JSON đơn hàng trả về từ `/v1/orders` của ACCESSTRADE
pub fn parse_accesstrade_orders_json(
    body: &str,
) -> Result<Vec<crate::db::models::TrackedOrder>, String> {
    let root: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Lỗi parse JSON Orders API: {}", e))?;
    let Some(arr) = root.get("data").and_then(|d| d.as_array()) else {
        return Ok(Vec::new());
    };

    let now_rfc = Utc::now().to_rfc3339();
    let mut orders = Vec::new();
    for item in arr {
        let order_id = item
            .get("order_id")
            .or_else(|| item.get("id"))
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.trim().to_string())
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_default();
        if order_id.is_empty() {
            continue;
        }

        let merchant_raw = item
            .get("merchant")
            .or_else(|| item.get("campaign_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let platform = normalize_order_platform(merchant_raw);
        let status = normalize_order_status_val(
            item.get("status").or_else(|| item.get("order_status")),
        );
        let order_amount = parse_json_f64(
            item.get("billing")
                .or_else(|| item.get("total_amount"))
                .or_else(|| item.get("amount")),
        )
        .unwrap_or(0.0);
        let commission = parse_json_f64(
            item.get("pub_commission").or_else(|| item.get("commission")),
        )
        .unwrap_or(0.0);
        let sub_id = item
            .get("utm_sub1")
            .or_else(|| item.get("sub1"))
            .or_else(|| item.get("utm_campaign"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let order_time = item
            .get("sales_time")
            .or_else(|| item.get("click_time"))
            .or_else(|| item.get("confirmed_time"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        orders.push(crate::db::models::TrackedOrder {
            order_key: format!("AT:{}", order_id),
            platform,
            order_id,
            status,
            order_amount,
            commission,
            sub_id,
            order_time,
            notified_at: now_rfc.clone(),
        });
    }

    Ok(orders)
}

fn is_tracking_gateway_host(host: &str) -> bool {
    let lower = host.to_lowercase();
    lower == "shorten.asia" || lower == "go.isclix.com" || lower == "fast.accesstrade.com.vn"
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
    fn test_best_target_url_guards_broken_gateway() {
        let approved = ApprovedCampaigns {
            tiki: Some(("4348614231480407268".to_string(), "TIKI (CPS)".to_string())),
            tiktok: Some(("6648523843406889655".to_string(), "TIKTOK SHOP CPS".to_string())),
            ..Default::default()
        };

        let tiki_offer = VoucherOffer {
            platform: "Tiki".to_string(),
            shop_name: "Tiki Trading".to_string(),
            title: "Giam 45K".to_string(),
            coupon_code: "KANG45".to_string(),
            discount_percent: 0,
            discount_amount: 45000.0,
            discount_desc: "Giam 45K".to_string(),
            min_spend: 699000.0,
            raw_url: "https://tiki.vn/cua-hang/tiki-trading".to_string(),
            aff_url: Some("https://go.isclix.com/deep_link/123/4348614231480407268?url=abc".to_string()),
            end_time: "2026-12-31".to_string(),
        };
        assert_eq!(tiki_offer.best_target_url(Some(&approved)), "https://tiki.vn/cua-hang/tiki-trading");
        assert_eq!(tiki_offer.discount_badge(), "-45K");

        let tiktok_offer = VoucherOffer {
            platform: "TikTok Shop".to_string(),
            shop_name: "TikTok".to_string(),
            title: "Sale".to_string(),
            coupon_code: "TT35".to_string(),
            discount_percent: 35,
            discount_amount: 0.0,
            discount_desc: "Giam 35%".to_string(),
            min_spend: 0.0,
            raw_url: "https://shop.tiktok.com".to_string(),
            aff_url: Some("https://go.isclix.com/deep_link/123/6648523843406889655?url=tt".to_string()),
            end_time: "2026-12-31".to_string(),
        };
        assert_eq!(
            tiktok_offer.best_target_url(Some(&approved)),
            "https://go.isclix.com/deep_link/123/6648523843406889655?url=tt"
        );
        assert_eq!(tiktok_offer.discount_badge(), "-35%");
        assert!(is_tracking_gateway_host("go.isclix.com"));
        assert!(is_tracking_gateway_host("shorten.asia"));
        assert!(!is_tracking_gateway_host("click.accesstrade.vn"));
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

        // Khi min_discount_pct = 0: giữ lại toàn bộ mã còn hạn (kể cả mã 10% SMALL10)
        let (all_active, _) = parse_offers_informations_page(sample, "2026-11-16", 0).unwrap();
        assert_eq!(all_active.len(), 3);
    }

    #[test]
    fn test_match_vouchers_for_product_strictly_avoids_unrelated_shops() {
        let make_offer = |shop: &str, code: &str, pct: u32, amt: f64, min_sp: f64| VoucherOffer {
            platform: "Tiki".to_string(),
            shop_name: shop.to_string(),
            title: format!("Ưu đãi {}", shop),
            coupon_code: code.to_string(),
            discount_percent: pct,
            discount_amount: amt,
            discount_desc: format!("Giảm {} đơn từ {}", code, min_sp),
            min_spend: min_sp,
            raw_url: "https://tiki.vn/cua-hang/test".to_string(),
            aff_url: None,
            end_time: "2026-12-31".to_string(),
        };

        let offers = vec![
            make_offer("BG Việt Nam Official Store", "T102026", 18, 20000.0, 0.0),
            make_offer("DONA Thời Trang", "DONAFLR22", 11, 22000.0, 200000.0),
            make_offer("Tiki Trading", "KANG45NT1126", 0, 45000.0, 699000.0),
            make_offer("Toàn sàn Tiki", "TIKIALL50", 0, 50000.0, 500000.0),
        ];

        let matched = match_vouchers_for_product(
            &offers,
            "Tiki",
            "Sữa bột Enfagrow Enspire 3 850g",
            "https://tiki.vn/product-p274504811.html",
            Some("Tiki Trading"),
            Some("Enfa"),
            Some(822000.0),
            3,
        );

        let codes: Vec<&str> = matched.iter().map(|v| v.coupon_code.as_str()).collect();
        assert_eq!(codes, vec!["KANG45NT1126", "TIKIALL50"]);
        assert!(!codes.contains(&"T102026"));
        assert!(!codes.contains(&"DONAFLR22"));

        // Khi sản phẩm từ shop chưa có mã riêng và không có mã toàn sàn -> trả về rỗng thay vì nhét mã shop khác
        let only_other_shops = vec![
            make_offer("BG Việt Nam Official Store", "T102026", 18, 20000.0, 0.0),
            make_offer("DONA Thời Trang", "DONAFLR22", 11, 22000.0, 200000.0),
        ];
        let empty = match_vouchers_for_product(
            &only_other_shops,
            "Tiki",
            "Sữa bột Enfagrow Enspire 3 850g",
            "https://tiki.vn/product-p274504811.html",
            Some("Enfa Store"),
            Some("Enfa"),
            Some(822000.0),
            3,
        );
        assert!(empty.is_empty());
    }

    #[test]
    fn test_parse_accesstrade_orders_json() {
        let raw = r#"{
            "data": [
                {
                    "order_id": "TTS998877",
                    "merchant": "tiktokshop_vn",
                    "status": 0,
                    "billing": 250000,
                    "pub_commission": 18500,
                    "utm_sub1": "u123_r0_abcd",
                    "sales_time": "2026-11-18T12:00:00Z"
                },
                {
                    "order_id": "TK112233",
                    "merchant": "tikivn",
                    "status": "approved",
                    "amount": "500000",
                    "commission": "25000",
                    "sub1": "web01"
                }
            ]
        }"#;
        let orders = parse_accesstrade_orders_json(raw).unwrap();
        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].order_key, "AT:TTS998877");
        assert_eq!(orders[0].platform, "TikTok Shop");
        assert_eq!(orders[0].status, "PENDING");
        assert_eq!(orders[0].order_amount, 250000.0);
        assert_eq!(orders[0].commission, 18500.0);
        assert_eq!(orders[1].order_key, "AT:TK112233");
        assert_eq!(orders[1].platform, "Tiki");
        assert_eq!(orders[1].status, "APPROVED");
        assert_eq!(orders[1].commission, 25000.0);
    }
}

fn is_generic_platform_voucher_shop(shop_lower: &str, platform_lower: &str) -> bool {
    let trimmed = shop_lower.trim();
    if trimmed.is_empty() || trimmed == platform_lower {
        return true;
    }
    matches!(
        trimmed,
        "tiki"
            | "tikinow"
            | "toàn sàn"
            | "toàn sàn tiki"
            | "toàn sàn shopee"
            | "toàn sàn lazada"
            | "shopee"
            | "shopee mall"
            | "lazada"
            | "lazmall"
            | "tiktok"
            | "tiktok shop"
            | "khác"
    ) || trimmed.starts_with("toàn sàn ")
}

/// Lọc và xếp hạng mã khớp ĐÚNG shop / thương hiệu / toàn sàn của sản phẩm
pub fn match_vouchers_for_product(
    offers: &[VoucherOffer],
    platform: &str,
    product_title: &str,
    clean_url: &str,
    shop_name: Option<&str>,
    brand_name: Option<&str>,
    price_vnd: Option<f64>,
    max_items: usize,
) -> Vec<VoucherOffer> {
    let platform_lower = platform.trim().to_lowercase();
    let title_lower = product_title.trim().to_lowercase();
    let url_lower = clean_url.trim().to_lowercase();
    let prod_shop_lower = shop_name.unwrap_or("").trim().to_lowercase();
    let prod_brand_lower = brand_name.unwrap_or("").trim().to_lowercase();

    let mut ranked: Vec<(u8, bool, u32, i64, VoucherOffer)> = Vec::new();

    for v in offers {
        if !v.platform.eq_ignore_ascii_case(platform) {
            continue;
        }
        let v_shop_lower = v.shop_name.trim().to_lowercase();
        let v_title_lower = v.title.trim().to_lowercase();
        let is_generic = is_generic_platform_voucher_shop(&v_shop_lower, &platform_lower);

        let exact_shop_match = !is_generic
            && !prod_shop_lower.is_empty()
            && v_shop_lower.len() >= 3
            && (v_shop_lower == prod_shop_lower
                || prod_shop_lower.contains(&v_shop_lower)
                || v_shop_lower.contains(&prod_shop_lower));

        let brand_or_product_match = !is_generic
            && ((!prod_brand_lower.is_empty()
                && prod_brand_lower.len() >= 3
                && (v_shop_lower.contains(&prod_brand_lower)
                    || v_title_lower.contains(&prod_brand_lower)))
                || (v_shop_lower.len() >= 4
                    && (title_lower.contains(&v_shop_lower)
                        || url_lower.contains(&v_shop_lower.replace(' ', "")))));

        let tier = if exact_shop_match {
            3
        } else if brand_or_product_match {
            2
        } else if is_generic {
            1
        } else {
            0
        };

        if tier == 0 {
            continue;
        }

        let price_eligible = match price_vnd {
            Some(price) if price > 0.0 && v.min_spend > 0.0 => price >= v.min_spend,
            _ => true,
        };

        ranked.push((
            tier,
            price_eligible,
            v.discount_percent,
            v.discount_amount.round() as i64,
            v.clone(),
        ));
    }

    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| b.3.cmp(&a.3))
    });

    let mut seen_codes: HashSet<String> = HashSet::new();
    let mut results = Vec::new();
    for (_, _, _, _, offer) in ranked {
        let code_key = offer.coupon_code.trim().to_uppercase();
        if !code_key.is_empty() && !seen_codes.insert(code_key) {
            continue;
        }
        results.push(offer);
        if results.len() >= max_items {
            break;
        }
    }
    results
}

/// Phân tích 1 trang JSON từ ACCESSTRADE v1/offers_informations hoặc /coupon (khi min_discount_pct = 0 sẽ lấy 100% mã còn hạn)
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
        let raw_content = item.content.as_deref().unwrap_or("").trim();
        let (shop_name, clean_desc) = split_shop_and_desc(raw_name, &platform);
        let api_pct = parse_json_f64(item.discount_percentage.as_ref())
            .map(|v| v.round() as u32)
            .unwrap_or(0);
        let api_disc_val = parse_json_f64(item.discount_value.as_ref()).unwrap_or(0.0);
        let api_min_spend = parse_json_f64(item.min_spend.as_ref()).unwrap_or(0.0);

        let end_label = if end_date_prefix.is_empty() {
            "Đang hoạt động".to_string()
        } else {
            end_date_prefix.to_string()
        };

        let coupons = item.coupons.unwrap_or_default();
        if coupons.is_empty() {
            // Không bỏ qua ưu đãi trực tiếp không cần nhập mã nếu min_discount_pct == 0
            let pct = if (1..=100).contains(&api_pct) {
                api_pct
            } else {
                extract_discount_percent(raw_name)
                    .or_else(|| extract_discount_percent(raw_content))
                    .unwrap_or(0)
            };
            if pct >= min_discount_pct {
                let disc_amt = if api_disc_val > 0.0 {
                    api_disc_val
                } else {
                    extract_discount_amount(raw_name)
                        .or_else(|| extract_discount_amount(raw_content))
                        .unwrap_or(0.0)
                };
                let min_spend = if api_min_spend > 0.0 {
                    api_min_spend
                } else {
                    extract_min_spend(raw_name)
                        .or_else(|| extract_min_spend(raw_content))
                        .unwrap_or(0.0)
                };
                results.push(VoucherOffer {
                    platform,
                    shop_name,
                    title: clean_desc.to_string(),
                    coupon_code: "TỰ ĐỘNG ÁP".to_string(),
                    discount_percent: pct,
                    discount_amount: disc_amt,
                    discount_desc: if clean_desc.is_empty() {
                        raw_content.lines().next().unwrap_or("Ưu đãi trực tiếp").to_string()
                    } else {
                        clean_desc.to_string()
                    },
                    min_spend,
                    raw_url,
                    aff_url,
                    end_time: end_label,
                });
            }
            continue;
        }

        for c in coupons {
            let raw_code = c.coupon_code.unwrap_or_default();
            let code = raw_code.trim();
            let code_final = if code.is_empty() {
                "TỰ ĐỘNG ÁP".to_string()
            } else {
                code.to_string()
            };
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
                    .or_else(|| extract_discount_percent(raw_content))
                    .unwrap_or(0)
            };

            if pct < min_discount_pct {
                continue;
            }

            let disc_amt = if api_disc_val > 0.0 {
                api_disc_val
            } else {
                extract_discount_amount(desc_candidate)
                    .or_else(|| extract_discount_amount(raw_name))
                    .or_else(|| extract_discount_amount(raw_content))
                    .unwrap_or(0.0)
            };

            let min_spend = if api_min_spend > 0.0 {
                api_min_spend
            } else {
                extract_min_spend(raw_name)
                    .or_else(|| extract_min_spend(desc_candidate))
                    .or_else(|| extract_min_spend(raw_content))
                    .unwrap_or(0.0)
            };

            results.push(VoucherOffer {
                platform: platform.clone(),
                shop_name: shop_name.clone(),
                title: clean_desc.to_string(),
                coupon_code: code_final,
                discount_percent: pct,
                discount_amount: disc_amt,
                discount_desc: desc_candidate.to_string(),
                min_spend,
                raw_url: raw_url.clone(),
                aff_url: aff_url.clone(),
                end_time: end_label.clone(),
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

fn extract_discount_amount(text: &str) -> Option<f64> {
    static RE_AMT: std::sync::OnceLock<Option<Regex>> = std::sync::OnceLock::new();
    let re = RE_AMT
        .get_or_init(|| {
            Regex::new(r"(?i)(?:giảm|trợ\s*giá)\s*(?:trực\s*tiếp\s*)?([\d,\.]+)\s*(k\b|triệu|tr\b|vnđ|đ\b)").ok()
        })
        .as_ref()?;
    let caps = re.captures(text)?;
    parse_money_with_unit(
        caps.get(1)?.as_str(),
        caps.get(2).map(|m| m.as_str()).unwrap_or(""),
    )
}

fn extract_min_spend(text: &str) -> Option<f64> {
    static RE_MIN: std::sync::OnceLock<Option<Regex>> = std::sync::OnceLock::new();
    let re = RE_MIN
        .get_or_init(|| Regex::new(r"(?i)(?:tối\s*thiểu|đơn\s*từ|hàng\s*từ|từ)\s*([\d,\.]+)\s*(k\b|triệu|tr\b|vnđ|đ)?").ok())
        .as_ref()?;
    let caps = re.captures(text)?;
    parse_money_with_unit(
        caps.get(1)?.as_str(),
        caps.get(2).map(|m| m.as_str()).unwrap_or(""),
    )
}

fn parse_money_with_unit(num_str: &str, raw_unit: &str) -> Option<f64> {
    let unit = raw_unit.to_lowercase();
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
