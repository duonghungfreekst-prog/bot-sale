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
    pub fn best_target_url(&self, approved: Option<&ApprovedCampaigns>) -> String {
        let active_camp_id = match self.platform.as_str() {
            "Shopee" => approved.and_then(|a| a.shopee.as_ref()).map(|(id, _)| id.as_str()),
            "Lazada" => approved.and_then(|a| a.lazada.as_ref()).map(|(id, _)| id.as_str()),
            "Tiki" => approved.and_then(|a| a.tiki.as_ref()).map(|(id, _)| id.as_str()),
            "TikTok Shop" => approved.and_then(|a| a.tiktok.as_ref()).map(|(id, _)| id.as_str()),
            "Alibaba" => approved.and_then(|a| a.alibaba.as_ref()).map(|(id, _)| id.as_str()),
            _ => None,
        };
        if let (Some(cid), Some(aff)) = (active_camp_id, self.aff_url.as_deref()) {
            let aff_trimmed = aff.trim();
            // Chiến dịch TIKI (CPS) cũ (4348614231480407268 / v2/62) bị lỗi HTTP 500 trên go.isclix.com
            if cid != "4348614231480407268" && !aff_trimmed.is_empty() {
                if aff_trimmed.contains(cid) || !aff_trimmed.contains("go.isclix.com/deep_link/") {
                    return aff_trimmed.to_string();
                }
                // Tự động gắn campaign_id hoạt động (ví dụ TIKI CREATOR 6985318064449073386) vào aff_link của kho mã
                if let Some((before_q, after_q)) = aff_trimmed.split_once('?') {
                    let base = before_q
                        .trim_end_matches("/4348614231480407268")
                        .trim_end_matches('/');
                    return format!("{}/{}?{}", base, cid, after_q);
                }
            }
        }
        self.raw_url.clone()
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

    /// Kiểm tra mã có phải mã riêng của Shop / Thương hiệu (không phải mã chung toàn sàn)
    pub fn is_shop_specific(&self) -> bool {
        !is_generic_platform_voucher_shop(&self.shop_name.to_lowercase(), &self.platform.to_lowercase())
    }
}

fn sort_voucher_offers_priority(list: &mut Vec<VoucherOffer>) {
    list.sort_by(|a, b| {
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
    list.truncate(1500);
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
    pub domain: String,
}

#[derive(Clone, Debug, Default)]
pub struct ApprovedCampaigns {
    pub shopee: Option<(String, String)>,
    pub lazada: Option<(String, String)>,
    pub tiki: Option<(String, String)>,
    pub tiktok: Option<(String, String)>,
    pub alibaba: Option<(String, String)>,
    pub all_approved: Vec<ApprovedCampaignSummary>,
    pub fetched_at_epoch: i64,
}

impl ApprovedCampaigns {
    /// Trả về danh sách các chiến dịch sàn/thương hiệu mở rộng đã duyệt ngoài 5 sàn cốt lõi
    pub fn dynamic_campaigns(&self) -> Vec<&ApprovedCampaignSummary> {
        self.all_approved
            .iter()
            .filter(|c| {
                !c.domain.is_empty()
                    && !matches!(
                        c.platform_group.as_str(),
                        "Shopee" | "Lazada" | "Tiki" | "TikTok Shop" | "Alibaba" | "Khác"
                    )
            })
            .collect()
    }

    /// Tự động tìm chiến dịch ACCESSTRADE đã duyệt khớp đúng tên miền sản phẩm từ URL người dùng gửi
    pub fn find_dynamic_campaign_for_url(&self, raw_url: &str) -> Option<&ApprovedCampaignSummary> {
        let user_host = extract_normalized_host(raw_url)?;
        if is_core_marketplace_domain(&user_host) {
            return None;
        }
        let mut candidates: Vec<&ApprovedCampaignSummary> = self
            .dynamic_campaigns()
            .into_iter()
            .filter(|c| {
                user_host == c.domain || user_host.ends_with(&format!(".{}", c.domain))
            })
            .collect();
        candidates.sort_by(|a, b| {
            let a_exact = a.domain == user_host;
            let b_exact = b.domain == user_host;
            b_exact
                .cmp(&a_exact)
                .then_with(|| b.domain.len().cmp(&a.domain.len()))
        });
        candidates.into_iter().next()
    }
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
    url: Option<String>,
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

type CachedVoucherPool = (String, i64, u32, Vec<VoucherOffer>);

/// Truy vấn lọc mã giảm giá khớp đúng shop, thương hiệu, URL và khoảng giá sản phẩm.
pub struct VoucherMatchQuery<'a> {
    pub platform: &'a str,
    pub product_title: &'a str,
    pub clean_url: &'a str,
    pub shop_name: Option<&'a str>,
    pub brand_name: Option<&'a str>,
    pub price_vnd: Option<f64>,
    pub max_items: usize,
}

/// Client giao tiếp với ACCESSTRADE Publisher API v1/v2
pub struct AccessTradeClient {
    http: Client,
    redirect_probe: Client,
    cache: Arc<RwLock<Option<(String, ApprovedCampaigns)>>>,
    campaigns_fetch_lock: Arc<tokio::sync::Mutex<()>>,
    voucher_cache: Arc<RwLock<Option<CachedVoucherPool>>>,
    voucher_meta: Arc<RwLock<VoucherSyncMeta>>,
}

impl AccessTradeClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(12))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
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
            campaigns_fetch_lock: Arc::new(tokio::sync::Mutex::new(())),
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

    /// Nạp tức thì kho mã đã lưu từ DB SQLite khi mở App (không phải chờ 25-30s đầu quét API)
    pub async fn seed_voucher_cache_from_json(&self, api_token: &str, json_str: &str) -> usize {
        let token = clean_api_token(api_token);
        if token.is_empty() || json_str.trim().is_empty() {
            return 0;
        }
        let Ok(mut list) = serde_json::from_str::<Vec<VoucherOffer>>(json_str) else {
            return 0;
        };
        let vn_now = Utc::now() + chrono::Duration::hours(7);
        let today_ymd = vn_now.format("%Y-%m-%d").to_string();
        list.retain(|v| {
            let exp = v.end_time.trim();
            !(exp.len() == 10 && exp < today_ymd.as_str())
        });
        if list.is_empty() {
            return 0;
        }
        let now = Utc::now().timestamp();
        let count = list.len();
        {
            let mut guard = self.voucher_cache.write().await;
            if guard.as_ref().map(|(_, _, _, cur)| !cur.is_empty()).unwrap_or(false) {
                return count;
            }
            // Đặt fetched_at = now - 60 để lần tick đầu vẫn làm mới ngầm từ API ngay lập tức
            *guard = Some((token.to_string(), now - 60, 0, list));
        }
        {
            let mut meta = self.voucher_meta.write().await;
            if meta.last_sync_vn.is_empty() {
                meta.last_sync_vn = format!("{} (Cache DB)", vn_now.format("%H:%M:%S %d/%m/%Y"));
            }
            meta.total_count = count;
        }
        count
    }

    /// Xuất tối đa 700 mã đang cache thành JSON để lưu bền vững xuống DB
    pub async fn export_voucher_cache_json(&self) -> Option<String> {
        let guard = self.voucher_cache.read().await;
        let (_, _, _, list) = guard.as_ref()?;
        if list.is_empty() {
            return None;
        }
        let slice = &list[..list.len().min(700)];
        serde_json::to_string(slice).ok()
    }

    /// Lấy danh sách các chiến dịch đã được duyệt trên tài khoản ACCESSTRADE (có cache 3 phút & chống gọi trùng)
    pub async fn get_approved_campaigns(&self, api_token: &str, force_refresh: bool) -> Result<ApprovedCampaigns, String> {
        let token = clean_api_token(api_token);
        if token.is_empty() {
            return Err("Chưa cấu hình ACCESSTRADE API Token".to_string());
        }

        let now = Utc::now().timestamp();
        if !force_refresh {
            let guard = self.cache.read().await;
            if let Some((cached_token, cached)) = guard.as_ref()
                && cached_token == token && (now - cached.fetched_at_epoch) < CACHE_TTL_SECS {
                    return Ok(cached.clone());
                }
        }

        let _fetch_guard = self.campaigns_fetch_lock.lock().await;
        let now_locked = Utc::now().timestamp();
        if !force_refresh {
            let guard = self.cache.read().await;
            if let Some((cached_token, cached)) = guard.as_ref()
                && cached_token == token && (now_locked - cached.fetched_at_epoch) < CACHE_TTL_SECS {
                    return Ok(cached.clone());
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
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(|e| format!("Lỗi kết nối ACCESSTRADE API: {}", e))?;

            if !resp.status().is_success() {
                let status_code = resp.status();
                let err_text = resp.text().await.unwrap_or_default();
                let clean_msg = if err_text.contains("IP") || err_text.contains("whitelist") {
                    format!("HTTP {} (IP máy chủ chưa được duyệt trong mục IP Whitelist: {})", status_code, err_text.trim())
                } else if !err_text.trim().is_empty() && err_text.len() < 150 {
                    format!("HTTP {} ({})", status_code, err_text.trim())
                } else {
                    format!("HTTP {}", status_code)
                };
                return Err(format!("ACCESSTRADE trả về mã lỗi: {}", clean_msg));
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
                Platform::Alibaba => approved.alibaba,
                Platform::Dynamic { campaign_id, name } => Some((campaign_id.clone(), name.clone())),
                Platform::Unknown => approved
                    .find_dynamic_campaign_for_url(clean_url)
                    .map(|c| (c.id.clone(), c.platform_group.clone())),
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
            .header("Accept", "application/json")
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

        if let Some(data) = parsed.data
            && let Some(links) = data.success_link
                && let Some(first) = links.first() {
                    if let Some(short) = first.short_link.as_deref().filter(|s| !s.trim().is_empty()) {
                        return Ok(short.to_string());
                    }
                    if let Some(aff) = first.aff_link.as_deref().filter(|s| !s.trim().is_empty()) {
                        return Ok(aff.to_string());
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
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Lỗi gọi v2 tiktokshop: {}", e))?;

        let text = resp.text().await.map_err(|e| format!("Lỗi đọc body v2: {}", e))?;
        let parsed: TikTokFeedCreateResp =
            serde_json::from_str(&text).map_err(|e| format!("Lỗi parse JSON v2: {}", e))?;

        if parsed.status == Some(true)
            && let Some(data) = parsed.data {
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

        Err("Sản phẩm không thuộc danh sách TikTok Product Feed".to_string())
    }

    /// Xem nhanh toàn bộ kho voucher thị trường đang nằm sẵn trong RAM cache (không gọi mạng)
    /// Lấy danh sách các chiến dịch đã duyệt đang nằm trong RAM cache (không gọi mạng)
    pub async fn peek_approved_campaigns(&self) -> Option<ApprovedCampaigns> {
        self.cache.read().await.as_ref().map(|(_, c)| c.clone())
    }

    /// Lấy danh sách chiến dịch đã duyệt từ cache RAM; nếu cache trống và có API Token thì tự tải 1 lần
    pub async fn get_or_load_approved_campaigns(&self, api_token: Option<&str>) -> Option<ApprovedCampaigns> {
        if let Some(cached) = self.peek_approved_campaigns().await {
            return Some(cached);
        }
        let token = api_token.map(str::trim).filter(|s| !s.is_empty())?;
        self.get_approved_campaigns(token, false).await.ok()
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
        query: &VoucherMatchQuery<'_>,
    ) -> Vec<VoucherOffer> {
        let all = self.peek_cached_high_discount_vouchers().await;
        if all.is_empty() {
            return Vec::new();
        }
        match_vouchers_for_product(&all, query)
    }

    /// Trả về link đích đã qua kiểm tra gateway redirect an toàn cho một mã giảm giá (không bao giờ đưa link chết 404/500)
    pub async fn verified_offer_url(
        &self,
        offer: &VoucherOffer,
        approved: Option<&ApprovedCampaigns>,
    ) -> String {
        let candidate = offer.best_target_url(approved);
        if candidate != offer.raw_url && !self.verify_affiliate_redirect_ok(&candidate).await {
            return offer.raw_url.clone();
        }
        candidate
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
            if let Some((cached_token, fetched_at, cached_min, list)) = guard.as_ref()
                && cached_token == token
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
        let token = clean_api_token(api_token);
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
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=shopee.vn&limit=50", 3),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiki.vn&limit=50", 3),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=lazada.vn&limit=50", 2),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiktok.com&limit=50", 2),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&limit=50", 10),
            ]
        } else {
            &[
                ("https://api.accesstrade.vn/v1/offers_informations/coupon?limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&is_hot=1&limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=shopee.vn&limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiki.vn&limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=lazada.vn&limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiktok.com&limit=50", 1),
                ("https://api.accesstrade.vn/v1/offers_informations?status=1&limit=50", 2),
            ]
        };

        for (group_idx, &(base_endpoint, max_pages)) in endpoint_groups.iter().enumerate() {
            for page in 1..=max_pages {
                let url = format!("{}&page={}", base_endpoint, page);
                let mut body_text_opt = None;

                for attempt in 1..=2 {
                    let resp = match self
                        .http
                        .get(&url)
                        .header("Authorization", format!("Token {}", token))
                        .header("Accept", "application/json")
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

            // Đổ tạm ngay batch đầu tiên (~1.5s) nếu cache RAM còn rỗng để người dùng vừa mở app có mã dùng liền
            if group_idx == 0 && !collected.is_empty() {
                let cache_empty = {
                    let guard = self.voucher_cache.read().await;
                    guard.as_ref().map(|(_, _, _, l)| l.is_empty()).unwrap_or(true)
                };
                if cache_empty {
                    let mut early = collected.clone();
                    sort_voucher_offers_priority(&mut early);
                    let early_len = early.len();
                    {
                        let mut guard = self.voucher_cache.write().await;
                        *guard = Some((token.to_string(), now, 0, early));
                    }
                    let mut meta = self.voucher_meta.write().await;
                    meta.total_count = early_len;
                    meta.last_sync_vn = format!("{} (Đang quét tiếp...)", vn_now.format("%H:%M:%S %d/%m/%Y"));
                }
            }

            if collected.len() >= 1500 {
                break;
            }
        }

        // Nếu đang là quick sync 60s, gộp giữ lại các mã còn hạn đã thu được từ lần full scan gần nhất
        if !full_scan {
            let guard = self.voucher_cache.read().await;
            if let Some((cached_token, _, _, existing)) = guard.as_ref()
                && cached_token == token {
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

        sort_voucher_offers_priority(&mut collected);

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

    /// Lấy danh sách đơn hàng / giao dịch phát sinh gần đây từ ACCESSTRADE API (`/v1/transactions`, fallback `/v1/orders`)
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

        let endpoints = [
            format!(
                "https://api.accesstrade.vn/v1/transactions?since={}&until={}&limit=50",
                since_iso, until_iso
            ),
            format!(
                "https://api.accesstrade.vn/v1/orders?since={}&until={}&limit=50",
                since_iso, until_iso
            ),
        ];

        let mut last_status_err = String::new();
        for url in endpoints {
            let resp = self
                .http
                .get(&url)
                .header("Authorization", format!("Token {}", token.trim()))
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(|e| format!("Lỗi kết nối ACCESSTRADE Orders API: {}", e))?;

            let status = resp.status();
            let body = resp
                .text()
                .await
                .map_err(|e| format!("Lỗi đọc phản hồi Orders API: {}", e))?;

            if status.is_success() {
                return parse_accesstrade_orders_json(&body);
            }
            last_status_err = format!("ACCESSTRADE Orders API trả về HTTP {}", status);
            if status != reqwest::StatusCode::NOT_FOUND {
                return Err(last_status_err);
            }
        }

        Err(last_status_err)
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
    } else if lower.contains("alibaba") {
        "Alibaba".to_string()
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

/// Giải mã JSON đơn hàng trả về từ `/v1/transactions` hoặc `/v1/orders` của ACCESSTRADE
pub fn parse_accesstrade_orders_json(
    body: &str,
) -> Result<Vec<crate::db::models::TrackedOrder>, String> {
    let root: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Lỗi parse JSON Orders API: {}", e))?;
    let Some(arr) = root.get("data").and_then(|d| d.as_array()) else {
        return Ok(Vec::new());
    };

    let now_rfc = Utc::now().to_rfc3339();
    let mut orders: Vec<crate::db::models::TrackedOrder> = Vec::new();
    let mut index_by_key: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut has_non_bonus: Vec<bool> = Vec::new();

    for item in arr {
        let order_id = item
            .get("transaction_id")
            .or_else(|| item.get("order_id"))
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
        let is_bonus_row = item
            .get("is_brand_bonus")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || item
                .get("product_category")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case("bonus"))
                .unwrap_or(false);
        let order_amount = parse_json_f64(
            item.get("transaction_value")
                .or_else(|| item.get("billing"))
                .or_else(|| item.get("total_amount"))
                .or_else(|| item.get("amount"))
                .or_else(|| item.get("product_price")),
        )
        .unwrap_or(0.0);
        let commission = parse_json_f64(
            item.get("pub_commission").or_else(|| item.get("commission")),
        )
        .unwrap_or(0.0);
        let sub_id = item
            .get("utm_sub1")
            .or_else(|| item.get("sub1"))
            .or_else(|| {
                item.get("_extra")
                    .and_then(|ex| ex.get("sub_params"))
                    .and_then(|sp| sp.get("sub1"))
            })
            .or_else(|| item.get("utm_campaign"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let order_time = item
            .get("transaction_time")
            .or_else(|| item.get("sales_time"))
            .or_else(|| item.get("click_time"))
            .or_else(|| item.get("confirmed_time"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        let key = format!("AT:{}", order_id);
        if let Some(&idx) = index_by_key.get(&key) {
            let existing = &mut orders[idx];
            existing.commission += commission;
            if !is_bonus_row {
                if has_non_bonus[idx] {
                    existing.order_amount += order_amount;
                } else {
                    existing.order_amount = order_amount;
                    has_non_bonus[idx] = true;
                }
            }
            if existing.sub_id.is_empty() && !sub_id.is_empty() {
                existing.sub_id = sub_id;
            }
            if existing.order_time.is_empty() && !order_time.is_empty() {
                existing.order_time = order_time;
            }
            if status == "APPROVED" || (existing.status == "PENDING" && status != "PENDING") {
                existing.status = status;
            }
        } else {
            index_by_key.insert(key.clone(), orders.len());
            has_non_bonus.push(!is_bonus_row);
            orders.push(crate::db::models::TrackedOrder {
                order_key: key,
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
        let raw_url = item.url.unwrap_or_default();
        let m_lower = merchant.to_lowercase();
        let n_lower = name.to_lowercase();

        let (platform_group, domain) = if is_shopee_campaign(&m_lower, &n_lower) {
            if result.shopee.is_none() || m_lower == "shopee" || m_lower == "shopee_kolnew" {
                result.shopee = Some((id_str.clone(), name.clone()));
            }
            ("Shopee".to_string(), "shopee.vn".to_string())
        } else if is_lazada_campaign(&m_lower, &n_lower) {
            if result.lazada.is_none() || m_lower == "lazada_kol" || m_lower == "lazada_cashback" {
                result.lazada = Some((id_str.clone(), name.clone()));
            }
            ("Lazada".to_string(), "lazada.vn".to_string())
        } else if is_tiki_campaign(&m_lower, &n_lower) {
            // Ưu tiên TIKI CREATOR (6985318064449073386 - gateway v2/3744 hoạt động 100%) thay vì gói cũ tikivn (4348614231480407268 lỗi 500)
            let is_preferred = m_lower == "tiki_creator" || id_str == "6985318064449073386";
            let target_is_legacy = result
                .tiki
                .as_ref()
                .map(|(id, _)| id == "4348614231480407268")
                .unwrap_or(false);
            if result.tiki.is_none() || is_preferred || (target_is_legacy && id_str != "4348614231480407268") {
                result.tiki = Some((id_str.clone(), name.clone()));
            }
            ("Tiki".to_string(), "tiki.vn".to_string())
        } else if is_tiktok_campaign(&m_lower, &n_lower) {
            if result.tiktok.is_none() || m_lower == "tiktok_cps" {
                result.tiktok = Some((id_str.clone(), name.clone()));
            }
            ("TikTok Shop".to_string(), "tiktok.com".to_string())
        } else if is_alibaba_campaign(&m_lower, &n_lower) {
            if result.alibaba.is_none() || m_lower == "admitad_alibaba" {
                result.alibaba = Some((id_str.clone(), name.clone()));
            }
            ("Alibaba".to_string(), "alibaba.com".to_string())
        } else if let Some(dyn_domain) = resolve_dynamic_campaign_domain(&m_lower, &raw_url) {
            let label = format_dynamic_platform_label(&name, &dyn_domain);
            (label, dyn_domain)
        } else {
            ("Khác".to_string(), String::new())
        };

        result.all_approved.push(ApprovedCampaignSummary {
            id: id_str,
            merchant,
            name,
            platform_group,
            domain,
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
    let page_tiki_preferred = page
        .tiki
        .as_ref()
        .map(|(id, _)| id == "6985318064449073386")
        .unwrap_or(false);
    let target_tiki_legacy = target
        .tiki
        .as_ref()
        .map(|(id, _)| id == "4348614231480407268")
        .unwrap_or(false);
    if target.tiki.is_none() || (page.tiki.is_some() && (page_tiki_preferred || target_tiki_legacy)) {
        target.tiki = page.tiki.take();
    }
    if target.tiktok.is_none() {
        target.tiktok = page.tiktok.take();
    }
    if target.alibaba.is_none() {
        target.alibaba = page.alibaba.take();
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

fn is_alibaba_campaign(merchant: &str, name: &str) -> bool {
    matches!(merchant, "admitad_alibaba" | "alibaba" | "alibaba_cps")
        || (merchant.contains("alibaba") && name.contains("alibaba"))
}

/// Chuẩn hóa domain từ URL bất kỳ để khớp chính xác các chiến dịch mở rộng trên ACCESSTRADE
pub fn extract_normalized_host(raw_url: &str) -> Option<String> {
    let parsed = url::Url::parse(raw_url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let mut host = parsed.host_str()?.trim().to_lowercase();
    for prefix in [
        "www.", "m.", "mobile.", "vn.", "vi.", "us.", "uk.", "get.", "shop.", "store.",
    ] {
        if let Some(rest) = host.strip_prefix(prefix)
            && rest.contains('.')
                && !matches!(
                    rest,
                    "com.vn" | "net.vn" | "org.vn" | "edu.vn" | "gov.vn" | "co.uk" | "com.au"
                )
            {
                host = rest.to_string();
                break;
            }
    }
    if host.contains('.') {
        Some(host)
    } else {
        None
    }
}

fn is_core_marketplace_domain(host: &str) -> bool {
    let h = host.to_lowercase();
    [
        "shopee.vn",
        "shope.ee",
        "shp.ee",
        "lazada.vn",
        "lzd.co",
        "tiki.vn",
        "ti.ki",
        "tiktok.com",
        "alibaba.com",
    ]
    .iter()
    .any(|core| h == *core || h.ends_with(&format!(".{}", core)))
}

fn resolve_dynamic_campaign_domain(merchant_lower: &str, raw_url: &str) -> Option<String> {
    if merchant_lower.is_empty()
        || matches!(
            merchant_lower,
            "shopeepay_cpr"
                | "shopeefood_taixe"
                | "oneat_referral_firebase"
                | "tiktok_business"
                | "eco_comm"
        )
        || merchant_lower.contains("fpt_software")
        || merchant_lower.contains("fptsoftware")
        || merchant_lower.contains("cashloan")
        || merchant_lower.contains("_vay")
        || merchant_lower.contains("vaytinchap")
        || merchant_lower.contains("thetindung")
        || merchant_lower.contains("_credit")
        || merchant_lower.ends_with("_android")
        || merchant_lower.ends_with("_ios")
    {
        return None;
    }

    // Bản đồ tên miền thật cho vài thương hiệu có link tracker gián tiếp trên ACCESSTRADE
    let mapped = match merchant_lower {
        "adidas_vietnam" => Some("adidas.com.vn"),
        "aeonmall_eshop" => Some("aeoneshop.com"),
        "bitis.myharavan.com" => Some("bitis.com.vn"),
        _ => None,
    };
    if let Some(m) = mapped {
        return Some(m.to_string());
    }

    let host = extract_normalized_host(raw_url)?;
    if is_core_marketplace_domain(&host) {
        return None;
    }
    let blocked_tracker_hosts = [
        "accesstrade.vn",
        "caiapp.online",
        "pxf.io",
        "onelink.me",
        "page.link",
        "firebase.com",
        "play.google.com",
        "apps.apple.com",
        "itunes.apple.com",
        "shorten.asia",
        "go.isclix.com",
        "bit.ly",
        "tinyurl.com",
        "t.me",
        "facebook.com",
        "fb.com",
        "instagram.com",
        "zalo.me",
        "youtube.com",
        "google.com",
    ];
    if blocked_tracker_hosts
        .iter()
        .any(|b| host == *b || host.ends_with(&format!(".{}", b)))
    {
        return None;
    }
    Some(host)
}

fn format_dynamic_platform_label(name: &str, domain: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return domain.to_string();
    }
    if trimmed.chars().count() > 26 {
        if let Some((head, _)) = trimmed.split_once(" - ") {
            let h = head.trim();
            if !h.is_empty() {
                return h.to_string();
            }
        }
        if let Some((head, _)) = trimmed.split_once(" _ ") {
            let h = head.trim();
            if !h.is_empty() {
                return h.to_string();
            }
        }
    }
    trimmed.to_string()
}

/// Tự động làm sạch API Token: gọt bỏ khoảng trắng, dấu ngoặc kép, tiền tố 'Token ' hoặc 'Bearer ' nếu người dùng dán nhầm
pub fn clean_api_token(raw_token: &str) -> &str {
    let mut s = raw_token.trim().trim_matches('"').trim_matches('\'').trim();
    if let Some(rest) = s.strip_prefix("Token ").or_else(|| s.strip_prefix("token ")) {
        s = rest.trim();
    } else if let Some(rest) = s.strip_prefix("Bearer ").or_else(|| s.strip_prefix("bearer ")) {
        s = rest.trim();
    }
    s
}

/// Che giấu API Token khi hiển thị trên giao diện Web Admin (chỉ hiện 4 ký tự đầu + cuối)
pub fn mask_token(token: &str) -> String {
    let cleaned = clean_api_token(token);
    let chars: Vec<char> = cleaned.chars().collect();
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
                {"id": "6985318064449073386", "merchant": "tiki_creator", "name": "TIKI CREATOR", "approval": "successful", "status": 1},
                {"id": "4348614231480407268", "merchant": "tikivn", "name": "TIKI (CPS)", "approval": "successful", "status": 1},
                {"id": "6954336422318207062", "merchant": "admitad_alibaba", "name": "ALIBABA", "approval": "successful", "status": 1},
                {"id": 6648523843406889655, "merchant": "tiktok_cps", "name": "TIKTOK SHOP CPS", "approval": "successful", "status": 1},
                {"id": "4751584435713464237", "merchant": "shopee", "name": "Shopee Việt Nam", "approval": "unregistered", "status": 1},
                {"id": "7105125015509882615", "merchant": "romand_shopee", "name": "Romand Shopee PUB", "approval": "successful", "status": 1},
                {"id": "5087153089503673507", "merchant": "lazada_kol", "name": "Lazada Việt Nam", "approval": "successful", "status": 1}
            ]
        }"#;

        let (parsed, count) = parse_approved_campaigns_page(sample, 1000).unwrap();
        assert_eq!(count, 7);
        assert_eq!(parsed.tiki.as_ref().unwrap().0, "6985318064449073386"); // TIKI CREATOR must win over legacy tikivn
        assert_eq!(parsed.alibaba.as_ref().unwrap().0, "6954336422318207062");
        assert_eq!(parsed.tiktok.as_ref().unwrap().0, "6648523843406889655");
        assert!(parsed.shopee.is_none()); // unregistered & brand-only shopee must NOT match
        assert_eq!(parsed.lazada.as_ref().unwrap().0, "5087153089503673507");
        assert_eq!(parsed.all_approved.len(), 6);

        // Kiểm tra merge đa trang: trang 1 chỉ có gói cũ tikivn (4348614231480407268), trang 2 có TIKI CREATOR (6985318064449073386) -> phải nâng cấp lên 6985318064449073386
        let page1 = r#"{"data": [{"id": "4348614231480407268", "merchant": "tikivn", "name": "TIKI (CPS)", "approval": "successful", "status": 1, "url": "https://tiki.vn"}]}"#;
        let page2 = r#"{"data": [{"id": "6985318064449073386", "merchant": "tiki_creator", "name": "TIKI CREATOR", "approval": "successful", "status": 1, "url": "https://tiki.vn"}, {"id": "7073924423820975592", "merchant": "shein_mobikok", "name": "Shein", "approval": "successful", "status": 1, "url": "https://www.shein.com/"}, {"id": "6259155740535091857", "merchant": "cellphones_cps", "name": "CellphoneS", "approval": "successful", "status": 1, "url": "https://cellphones.com.vn/danh-sach-khuyen-mai"}, {"id": "7073874095910363548", "merchant": "shopeepay_cpr", "name": "ShopeePay CPR", "approval": "successful", "status": 1, "url": "https://motaikhoanspp.caiapp.online"}]}"#;
        let (mut combined, _) = parse_approved_campaigns_page(page1, 1000).unwrap();
        let (p2, _) = parse_approved_campaigns_page(page2, 1000).unwrap();
        merge_approved_campaigns(&mut combined, p2);
        assert_eq!(combined.tiki.as_ref().unwrap().0, "6985318064449073386");

        // Kiểm tra tự động nhận diện đa sàn mở rộng từ trường url & loại bỏ ví/app rác (shopeepay_cpr)
        assert_eq!(
            combined
                .find_dynamic_campaign_for_url("https://vn.shein.com/Dress-p-123.html")
                .map(|c| c.id.as_str()),
            Some("7073924423820975592")
        );
        assert_eq!(
            combined
                .find_dynamic_campaign_for_url("https://cellphones.com.vn/iphone-16.html")
                .map(|c| c.id.as_str()),
            Some("6259155740535091857")
        );
        assert!(combined
            .find_dynamic_campaign_for_url("https://motaikhoanspp.caiapp.online")
            .is_none());
        assert!(combined
            .find_dynamic_campaign_for_url("https://shopee.vn/product/1/2")
            .is_none());
    }

    #[test]
    fn test_mask_token() {
        assert_eq!(mask_token("1234567890abcdef"), "1234••••••••cdef");
        assert_eq!(mask_token("short"), "********");
    }

    #[test]
    fn test_clean_api_token() {
        assert_eq!(clean_api_token("Token my_secret_token_123"), "my_secret_token_123");
        assert_eq!(clean_api_token("Bearer my_bearer_token_456"), "my_bearer_token_456");
        assert_eq!(clean_api_token("\"raw_token_789\""), "raw_token_789");
        assert_eq!(clean_api_token("  my_token_000  "), "my_token_000");
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

        // Khi TIKI CREATOR (6985318064449073386) đã duyệt, tự động nâng cấp aff_link của kho mã sang campaign 6985318064449073386
        let approved_tiki_creator = ApprovedCampaigns {
            tiki: Some(("6985318064449073386".to_string(), "TIKI CREATOR".to_string())),
            ..Default::default()
        };
        let tiki_offer_no_cid = VoucherOffer {
            aff_url: Some("https://go.isclix.com/deep_link/7114511313911476062?url=https%3A%2F%2Ftiki.vn".to_string()),
            ..tiki_offer.clone()
        };
        assert_eq!(
            tiki_offer_no_cid.best_target_url(Some(&approved_tiki_creator)),
            "https://go.isclix.com/deep_link/7114511313911476062/6985318064449073386?url=https%3A%2F%2Ftiki.vn"
        );
        assert_eq!(
            tiki_offer.best_target_url(Some(&approved_tiki_creator)),
            "https://go.isclix.com/deep_link/123/6985318064449073386?url=abc"
        );

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
            &VoucherMatchQuery {
                platform: "Tiki",
                product_title: "Sữa bột Enfagrow Enspire 3 850g",
                clean_url: "https://tiki.vn/product-p274504811.html",
                shop_name: Some("Tiki Trading"),
                brand_name: Some("Enfa"),
                price_vnd: Some(822000.0),
                max_items: 3,
            },
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
            &VoucherMatchQuery {
                platform: "Tiki",
                product_title: "Sữa bột Enfagrow Enspire 3 850g",
                clean_url: "https://tiki.vn/product-p274504811.html",
                shop_name: Some("Enfa Store"),
                brand_name: Some("Enfa"),
                price_vnd: Some(822000.0),
                max_items: 3,
            },
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
                },
                {
                    "transaction_id": "587282004941178761",
                    "merchant": "tiktok_cps",
                    "status": 0,
                    "is_brand_bonus": true,
                    "product_category": "bonus",
                    "transaction_value": 1450.0,
                    "commission": 930.0,
                    "transaction_time": "2026-11-20T09:23:56",
                    "_extra": {
                        "sub_params": {
                            "sub1": "5342271625"
                        }
                    }
                },
                {
                    "transaction_id": "587282004941178761",
                    "merchant": "tiktok_cps",
                    "status": 0,
                    "is_brand_bonus": false,
                    "product_category": "285585744069",
                    "transaction_value": 29000.0,
                    "commission": 0.0,
                    "transaction_time": "2026-11-20T09:23:56"
                }
            ]
        }"#;
        let orders = parse_accesstrade_orders_json(raw).unwrap();
        assert_eq!(orders.len(), 3);
        assert_eq!(orders[0].order_key, "AT:TTS998877");
        assert_eq!(orders[0].platform, "TikTok Shop");
        assert_eq!(orders[0].status, "PENDING");
        assert_eq!(orders[0].order_amount, 250000.0);
        assert_eq!(orders[0].commission, 18500.0);
        assert_eq!(orders[1].order_key, "AT:TK112233");
        assert_eq!(orders[1].platform, "Tiki");
        assert_eq!(orders[1].status, "APPROVED");
        assert_eq!(orders[1].commission, 25000.0);
        assert_eq!(orders[2].order_key, "AT:587282004941178761");
        assert_eq!(orders[2].platform, "TikTok Shop");
        assert_eq!(orders[2].order_amount, 29000.0);
        assert_eq!(orders[2].commission, 930.0);
        assert_eq!(orders[2].sub_id, "5342271625");
        assert_eq!(orders[2].order_time, "2026-11-20T09:23:56");
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
            | "shopee việt nam"
            | "shopee mall"
            | "mã sàn shopee"
            | "voucher xtra"
            | "freeship xtra"
            | "shopee video"
            | "shopee live"
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
    query: &VoucherMatchQuery<'_>,
) -> Vec<VoucherOffer> {
    let platform_lower = query.platform.trim().to_lowercase();
    let title_lower = query.product_title.trim().to_lowercase();
    let url_lower = query.clean_url.trim().to_lowercase();
    let prod_shop_lower = query.shop_name.unwrap_or("").trim().to_lowercase();
    let prod_brand_lower = query.brand_name.unwrap_or("").trim().to_lowercase();

    let mut ranked: Vec<(u8, bool, u32, i64, VoucherOffer)> = Vec::new();

    for v in offers {
        if !v.platform.eq_ignore_ascii_case(query.platform) {
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

        let price_eligible = match query.price_vnd {
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
        if results.len() >= query.max_items {
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
    } else if joined.contains("alibaba") {
        "Alibaba".to_string()
    } else {
        "Khác".to_string()
    }
}

fn split_shop_and_desc<'a>(name: &'a str, fallback_platform: &str) -> (String, &'a str) {
    if let Some(rest) = name.strip_prefix('[')
        && let Some(end_idx) = rest.find(']') {
            let shop = rest[..end_idx].trim();
            let desc = rest[end_idx + 1..].trim().trim_start_matches('-').trim();
            if !shop.is_empty() {
                return (shop.to_string(), if desc.is_empty() { name } else { desc });
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
