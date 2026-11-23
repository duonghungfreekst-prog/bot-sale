use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use axum::{
    extract::{Form, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use chrono::Utc;
use rand::RngExt;
use subtle::ConstantTimeEq;
use teloxide::types::{Update, UpdateKind};
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::bot::handlers::{is_exclusive_deal_relevant, load_owned_aff_settings};
use crate::config::Config;
use crate::db::models::{ClickRecord, TrackedOrder};
use crate::db::repository::{NewDealInput, Repository};
use crate::db::AppDb;
use crate::services::accesstrade::{mask_token, AccessTradeClient, ApprovedCampaigns, VoucherOffer};
use crate::services::deal_parser::DealParser;
use crate::services::order_tracker;
use crate::services::shopee_affiliate::{
    extract_social_passthrough_params, is_valid_shopee_affiliate_id,
};
use crate::utils::{escape_html, format_vnd, sanitize_crlf};

pub type WebhookUpdateSender = mpsc::UnboundedSender<Result<Update, Infallible>>;

#[derive(Clone)]
pub struct AppState {
    pub db: AppDb,
    pub config: Config,
    pub accesstrade: Arc<AccessTradeClient>,
    pub parser: Arc<DealParser>,
    pub sessions: Arc<RwLock<HashSet<String>>>,
    pub login_limiter: Arc<RwLock<HashMap<String, (u32, i64)>>>,
    pub convert_limiter: Arc<RwLock<HashMap<String, (u32, i64)>>>,
    pub webhook_tx: Option<WebhookUpdateSender>,
}

/// Khởi chạy Web Admin Dashboard Server & Telegram Webhook Endpoint
pub async fn start_web_server(
    port: u16,
    db: AppDb,
    config: Config,
    accesstrade: Arc<AccessTradeClient>,
    webhook_tx: Option<WebhookUpdateSender>,
    ready_tx: Option<oneshot::Sender<()>>,
) -> anyhow::Result<()> {
    let parser = Arc::new(DealParser::with_accesstrade(accesstrade.clone()));
    let state = AppState {
        db,
        config,
        accesstrade,
        parser,
        sessions: Arc::new(RwLock::new(HashSet::new())),
        login_limiter: Arc::new(RwLock::new(HashMap::new())),
        convert_limiter: Arc::new(RwLock::new(HashMap::new())),
        webhook_tx,
    };

    let app = build_router(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    if let Some(tx) = ready_tx {
        let _ = tx.send(());
    }
    info!("🚀 Web Admin Dashboard Thế Giới Siêu Sale đang chạy tại: http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    Router::new()
        // Các route công khai
        .route("/", get(handle_root))
        .route("/convert", post(handle_public_convert).get(|| async { Redirect::to("/") }))
        .route("/assets/bridge.js", get(handle_bridge_js))
        .route("/assets/home.js", get(handle_home_js))
        .route("/health", get(handle_health))
        .route("/telegram/webhook", post(handle_telegram_webhook))
        .route("/admin/login", get(handle_login_page).post(handle_login_submit))
        .route("/admin/logout", get(handle_logout))
        // Các route quản trị (chỉ 1 tài khoản Admin duy nhất)
        .route("/admin", get(handle_dashboard))
        .route("/admin/settings/accesstrade", post(handle_update_accesstrade))
        .route("/admin/settings/accesstrade/sync", post(handle_sync_accesstrade))
        .route("/admin/settings/shopee", post(handle_update_shopee_settings))
        .route("/admin/settings/affiliate", post(handle_update_affiliate_settings))
        .route("/admin/settings/notifications", post(handle_update_notifications))
        .route("/admin/orders/sync", post(handle_sync_orders))
        .route("/admin/vouchers/sync", post(handle_sync_vouchers))
        .route("/admin/deals/add", post(handle_add_deal))
        .route("/admin/deals/import", post(handle_import_deals))
        .route("/admin/deals/toggle", post(handle_toggle_deal))
        .route("/admin/deals/delete", post(handle_delete_deal))
        // Giải mã link bọc 8 ký tự chuẩn DealX (đặt cuối cùng)
        .route("/go/{code}", get(handle_shortlink_redirect))
        .route("/{code}", get(handle_shortlink_redirect))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            shortlink_cloak_middleware,
        ))
        .layer(middleware::from_fn(security_headers_middleware))
        .with_state(state)
}

/// Xác thực chữ ký header Telegram Webhook theo thời gian hằng số và đẩy Update vào Dispatcher
async fn handle_telegram_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> StatusCode {
    let Some(tx) = state.webhook_tx.as_ref() else {
        return StatusCode::NOT_FOUND;
    };

    let provided_secret = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let expected_secret = state.config.telegram_webhook_secret.as_str();

    let is_valid: bool = provided_secret
        .as_bytes()
        .ct_eq(expected_secret.as_bytes())
        .into();
    if !is_valid {
        warn!("[WEB] Từ chối yêu cầu Telegram Webhook do sai secret token.");
        return StatusCode::UNAUTHORIZED;
    }

    match serde_json::from_str::<Update>(&body) {
        Ok(mut update) => {
            if let UpdateKind::Error(value) = &mut update.kind {
                *value = serde_json::from_str(&body).unwrap_or_default();
            }
            if let Err(err) = tx.send(Ok(update)) {
                error!("[WEB] Không thể gửi Update từ Webhook vào hàng đợi bot: {}", err);
                return StatusCode::SERVICE_UNAVAILABLE;
            }
        }
        Err(err) => {
            warn!("[WEB] Không thể giải mã JSON Telegram Webhook: {}", err);
        }
    }

    StatusCode::OK
}

/// Bổ sung các HTTP Security Headers bắt buộc cho mọi phản hồi Web Admin
async fn security_headers_middleware(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert(
        "strict-transport-security",
        HeaderValue::from_static("max-age=31536000"),
    );
    headers.insert("x-xss-protection", HeaderValue::from_static("1; mode=block"));
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'self'; style-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' data:"),
    );
    res
}

const CLOUDFLARE_WORKER_TEMPLATE: &str = include_str!("../../cloudflare_worker.js");

/// Phản hồi chuẩn 404 Not Found cho tên miền bọc link hoặc chế độ ẩn web chính
fn not_found_cloak_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        [
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (
                HeaderName::from_static("x-robots-tag"),
                HeaderValue::from_static("noindex, nofollow"),
            ),
        ],
        "Not Found",
    )
        .into_response()
}

/// Chuẩn hóa tên miền (host) từ URL hoặc Host header (lowercase, bỏ :port và www.)
fn extract_host_only(raw_host_or_url: &str) -> Option<String> {
    let trimmed = raw_host_or_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = if trimmed.contains("://") {
        url::Url::parse(trimmed).ok()?
    } else {
        url::Url::parse(&format!("http://{}", trimmed)).ok()?
    };
    let host = parsed.host_str()?.to_ascii_lowercase();
    let normalized = host.strip_prefix("www.").unwrap_or(&host).to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Kiểm tra path có thuộc nhóm duy nhất được phép mở qua tên miền shortlink bị che hay không
fn is_allowed_shortlink_cloak_path(method: &axum::http::Method, path: &str) -> bool {
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        return false;
    }
    if path == "/assets/bridge.js" {
        return true;
    }
    if matches!(
        path.to_ascii_lowercase().as_str(),
        "/health" | "/convert" | "/status" | "/metrics" | "/robots"
    ) {
        return false;
    }
    let code = path
        .strip_prefix("/go/")
        .or_else(|| path.strip_prefix('/'))
        .unwrap_or("");
    !code.contains('/') && Repository::is_valid_short_code(code)
}

/// Kiểm tra Admin có bật tùy chọn tạm ẩn Trang chủ `/` thành 404 Not Found hay không
async fn is_public_home_cloaked(state: &AppState) -> bool {
    let Ok(conn) = state.db.conn() else {
        return false;
    };
    Repository::get_setting(&conn, "public_home_cloak")
        .await
        .ok()
        .flatten()
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

/// Middleware che hoàn toàn Web Chính & `/admin` khi request đi qua Cloudflare Worker (`X-DealX-Cloak: 1`)
/// hoặc truy cập bằng tên miền bọc link riêng (`shortlink_base_url` khác host của Web Chính)
async fn shortlink_cloak_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();
    let has_cloak_header = headers
        .get("x-dealx-cloak")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim() == "1")
        .unwrap_or(false);

    let is_cloaked_domain = if has_cloak_header {
        true
    } else {
        let req_host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .and_then(extract_host_only);
        let public_host = state
            .config
            .public_base_url
            .as_deref()
            .and_then(extract_host_only);

        match req_host {
            Some(rh) if public_host.as_deref() != Some(rh.as_str()) => {
                let db_short_raw = if let Ok(conn) = state.db.conn() {
                    Repository::get_setting(&conn, "shortlink_base_url")
                        .await
                        .ok()
                        .flatten()
                        .filter(|s| !s.trim().is_empty())
                } else {
                    None
                };
                let short_host = db_short_raw
                    .as_deref()
                    .or(state.config.shortlink_base_url.as_deref())
                    .and_then(extract_host_only);
                match short_host {
                    Some(sh) => {
                        sh == rh
                            && public_host.as_deref() != Some(sh.as_str())
                            && !(public_host.is_none() && rh.ends_with(".onrender.com"))
                    }
                    None => false,
                }
            }
            _ => false,
        }
    };

    if is_cloaked_domain && !is_allowed_shortlink_cloak_path(req.method(), req.uri().path()) {
        return not_found_cloak_response();
    }

    next.run(req).await
}

#[derive(serde::Deserialize, Default)]
pub struct PublicHomeQuery {
    pub q: Option<String>,
    pub plat: Option<String>,
}

enum PublicDealCard<'a> {
    Db(&'a crate::db::models::DealItem),
    Api(&'a VoucherOffer),
}

#[derive(serde::Deserialize, Default)]
pub struct PublicConvertForm {
    pub url: String,
}

/// Trang chủ công khai tổng hợp mã giảm giá & gợi ý sản phẩm (phục vụ khách hàng & xét duyệt đối tác)
async fn handle_root(
    Query(query): Query<PublicHomeQuery>,
    State(state): State<AppState>,
) -> Response {
    if is_public_home_cloaked(&state).await {
        return not_found_cloak_response();
    }
    render_public_home(&state, query, "", "").await
}

/// Đổi link sản phẩm trực tiếp trên Trang chủ Web `/` (không cần Telegram)
async fn handle_public_convert(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<PublicConvertForm>,
) -> Response {
    if is_public_home_cloaked(&state).await {
        return not_found_cloak_response();
    }
    let input_raw = form.url.trim().to_string();
    let client_ip = extract_client_ip(&headers);

    // Chống spam endpoint /convert theo IP (tối đa 20 lượt / 60 giây)
    {
        let now_ts = Utc::now().timestamp();
        let mut limiter = state.convert_limiter.write().await;
        if limiter.len() >= 4000 {
            limiter.clear();
        }
        let entry = limiter.entry(client_ip).or_insert((0, now_ts));
        if now_ts - entry.1 >= 60 {
            *entry = (1, now_ts);
        } else if entry.0 >= 20 {
            let err_html = r#"<div class="convert-alert convert-alert--error" role="alert">⏳ Bạn đang gửi quá nhanh. Vui lòng chờ khoảng 1 phút rồi thử đổi link tiếp nhé!</div>"#;
            return render_public_home(&state, PublicHomeQuery::default(), &input_raw, err_html)
                .await;
        } else {
            entry.0 += 1;
        }
    }

    if state.parser.extract_first_url(&input_raw).is_none() {
        let err_html = r#"<div class="convert-alert convert-alert--error" role="alert">⚠️ Vui lòng dán đúng đường link sản phẩm bắt đầu bằng <code>https://</code> từ Shopee, Tiki, Lazada, TikTok Shop hoặc Alibaba.</div>"#;
        return render_public_home(&state, PublicHomeQuery::default(), &input_raw, err_html).await;
    }

    let Ok(conn) = state.db.conn() else {
        let err_html = r#"<div class="convert-alert convert-alert--error" role="alert">❌ Hệ thống cơ sở dữ liệu tạm bận, vui lòng thử lại sau ít giây.</div>"#;
        return render_public_home(&state, PublicHomeQuery::default(), &input_raw, err_html).await;
    };

    // Đảm bảo tài khoản khách Web (user_id = 0) tồn tại để thỏa khóa ngoại bảng clicks
    let _ = Repository::get_or_create_user(&conn, 0, Some("web_guest"), "Khách Web Thế Giới Siêu Sale", None)
        .await;
    let owned_aff = load_owned_aff_settings(&conn, &state.config).await;
    let _ = state
        .accesstrade
        .get_or_load_approved_campaigns(owned_aff.accesstrade_token.as_deref())
        .await;

    match state
        .parser
        .process_link(&input_raw, 0, None, owned_aff.as_borrowed())
        .await
    {
        Ok(parsed) => {
            let click_record = ClickRecord {
                id: parsed.click_id.clone(),
                user_id: 0,
                platform: parsed.platform.as_str().to_string(),
                original_url: parsed.original_url.clone(),
                clean_url: parsed.clean_url.clone(),
                affiliate_url: parsed.affiliate_url.clone(),
                sub_id: parsed.sub_id.clone(),
                created_at: Utc::now().to_rfc3339(),
            };
            let _ = Repository::record_click(&conn, &click_record).await;

            let wrapped_main = Repository::wrap_url_if_public(
                &conn,
                owned_aff.shortlink_base.as_deref(),
                &parsed.affiliate_url,
                &parsed.product_title,
                parsed.platform.as_str(),
            )
            .await;

            let mut meta_parts = Vec::new();
            if let Some(ref shop) = parsed.product_shop {
                meta_parts.push(format!("Shop: <strong>{}</strong>", escape_html(shop)));
            }
            if let Some(price) = parsed.product_price_vnd.filter(|&p| p > 0.0) {
                meta_parts.push(format!("Giá tham khảo: <strong>{}</strong>", escape_html(&format_vnd(price))));
            }
            let meta_line = if meta_parts.is_empty() {
                String::new()
            } else {
                format!(r#"<p class="convert-result__meta">{}</p>"#, meta_parts.join(" · "))
            };

            let mut matched_btns = String::new();
            if let Ok(deals) = Repository::get_active_deals(&conn, Some(parsed.platform.as_str())).await {
                for d in deals
                    .into_iter()
                    .filter(|d| is_exclusive_deal_relevant(d, &parsed))
                    .take(2)
                {
                    let wrapped_ex = Repository::wrap_url_if_public(
                        &conn,
                        owned_aff.shortlink_base.as_deref(),
                        &d.deal_url,
                        &d.title,
                        &d.platform,
                    )
                    .await;
                    let code_label = d
                        .coupon_code
                        .as_deref()
                        .filter(|c| !c.trim().is_empty())
                        .map(|c| format!(" · {}", c))
                        .unwrap_or_default();
                    matched_btns.push_str(&format!(
                        r#"<a class="btn btn--secondary" href="{}" target="_blank" rel="noopener noreferrer">👑 {}{} ↗</a>"#,
                        escape_html(&wrapped_ex),
                        escape_html(&d.title),
                        escape_html(&code_label)
                    ));
                }
            }

            let approved_cached = state.accesstrade.peek_approved_campaigns().await;
            let matched_market = state
                .accesstrade
                .search_matching_vouchers_for_deal(
                    parsed.platform.as_str(),
                    &parsed.product_title,
                    &parsed.clean_url,
                    parsed.product_shop.as_deref(),
                    parsed.product_brand.as_deref(),
                    parsed.product_price_vnd,
                    2,
                )
                .await;
            for mv in matched_market {
                let raw_v_url = state
                    .accesstrade
                    .verified_offer_url(&mv, approved_cached.as_ref())
                    .await;
                let wrapped_v = Repository::wrap_url_if_public(
                    &conn,
                    owned_aff.shortlink_base.as_deref(),
                    &raw_v_url,
                    &mv.shop_name,
                    &mv.platform,
                )
                .await;
                matched_btns.push_str(&format!(
                    r#"<a class="btn btn--secondary" href="{}" target="_blank" rel="noopener noreferrer">🎟️ Mã {} ({}) ↗</a>"#,
                    escape_html(&wrapped_v),
                    escape_html(&mv.coupon_code),
                    escape_html(&mv.discount_badge())
                ));
            }

            let result_html = format!(
                r#"<div class="convert-result" aria-live="polite">
                    <div class="convert-result__head">
                        <span class="badge badge--platform">{}</span>
                        <span class="badge badge--accent">Đã gắn link bọc ưu đãi</span>
                    </div>
                    <h2 class="convert-result__title">{}</h2>
                    {}
                    <div class="convert-result__url"><code>{}</code></div>
                    <div class="convert-actions">
                        <a class="btn btn--primary" href="{}" target="_blank" rel="noopener noreferrer">🚀 Mở Ứng Dụng &amp; Áp Mã Ngay ↗</a>
                        <button type="button" class="btn btn--secondary js-copy-link" data-copy-url="{}">📋 Sao chép link ưu đãi</button>
                        {}
                    </div>
                </div>"#,
                escape_html(parsed.platform.as_str()),
                escape_html(&parsed.product_title),
                meta_line,
                escape_html(&wrapped_main),
                escape_html(&wrapped_main),
                escape_html(&wrapped_main),
                matched_btns
            );

            render_public_home(&state, PublicHomeQuery::default(), &input_raw, &result_html).await
        }
        Err(err) => {
            let err_html = format!(
                r#"<div class="convert-alert convert-alert--error" role="alert">⚠️ Không thể phân tích link này: {}</div>"#,
                escape_html(&err.to_string())
            );
            render_public_home(&state, PublicHomeQuery::default(), &input_raw, &err_html).await
        }
    }
}

/// JS ngoài cho Trang Chủ & nút sao chép trong Web Admin (tuân thủ nghiêm ngặt CSP `default-src 'self'`)
async fn handle_home_js() -> Response {
    let js = r#"document.addEventListener("click", function (e) {
    var btn = e.target.closest(".js-copy-link");
    if (!btn) return;
    var url = btn.getAttribute("data-copy-url");
    var fromTarget = false;
    if (!url) {
        var targetId = btn.getAttribute("data-copy-target");
        if (targetId) {
            var el = document.getElementById(targetId);
            if (el) {
                url = typeof el.value === "string" ? el.value : el.textContent;
                fromTarget = true;
            }
        }
    }
    if (!url) return;
    if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(url).then(function () {
            var old = btn.textContent;
            btn.textContent = fromTarget ? "✅ Đã sao chép code" : "✅ Đã sao chép link";
            setTimeout(function () { btn.textContent = old; }, 2000);
        });
    }
});"#;
    let mut resp = (StatusCode::OK, js).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    resp
}

async fn render_public_home(
    state: &AppState,
    query: PublicHomeQuery,
    convert_input_val: &str,
    convert_result_html: &str,
) -> Response {
    let search_q = query.q.unwrap_or_default().trim().to_string();
    let filter_plat = query
        .plat
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("all")
        .to_string();

    let conn_opt = state.db.conn().ok();
    let db_deals = match conn_opt.as_ref() {
        Some(conn) => Repository::get_active_deals(conn, None).await.unwrap_or_default(),
        None => Vec::new(),
    };
    let shortlink_base = match conn_opt.as_ref() {
        Some(conn) => Repository::get_setting(conn, "shortlink_base_url")
            .await
            .ok()
            .flatten()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| s.starts_with("https://") || s.starts_with("http://"))
            .or_else(|| state.config.shortlink_base_url.clone())
            .or_else(|| state.config.public_base_url.clone()),
        None => state
            .config
            .shortlink_base_url
            .clone()
            .or_else(|| state.config.public_base_url.clone()),
    };
    let market_vouchers = state.accesstrade.peek_cached_high_discount_vouchers().await;
    let approved = state.accesstrade.peek_approved_campaigns().await;
    let sync_meta = state.accesstrade.get_voucher_sync_meta().await;

    let q_lower = search_q.to_lowercase();
    let mut cards: Vec<PublicDealCard<'_>> = Vec::new();

    for d in &db_deals {
        let plat_ok = filter_plat == "all" || d.platform.eq_ignore_ascii_case(&filter_plat);
        let q_ok = q_lower.is_empty()
            || d.title.to_lowercase().contains(&q_lower)
            || d.discount_desc.to_lowercase().contains(&q_lower)
            || d.coupon_code.as_deref().unwrap_or("").to_lowercase().contains(&q_lower)
            || d.platform.to_lowercase().contains(&q_lower);
        if plat_ok && q_ok {
            cards.push(PublicDealCard::Db(d));
        }
    }

    for v in &market_vouchers {
        let plat_ok = filter_plat == "all" || v.platform.eq_ignore_ascii_case(&filter_plat);
        let q_ok = q_lower.is_empty()
            || v.shop_name.to_lowercase().contains(&q_lower)
            || v.title.to_lowercase().contains(&q_lower)
            || v.coupon_code.to_lowercase().contains(&q_lower)
            || v.discount_desc.to_lowercase().contains(&q_lower)
            || v.platform.to_lowercase().contains(&q_lower);
        if plat_ok && q_ok {
            cards.push(PublicDealCard::Api(v));
        }
    }

    let total_available = db_deals.len() + market_vouchers.len();
    let matched_count = cards.len();
    let last_sync_text = if sync_meta.last_sync_vn.is_empty() {
        "Đang đồng bộ thời gian thực".to_string()
    } else {
        sync_meta.last_sync_vn
    };

    let mut cards_html = String::new();
    for card in cards.into_iter().take(24) {
        match card {
            PublicDealCard::Db(d) => {
                let code_txt = d
                    .coupon_code
                    .as_deref()
                    .filter(|c| !c.trim().is_empty())
                    .unwrap_or("Tự động áp dụng");
                let badge = if d.is_exclusive {
                    "Ưu đãi nổi bật"
                } else {
                    "Mã giảm giá"
                };
                let min_txt = if d.min_spend > 0.0 {
                    format!("Đơn từ {}", format_vnd(d.min_spend))
                } else {
                    "Không giới hạn đơn tối thiểu".to_string()
                };
                let wrapped_url = match conn_opt.as_ref() {
                    Some(conn) => {
                        Repository::wrap_url_if_public(
                            conn,
                            shortlink_base.as_deref(),
                            &d.deal_url,
                            &d.title,
                            &d.platform,
                        )
                        .await
                    }
                    None => d.deal_url.clone(),
                };
                cards_html.push_str(&format!(
                    r#"<article class="deal-card">
                        <div class="deal-card__top">
                            <span class="badge badge--platform">{}</span>
                            <span class="badge badge--accent">{}</span>
                        </div>
                        <h3 class="deal-card__title">{}</h3>
                        <p class="deal-card__desc">{}</p>
                        <div class="deal-card__meta">
                            <span class="coupon-pill">Mã: <code>{}</code></span>
                            <span class="deal-card__note">{}</span>
                        </div>
                        <a class="btn btn--primary deal-card__cta" href="{}" target="_blank" rel="noopener noreferrer">Lấy mã &amp; Xem ưu đãi ↗</a>
                    </article>"#,
                    escape_html(&d.platform),
                    badge,
                    escape_html(&d.title),
                    escape_html(&d.discount_desc),
                    escape_html(code_txt),
                    escape_html(&min_txt),
                    escape_html(&wrapped_url)
                ));
            }
            PublicDealCard::Api(v) => {
                let target_url = v.best_target_url(approved.as_ref());
                let wrapped_url = match conn_opt.as_ref() {
                    Some(conn) => {
                        Repository::wrap_url_if_public(
                            conn,
                            shortlink_base.as_deref(),
                            &target_url,
                            &v.shop_name,
                            &v.platform,
                        )
                        .await
                    }
                    None => target_url,
                };
                let badge = v.discount_badge();
                let expiry = if v.end_time.trim().is_empty() {
                    "Đang hiệu lực".to_string()
                } else {
                    format!("HSD: {}", v.end_time.trim())
                };
                cards_html.push_str(&format!(
                    r#"<article class="deal-card">
                        <div class="deal-card__top">
                            <span class="badge badge--platform">{}</span>
                            <span class="badge badge--accent">{}</span>
                        </div>
                        <h3 class="deal-card__title">{}</h3>
                        <p class="deal-card__desc">{}</p>
                        <div class="deal-card__meta">
                            <span class="coupon-pill">Mã: <code>{}</code></span>
                            <span class="deal-card__note">{}</span>
                        </div>
                        <a class="btn btn--primary deal-card__cta" href="{}" target="_blank" rel="noopener noreferrer">Lấy mã &amp; Mở gian hàng ↗</a>
                    </article>"#,
                    escape_html(&v.platform),
                    escape_html(&badge),
                    escape_html(&v.shop_name),
                    escape_html(&v.discount_desc),
                    escape_html(&v.coupon_code),
                    escape_html(&expiry),
                    escape_html(&wrapped_url)
                ));
            }
        }
    }

    if cards_html.is_empty() {
        let empty_msg = if total_available == 0 {
            "Hệ thống đang đồng bộ danh sách ưu đãi mới nhất từ đối tác. Bạn có thể quay lại sau ít phút hoặc gửi trực tiếp link sản phẩm qua trợ lý Telegram để kiểm tra nhanh."
        } else {
            "Không tìm thấy ưu đãi nào khớp với từ khóa hoặc sàn bạn vừa chọn. Hãy thử xóa bộ lọc để xem toàn bộ ưu đãi đang hoạt động."
        };
        cards_html = format!(
            r#"<div class="empty-state">
                <p>{}</p>
                <a class="btn btn--secondary" href="/">Xem toàn bộ ưu đãi</a>
            </div>"#,
            empty_msg
        );
    }

    let filter_links = [
        ("all", "Tất cả sàn"),
        ("Shopee", "Shopee"),
        ("Tiki", "Tiki"),
        ("Lazada", "Lazada"),
        ("TikTok Shop", "TikTok Shop"),
        ("Alibaba", "Alibaba"),
    ]
    .into_iter()
    .map(|(val, label)| {
        let active_cls = if filter_plat.eq_ignore_ascii_case(val) {
            "filter-pill filter-pill--active"
        } else {
            "filter-pill"
        };
        let href = if search_q.is_empty() {
            format!("/?plat={}", urlencoding_simple(val))
        } else {
            format!(
                "/?q={}&plat={}",
                urlencoding_simple(&search_q),
                urlencoding_simple(val)
            )
        };
        format!(r#"<a class="{}" href="{}">{}</a>"#, active_cls, href, label)
    })
    .collect::<Vec<_>>()
    .join("");

    let html = format!(
        r##"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Thế Giới Siêu Sale — Tổng Hợp Mã Giảm Giá &amp; Gợi Ý Sản Phẩm Chính Hãng</title>
    <meta name="description" content="Cổng tổng hợp mã giảm giá công khai theo từng ngành hàng từ Shopee, Tiki, Lazada, TikTok Shop giúp người mua tiết kiệm mỗi ngày.">
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600;700&amp;family=Lexend:wght@600;700;800&amp;display=swap">
    <style>
        :root {{
            --color-bg: #0c0f17;
            --color-surface: #141926;
            --color-surface-elevated: #1c2333;
            --color-text: #f8fafc;
            --color-text-muted: #cbd5e1;
            --color-primary: #f97316;
            --color-primary-hover: #ea580c;
            --color-primary-contrast: #0b0f19;
            --color-border: #263044;
            --color-success: #34d399;
            --radius-sm: 8px;
            --radius-md: 12px;
            --radius-lg: 18px;
            --shadow-md: 0 10px 30px rgba(0, 0, 0, 0.35);
            --font-sans: "Be Vietnam Pro", system-ui, -apple-system, sans-serif;
            --font-display: "Lexend", "Be Vietnam Pro", system-ui, sans-serif;
        }}
        * {{ box-sizing: border-box; }}
        html {{ scroll-behavior: smooth; }}
        body {{
            margin: 0;
            background: radial-gradient(circle at top right, rgba(249, 115, 22, 0.12), transparent 42%), var(--color-bg);
            color: var(--color-text);
            font-family: var(--font-sans);
            font-size: 16px;
            line-height: 1.6;
        }}
        a, button, input {{ font: inherit; }}
        a:focus-visible, button:focus-visible, input:focus-visible {{
            outline: 2px solid var(--color-primary);
            outline-offset: 2px;
        }}
        .container {{ max-width: 1180px; margin: 0 auto; padding: 0 16px; }}
        .site-header {{
            position: sticky; top: 0; z-index: 20;
            background: rgba(12, 15, 23, 0.92);
            backdrop-filter: blur(10px);
            border-bottom: 1px solid var(--color-border);
        }}
        .site-header__inner {{
            display: flex; align-items: center; justify-content: space-between;
            gap: 12px; min-height: 68px; flex-wrap: wrap; padding: 10px 0;
        }}
        .brand {{
            display: inline-flex; align-items: center; gap: 10px;
            text-decoration: none; color: var(--color-text);
            font-family: var(--font-display); font-weight: 800; font-size: 1.15rem;
        }}
        .brand__mark {{
            width: 38px; height: 38px; border-radius: var(--radius-md);
            display: inline-flex; align-items: center; justify-content: center;
            background: var(--color-primary); color: var(--color-primary-contrast); font-weight: 800;
        }}
        .nav {{ display: flex; align-items: center; gap: 16px; flex-wrap: wrap; }}
        .nav__link {{
            color: var(--color-text-muted); text-decoration: none; font-size: 0.95rem; font-weight: 500;
            padding: 8px 4px; min-height: 44px; display: inline-flex; align-items: center;
        }}
        .nav__link:hover {{ color: var(--color-text); }}
        .btn {{
            display: inline-flex; align-items: center; justify-content: center; gap: 8px;
            min-height: 44px; padding: 10px 18px; border-radius: var(--radius-md);
            border: 1px solid transparent; font-weight: 700; text-decoration: none; cursor: pointer;
            transition: background-color 180ms ease, transform 180ms ease;
        }}
        .btn--primary {{
            background: var(--color-primary); color: var(--color-primary-contrast);
        }}
        .btn--primary:hover {{ background: var(--color-primary-hover); transform: translateY(-1px); }}
        .btn--secondary {{
            background: var(--color-surface-elevated); color: var(--color-text);
            border-color: var(--color-border);
        }}
        .btn--secondary:hover {{ border-color: var(--color-primary); }}
        .hero {{ padding: 40px 0 28px; }}
        .hero__eyebrow {{
            display: inline-flex; align-items: center; gap: 8px;
            padding: 6px 12px; border-radius: 999px;
            background: var(--color-surface); border: 1px solid var(--color-border);
            color: var(--color-success); font-size: 0.85rem; font-weight: 600;
        }}
        .hero__title {{
            font-family: var(--font-display);
            font-size: clamp(1.75rem, 4vw, 2.75rem);
            line-height: 1.2; margin: 16px 0 12px; max-width: 780px;
        }}
        .hero__subtitle {{
            color: var(--color-text-muted); max-width: 68ch; margin: 0 0 24px;
        }}
        .stats-bar {{
            display: flex; flex-wrap: wrap; gap: 16px; margin-bottom: 24px;
            color: var(--color-text-muted); font-size: 0.9rem;
        }}
        .stats-bar strong {{ color: var(--color-text); }}
        .search-panel {{
            background: var(--color-surface); border: 1px solid var(--color-border);
            border-radius: var(--radius-lg); padding: 16px; box-shadow: var(--shadow-md);
        }}
        .search-form {{ display: flex; flex-wrap: wrap; gap: 12px; align-items: stretch; }}
        .sr-only {{
            position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
            overflow: hidden; clip: rect(0, 0, 0, 0); border: 0;
        }}
        .search-input {{
            flex: 1 1 240px; min-height: 44px; padding: 10px 14px;
            border-radius: var(--radius-md); border: 1px solid var(--color-border);
            background: var(--color-bg); color: var(--color-text);
        }}
        .search-input::placeholder {{ color: #94a3b8; }}
        .filter-row {{ display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }}
        .filter-pill {{
            min-height: 40px; padding: 6px 14px; border-radius: 999px;
            border: 1px solid var(--color-border); background: var(--color-bg);
            color: var(--color-text-muted); text-decoration: none; font-size: 0.9rem; font-weight: 600;
            display: inline-flex; align-items: center;
        }}
        .filter-pill--active {{
            background: var(--color-primary); color: var(--color-primary-contrast); border-color: var(--color-primary);
        }}
        .section {{ padding: 28px 0; }}
        .section__head {{
            display: flex; flex-wrap: wrap; justify-content: space-between; align-items: baseline;
            gap: 12px; margin-bottom: 18px;
        }}
        .section__title {{ font-family: var(--font-display); font-size: 1.45rem; margin: 0; }}
        .deals-grid {{
            display: grid; gap: 16px;
            grid-template-columns: 1fr;
        }}
        @media (min-width: 720px) {{ .deals-grid {{ grid-template-columns: repeat(2, minmax(0, 1fr)); }} }}
        @media (min-width: 1060px) {{ .deals-grid {{ grid-template-columns: repeat(3, minmax(0, 1fr)); }} }}
        .deal-card {{
            display: flex; flex-direction: column; justify-content: space-between; gap: 12px;
            background: var(--color-surface); border: 1px solid var(--color-border);
            border-radius: var(--radius-lg); padding: 18px;
        }}
        .deal-card__top {{ display: flex; justify-content: space-between; align-items: center; gap: 8px; }}
        .badge {{
            font-size: 0.78rem; font-weight: 700; padding: 4px 10px; border-radius: 999px;
        }}
        .badge--platform {{ background: var(--color-surface-elevated); color: var(--color-text); border: 1px solid var(--color-border); }}
        .badge--accent {{ background: rgba(249, 115, 22, 0.18); color: #fdba74; border: 1px solid rgba(249, 115, 22, 0.4); }}
        .deal-card__title {{ font-family: var(--font-display); font-size: 1.05rem; margin: 0; color: var(--color-text); }}
        .deal-card__desc {{ margin: 0; color: var(--color-text-muted); font-size: 0.92rem; }}
        .deal-card__meta {{ display: flex; flex-direction: column; gap: 4px; font-size: 0.85rem; }}
        .coupon-pill code {{
            background: var(--color-bg); border: 1px dashed var(--color-primary);
            padding: 2px 8px; border-radius: 6px; color: #fdba74; font-weight: 700;
        }}
        .deal-card__note {{ color: #94a3b8; }}
        .deal-card__cta {{ width: 100%; }}
        .steps-grid {{
            display: grid; gap: 16px; grid-template-columns: 1fr;
        }}
        @media (min-width: 768px) {{ .steps-grid {{ grid-template-columns: repeat(3, minmax(0, 1fr)); }} }}
        .step-card {{
            background: var(--color-surface); border: 1px solid var(--color-border);
            border-radius: var(--radius-lg); padding: 18px;
        }}
        .step-card h3 {{ margin: 0 0 8px; font-family: var(--font-display); font-size: 1.05rem; }}
        .step-card p {{ margin: 0; color: var(--color-text-muted); font-size: 0.92rem; }}
        .empty-state {{
            grid-column: 1 / -1; background: var(--color-surface); border: 1px solid var(--color-border);
            border-radius: var(--radius-lg); padding: 28px; text-align: center; color: var(--color-text-muted);
        }}
        .site-footer {{
            border-top: 1px solid var(--color-border); margin-top: 32px; padding: 28px 0 40px;
            color: var(--color-text-muted); font-size: 0.88rem;
        }}
        .site-footer__links {{ display: flex; flex-wrap: wrap; gap: 16px; margin-top: 10px; }}
        .site-footer a {{ color: #fdba74; }}
        .convert-panel {{
            background: linear-gradient(135deg, rgba(249, 115, 22, 0.12), rgba(20, 25, 38, 0.96));
            border: 1px solid rgba(249, 115, 22, 0.45);
            border-radius: var(--radius-lg); padding: 18px; box-shadow: var(--shadow-md);
            margin-bottom: 16px;
        }}
        .convert-panel__title {{
            font-family: var(--font-display); font-size: 1.05rem; font-weight: 700;
            margin: 0 0 10px; color: var(--color-text);
        }}
        .convert-result {{
            margin-top: 14px; background: var(--color-bg); border: 1px solid var(--color-border);
            border-radius: var(--radius-md); padding: 14px;
        }}
        .convert-result__head {{ display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 8px; }}
        .convert-result__title {{ font-family: var(--font-display); font-size: 1rem; margin: 0 0 6px; }}
        .convert-result__meta {{ margin: 0 0 8px; color: var(--color-text-muted); font-size: 0.9rem; }}
        .convert-result__url {{
            background: var(--color-surface); border: 1px dashed var(--color-border);
            padding: 8px 10px; border-radius: var(--radius-sm); word-break: break-all;
            font-size: 0.85rem; margin-bottom: 12px; color: #fdba74;
        }}
        .convert-actions {{ display: flex; flex-wrap: wrap; gap: 10px; }}
        .convert-alert {{
            margin-top: 12px; padding: 12px 14px; border-radius: var(--radius-md); font-size: 0.92rem;
        }}
        .convert-alert--error {{
            background: rgba(239, 68, 68, 0.12); border: 1px solid rgba(239, 68, 68, 0.45); color: #fecaca;
        }}
        @media (prefers-reduced-motion: reduce) {{
            *, *::before, *::after {{ transition: none !important; scroll-behavior: auto !important; }}
        }}
    </style>
</head>
<body>
    <header class="site-header">
        <div class="container site-header__inner">
            <a class="brand" href="/">
                <span class="brand__mark" aria-hidden="true">TG</span>
                <span>Thế Giới Siêu Sale</span>
            </a>
            <nav class="nav" aria-label="Điều hướng chính">
                <a class="nav__link" href="#uu-dai">Ưu đãi nổi bật</a>
                <a class="nav__link" href="#huong-dan">Cách nhận mã</a>
                <a class="nav__link" href="#chinh-sach">Minh bạch đối tác</a>
                <a class="btn btn--primary" href="https://t.me/{}" target="_blank" rel="noopener noreferrer">Mở Trợ Lý @{}</a>
            </nav>
        </div>
    </header>

    <main class="container">
        <section class="hero">
            <span class="hero__eyebrow">Cập nhật tự động từ đối tác chính hãng</span>
            <h1 class="hero__title">Tổng hợp mã giảm giá &amp; gợi ý sản phẩm tiết kiệm mỗi ngày</h1>
            <p class="hero__subtitle">
                Thế Giới Siêu Sale tổng hợp các chương trình ưu đãi, mã giảm giá công khai theo từng gian hàng và ngành hàng từ Shopee, Tiki, Lazada, TikTok Shop và Alibaba giúp người mua dễ dàng so sánh và đặt hàng trực tiếp trên sàn.
            </p>
            <div class="stats-bar">
                <span>Đang hiển thị: <strong>{}</strong> / <strong>{}</strong> ưu đãi hoạt động</span>
                <span>Lần đồng bộ gần nhất: <strong>{}</strong></span>
            </div>

            <div class="convert-panel">
                <p class="convert-panel__title">⚡ Dán link sản phẩm để lấy link ưu đãi &amp; mã khớp ngay (không cần cài App)</p>
                <form class="search-form" method="POST" action="/convert">
                    <label class="sr-only" for="web-convert-url">Dán đường link sản phẩm cần săn mã</label>
                    <input id="web-convert-url" class="search-input" type="url" name="url" required value="{}" placeholder="Dán link sản phẩm Shopee, Tiki, Lazada, TikTok Shop hoặc Alibaba vào đây...">
                    <button class="btn btn--primary" type="submit">⚡ Lấy Link Ưu Đãi</button>
                </form>
                {}
            </div>

            <div class="search-panel">
                <form class="search-form" method="GET" action="/">
                    <label class="sr-only" for="deal-search">Tìm mã giảm giá hoặc tên gian hàng</label>
                    <input id="deal-search" class="search-input" type="text" name="q" value="{}" placeholder="Tìm tên gian hàng, ngành hàng hoặc mã ưu đãi (VD: Tiki Trading, Gia dụng...)">
                    <input type="hidden" name="plat" value="{}">
                    <button class="btn btn--primary" type="submit">Tìm ưu đãi</button>
                </form>
                <div class="filter-row" aria-label="Lọc theo sàn thương mại điện tử">
                    {}
                </div>
            </div>
        </section>

        <section id="uu-dai" class="section">
            <div class="section__head">
                <h2 class="section__title">Danh sách ưu đãi &amp; mã giảm giá mới nhất</h2>
                <span class="deal-card__note">Chuyển tiếp trực tiếp sang trang chính thức của sàn</span>
            </div>
            <div class="deals-grid">
                {}
            </div>
        </section>

        <section id="huong-dan" class="section">
            <div class="section__head">
                <h2 class="section__title">Quy trình sử dụng nhanh chóng &amp; hợp lệ</h2>
            </div>
            <div class="steps-grid">
                <article class="step-card">
                    <h3>1. Chọn ưu đãi hoặc gửi link sản phẩm</h3>
                    <p>Tìm mã theo ngành hàng ngay trên website hoặc dán liên kết sản phẩm trực tiếp cho trợ lý Telegram @{}.</p>
                </article>
                <article class="step-card">
                    <h3>2. Lưu mã tại trang gian hàng chính thức</h3>
                    <p>Bấm nút mở gian hàng để truy cập trang khuyến mãi hợp lệ trên Shopee, Tiki, Lazada hoặc TikTok Shop.</p>
                </article>
                <article class="step-card">
                    <h3>3. Thanh toán trực tiếp trên sàn</h3>
                    <p>Người dùng chủ động chọn mã ở bước thanh toán trên ứng dụng thương mại điện tử để hưởng mức giá tốt nhất.</p>
                </article>
            </div>
        </section>
    </main>

    <footer id="chinh-sach" class="site-footer">
        <div class="container">
            <p><strong>Tuyên bố minh bạch tiếp thị liên kết (Affiliate Disclosure):</strong> Thế Giới Siêu Sale là website tổng hợp thông tin khuyến mãi độc lập, hoạt động theo quy chuẩn đối tác tiếp thị liên kết chính thức. Khi người dùng bấm vào liên kết ưu đãi và hoàn tất mua sắm tự nhiên trên sàn thương mại điện tử, nền tảng có thể nhận hoa hồng giới thiệu mà không làm tăng giá sản phẩm của người mua.</p>
            <div class="site-footer__links">
                <span>Thế Giới Siêu Sale © 2026</span>
                <a href="https://t.me/{}" target="_blank" rel="noopener noreferrer">Kênh hỗ trợ Telegram</a>
            </div>
        </div>
    </footer>
    <script src="/assets/home.js" defer></script>
</body>
</html>"##,
        escape_html(&state.config.bot_username),
        escape_html(&state.config.bot_username),
        matched_count,
        total_available,
        escape_html(&last_sync_text),
        escape_html(convert_input_val),
        convert_result_html,
        escape_html(&search_q),
        escape_html(&filter_plat),
        filter_links,
        cards_html,
        escape_html(&state.config.bot_username),
        escape_html(&state.config.bot_username)
    );

    Html(html).into_response()
}

fn urlencoding_simple(data: &str) -> String {
    url::form_urlencoded::byte_serialize(data.as_bytes()).collect()
}

fn is_preview_bot(headers: &HeaderMap) -> bool {
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    ua.contains("telegrambot")
        || ua.contains("twitterbot")
        || ua.contains("facebookexternalhit")
        || ua.contains("whatsapp")
        || ua.contains("discordbot")
        || ua.contains("slackbot")
        || ua.contains("zalo")
}

fn is_desktop_browser(headers: &HeaderMap) -> bool {
    let sec_mobile = headers
        .get("sec-ch-ua-mobile")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if sec_mobile == "?0" {
        return true;
    }

    let sec_platform = headers
        .get("sec-ch-ua-platform")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if sec_platform.contains("windows")
        || sec_platform.contains("macos")
        || sec_platform.contains("linux")
        || sec_platform.contains("chrome os")
    {
        return true;
    }

    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let is_mobile_ua = ua.contains("android")
        || ua.contains("iphone")
        || ua.contains("ipad")
        || ua.contains("ipod")
        || ua.contains("mobile");
    !is_mobile_ua
        && (ua.contains("windows nt") || ua.contains("macintosh") || ua.contains("x11; linux"))
}

/// Mã hóa XOR payload link đích bằng khóa ngẫu nhiên 8 byte để không lộ URL thô trong mã nguồn HTML
fn encode_bridge_payload(url: &str) -> (String, String) {
    let mut rng = rand::rng();
    let mut key = [0u8; 8];
    rng.fill(&mut key);

    let key_hex: String = key.iter().map(|b| format!("{:02x}", b)).collect();
    let mut payload_hex = String::with_capacity(url.len() * 2);
    for (i, byte) in url.as_bytes().iter().enumerate() {
        payload_hex.push_str(&format!("{:02x}", byte ^ key[i % key.len()]));
    }
    (payload_hex, key_hex)
}

#[allow(dead_code)]
fn decode_bridge_payload(payload_hex: &str, key_hex: &str) -> Option<String> {
    if payload_hex.is_empty()
        || key_hex.is_empty()
        || !payload_hex.len().is_multiple_of(2)
        || !key_hex.len().is_multiple_of(2)
    {
        return None;
    }
    let key: Vec<u8> = (0..key_hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&key_hex[i..i + 2], 16).ok())
        .collect();
    if key.len() * 2 != key_hex.len() || key.is_empty() {
        return None;
    }
    let bytes: Vec<u8> = (0..payload_hex.len())
        .step_by(2)
        .enumerate()
        .filter_map(|(idx, i)| {
            u8::from_str_radix(&payload_hex[i..i + 2], 16)
                .ok()
                .map(|b| b ^ key[idx % key.len()])
        })
        .collect();
    if bytes.len() * 2 != payload_hex.len() {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// JS ngoài cho Cầu nối Mobile (lớp 2 chống giả lập PC & chống kẹt trình duyệt nhúng Zalo/Messenger)
async fn handle_bridge_js() -> Response {
    let js = r#"function isRealMobile() {
    if (navigator.webdriver) return false;
    var uad = navigator.userAgentData;
    if (uad) {
        var uadPlat = (uad.platform || "").toLowerCase();
        if (uad.mobile === false) return false;
        if (/windows|macos|linux|chrome os/.test(uadPlat)) return false;
    }
    var plat = (navigator.platform || "").toLowerCase();
    if (/^win|linux x86_64|linux i686/.test(plat)) return false;
    if (plat === "macintel" && (navigator.maxTouchPoints || 0) <= 1) return false;
    return true;
}

function decodeXorHex(pHex, kHex) {
    if (!pHex || !kHex || pHex.length % 2 !== 0 || kHex.length % 2 !== 0) return "";
    var key = new Uint8Array(kHex.length / 2);
    var out = new Uint8Array(pHex.length / 2);
    for (var i = 0; i < key.length; i++) key[i] = parseInt(kHex.substr(i * 2, 2), 16);
    for (var j = 0; j < out.length; j++) {
        out[j] = parseInt(pHex.substr(j * 2, 2), 16) ^ key[j % key.length];
    }
    return new TextDecoder("utf-8").decode(out);
}

(function () {
    var body = document.body;
    if (!body) return;
    var mobileOnly = body.getAttribute("data-mobile-only") === "1";
    if (mobileOnly && !isRealMobile()) {
        var titleEl = document.getElementById("bridge-title");
        var descEl = document.getElementById("bridge-desc");
        var spinner = document.getElementById("bridge-spinner");
        if (spinner) spinner.style.display = "none";
        if (titleEl) titleEl.textContent = "📱 Link ưu đãi chỉ mở được trên Điện Thoại";
        if (descEl) descEl.textContent = "Phát hiện trình duyệt máy tính đang bật giả lập. Vui lòng mở trực tiếp trên điện thoại iOS / Android!";
        return;
    }

    var targetUrl = decodeXorHex(body.getAttribute("data-p") || "", body.getAttribute("data-k") || "");
    if (!targetUrl || (!targetUrl.startsWith("https://") && !targetUrl.startsWith("http://"))) return;

    var rescueBtn = document.getElementById("rescue-open-btn");
    var copyBtn = document.getElementById("rescue-copy-btn");
    var rescueBox = document.getElementById("rescue-box");
    var webviewTip = document.getElementById("webview-tip");

    if (rescueBtn) rescueBtn.setAttribute("href", targetUrl);
    if (copyBtn) {
        copyBtn.addEventListener("click", function () {
            if (navigator.clipboard && navigator.clipboard.writeText) {
                navigator.clipboard.writeText(targetUrl).then(function () {
                    copyBtn.textContent = "✅ Đã sao chép link mở App";
                });
            }
        });
    }

    var ua = navigator.userAgent || "";
    var isEmbeddedWebView = /zalo|fban|fbav|fb_iab|messenger|instagram/i.test(ua);
    if (isEmbeddedWebView && webviewTip) {
        webviewTip.style.display = "block";
    }

    setTimeout(function () {
        if (rescueBox) rescueBox.classList.remove("hidden");
    }, isEmbeddedWebView ? 250 : 850);

    try {
        window.location.replace(targetUrl);
    } catch (_) {}
})();"#;
    let mut resp = (StatusCode::OK, js).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    resp
}

/// Giải mã link bọc `/{code}` & `/go/{code}`: bảo vệ 2 lớp chống soi link trên PC & giữ referrer chuẩn sang App sàn
async fn handle_shortlink_redirect(
    Path(code): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    let code_trimmed = code.trim();
    if !Repository::is_valid_short_code(code_trimmed) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let Ok(conn) = state.db.conn() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let Ok(Some(record)) = Repository::resolve_short_link(&conn, code_trimmed).await else {
        return StatusCode::NOT_FOUND.into_response();
    };

    if is_preview_bot(&headers) {
        let plat_desc = if record.platform.trim().is_empty() {
            "ứng dụng mua sắm".to_string()
        } else {
            record.platform.trim().to_string()
        };
        let title_display = if record.title.trim().is_empty() {
            format!("Ưu đãi {} hôm nay", plat_desc)
        } else {
            record.title.clone()
        };
        let preview_html = format!(
            r#"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="utf-8">
    <meta name="robots" content="noindex, nofollow">
    <meta property="og:title" content="{}">
    <meta property="og:description" content="Bấm để mở ưu đãi trên {}">
    <title>{}</title>
</head>
<body><p>Đang mở liên kết ưu đãi.</p></body>
</html>"#,
            escape_html(&title_display),
            escape_html(&plat_desc),
            escape_html(&title_display)
        );
        return (StatusCode::OK, Html(preview_html)).into_response();
    }

    let mobile_only = Repository::get_setting(&conn, "shortlink_mobile_only")
        .await
        .ok()
        .flatten()
        .map(|v| v.trim() != "0")
        .unwrap_or(true);

    if mobile_only && is_desktop_browser(&headers) {
        let blocked_html = r#"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="robots" content="noindex, nofollow">
    <title>Chỉ hỗ trợ mở trên Điện Thoại</title>
</head>
<body style="margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:#0c0f17;color:#f8fafc;font-family:system-ui,sans-serif;padding:24px;text-align:center;">
    <div style="max-width:420px;background:#141926;border:1px solid #263044;border-radius:16px;padding:24px;">
        <h1 style="margin:0 0 12px;font-size:18px;">📱 Link ưu đãi chỉ mở được trên Điện Thoại</h1>
        <p style="margin:0;color:#cbd5e1;font-size:14px;line-height:1.6;">Link ưu đãi này chỉ mở được trên Điện Thoại (iOS / Android) để tự động chuyển vào App sàn &amp; kích hoạt ưu đãi. Vui lòng bấm mở từ điện thoại của bạn!</p>
    </div>
</body>
</html>"#;
        let mut resp = (StatusCode::FORBIDDEN, Html(blocked_html)).into_response();
        resp.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        resp.headers_mut().insert(
            HeaderName::from_static("x-robots-tag"),
            HeaderValue::from_static("noindex, nofollow"),
        );
        return resp;
    }

    let _ = Repository::increment_short_link_click(&conn, code_trimmed).await;

    // Khi Admin tắt chặn máy tính (mobile_only = false), chuyển hướng 302 trực tiếp để test nhanh trên PC
    if !mobile_only {
        let mut response = StatusCode::FOUND.into_response();
        if let Ok(loc_val) = HeaderValue::from_str(&record.target_url) {
            response.headers_mut().insert(header::LOCATION, loc_val);
        } else {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        response.headers_mut().insert(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("unsafe-url"),
        );
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, must-revalidate, private"),
        );
        response.headers_mut().insert(
            HeaderName::from_static("x-robots-tag"),
            HeaderValue::from_static("noindex, nofollow"),
        );
        return response;
    }

    // Khi bật chặn máy tính: trả cầu nối Mobile thông minh lớp 2 (XOR payload + chống kẹt Zalo/Messenger)
    let (payload_hex, key_hex) = encode_bridge_payload(&record.target_url);
    let plat_label = if record.platform.trim().is_empty() {
        "ứng dụng mua sắm".to_string()
    } else {
        record.platform.trim().to_string()
    };
    let bridge_html = format!(
        r##"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="robots" content="noindex, nofollow">
    <meta name="referrer" content="unsafe-url">
    <title>Đang chuyển sang {}...</title>
    <style>
        * {{ box-sizing: border-box; }}
        body {{
            margin: 0; min-height: 100vh; display: flex; align-items: center; justify-content: center;
            background: #0c0f17; color: #f8fafc; font-family: system-ui, -apple-system, sans-serif; padding: 20px;
        }}
        .card {{
            width: 100%; max-width: 420px; background: #141926; border: 1px solid #263044;
            border-radius: 18px; padding: 24px; text-align: center;
        }}
        .spinner {{
            width: 34px; height: 34px; margin: 0 auto 14px; border: 3px solid #263044;
            border-top-color: #f97316; border-radius: 50%; animation: spin 0.8s linear infinite;
        }}
        @keyframes spin {{ to {{ transform: rotate(360deg); }} }}
        h1 {{ margin: 0 0 10px; font-size: 18px; line-height: 1.35; }}
        p {{ margin: 0; color: #cbd5e1; font-size: 14px; line-height: 1.6; }}
        .hidden {{ display: none; }}
        .rescue {{ margin-top: 18px; display: flex; flex-direction: column; gap: 10px; }}
        .btn-open {{
            display: flex; align-items: center; justify-content: center; min-height: 48px;
            background: #f97316; color: #0b0f19; font-weight: 700; text-decoration: none;
            border-radius: 12px; padding: 12px 16px;
        }}
        .btn-copy {{
            min-height: 44px; background: #1c2333; color: #f8fafc; border: 1px solid #263044;
            border-radius: 12px; font-weight: 600; padding: 10px 14px; cursor: pointer;
        }}
        .tip {{
            display: none; font-size: 12px; color: #fdba74; background: rgba(249, 115, 22, 0.12);
            border: 1px solid rgba(249, 115, 22, 0.35); border-radius: 10px; padding: 10px; text-align: left;
        }}
    </style>
</head>
<body data-p="{}" data-k="{}" data-mobile-only="1">
    <main class="card">
        <div id="bridge-spinner" class="spinner" aria-hidden="true"></div>
        <h1 id="bridge-title">🚀 Đang mở {}...</h1>
        <p id="bridge-desc">Hệ thống đang chuyển bạn sang ứng dụng để kích hoạt mã giảm giá.</p>
        <div id="rescue-box" class="rescue hidden">
            <a id="rescue-open-btn" class="btn-open" href="#" rel="noopener">🛍️ Bấm Để Mở Ứng Dụng Ngay</a>
            <button id="rescue-copy-btn" class="btn-copy" type="button">📋 Sao chép link mở bằng Safari/Chrome</button>
            <div id="webview-tip" class="tip">
                💡 <b>Mẹo nếu mở từ Zalo / Messenger:</b> Bấm dấu <b>⋮</b> (hoặc <b>⋯</b>) ở góc màn hình rồi chọn <b>“Mở bằng trình duyệt mặc định”</b> để tự động bật App sàn.
            </div>
        </div>
    </main>
    <script src="/assets/bridge.js" defer></script>
</body>
</html>"##,
        escape_html(&plat_label),
        escape_html(&payload_hex),
        escape_html(&key_hex),
        escape_html(&plat_label)
    );

    let mut resp = (StatusCode::OK, Html(bridge_html)).into_response();
    resp.headers_mut().insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("unsafe-url"),
    );
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate, private"),
    );
    resp.headers_mut().insert(
        HeaderName::from_static("x-robots-tag"),
        HeaderValue::from_static("noindex, nofollow"),
    );
    resp
}

/// Health check cho Render Web Service
async fn handle_health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "The Gioi Sieu Sale Bot & Web Admin Healthy")
}

/// Trích xuất session token từ HTTP Cookie header
fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    let cookie_str = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_str.split(';') {
        let part = part.trim();
        if let Some(token) = part.strip_prefix("dealx_session=") {
            return Some(token.to_string());
        }
    }
    None
}

/// Lấy địa chỉ IP gốc của client qua Cloudflare/Render proxy headers
fn extract_client_ip(headers: &HeaderMap) -> String {
    if let Some(ip) = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return ip.to_string();
    }
    if let Some(xff) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return xff.to_string();
    }
    "local".to_string()
}

/// Kiểm tra xem yêu cầu hiện tại có quyền Admin hay không (từ RAM cache hoặc DB băm SHA-256)
async fn is_authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(token) = extract_session_token(headers) else {
        return false;
    };
    {
        let sessions = state.sessions.read().await;
        if sessions.contains(&token) {
            return true;
        }
    }
    if let Ok(conn) = state.db.conn() {
        if let Ok(true) = Repository::verify_admin_session(&conn, &token).await {
            let mut sessions = state.sessions.write().await;
            if sessions.len() >= 100 {
                sessions.clear();
            }
            sessions.insert(token);
            return true;
        }
    }
    false
}

/// Form đăng nhập admin
#[derive(serde::Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

/// Giao diện trang Đăng Nhập Admin (Chỉ duy nhất 1 tài khoản, không cho phép đăng ký)
async fn handle_login_page(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if is_authenticated(&state, &headers).await {
        return Redirect::to("/admin").into_response();
    }

    let html = r#"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Đăng Nhập Quản Trị - Thế Giới Siêu Sale Admin</title>
    <link href="https://cdn.jsdelivr.net/npm/tailwindcss@2.2.19/dist/tailwind.min.css" rel="stylesheet">
    <style>
        body { background: #0b0f19; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }
    </style>
</head>
<body class="flex items-center justify-center min-h-screen text-gray-100 p-4">
    <div class="w-full max-w-md bg-gray-900 border border-gray-800 rounded-2xl shadow-2xl p-8 backdrop-blur-lg">
        <div class="text-center mb-8">
            <div class="inline-flex items-center justify-center w-16 h-16 rounded-2xl bg-gradient-to-tr from-yellow-500 to-red-500 text-white font-extrabold text-3xl shadow-lg mb-4">
                ⚡
            </div>
            <h1 class="text-2xl font-black tracking-wide text-white">THẾ GIỚI SIÊU SALE</h1>
            <p class="text-sm text-gray-400 mt-1">Cổng Quản Trị Hệ Thống Săn Sale Đa Sàn</p>
        </div>

        <form method="POST" action="/admin/login" class="space-y-5">
            <div>
                <label class="block text-xs font-semibold uppercase tracking-wider text-gray-400 mb-2">Tài khoản Quản Trị</label>
                <input type="text" name="username" required autocomplete="username"
                    class="w-full px-4 py-3 bg-gray-800 border border-gray-700 rounded-xl text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-yellow-500 focus:border-transparent transition"
                    placeholder="Nhập tên đăng nhập admin...">
            </div>

            <div>
                <label class="block text-xs font-semibold uppercase tracking-wider text-gray-400 mb-2">Mật khẩu</label>
                <input type="password" name="password" required autocomplete="current-password"
                    class="w-full px-4 py-3 bg-gray-800 border border-gray-700 rounded-xl text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-yellow-500 focus:border-transparent transition"
                    placeholder="Nhập mật khẩu quản trị...">
            </div>

            <button type="submit"
                class="w-full py-3.5 px-4 bg-gradient-to-r from-yellow-500 via-orange-500 to-red-500 hover:from-yellow-600 hover:to-red-600 font-bold rounded-xl text-white shadow-lg shadow-orange-500/30 transform active:scale-95 transition duration-150">
                Đăng Nhập Hệ Thống
            </button>
        </form>

        <div class="mt-8 pt-6 border-t border-gray-800 text-center">
            <div class="inline-flex items-center text-xs text-gray-500 space-x-1">
                <span>🛡️</span>
                <span>Bảo mật 1 tài khoản Admin duy nhất · Đăng ký bị vô hiệu hoá</span>
            </div>
        </div>
    </div>
</body>
</html>"#;

    Html(html).into_response()
}

/// Xử lý xác thực đăng nhập (Chống Brute-force 5 lần / 15 phút & chống Timing Attack)
async fn handle_login_submit(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Response {
    let client_ip = extract_client_ip(&headers);
    let now_ts = Utc::now().timestamp();

    {
        let limiter = state.login_limiter.read().await;
        if let Some(&(attempts, last_ts)) = limiter.get(&client_ip) {
            if attempts >= 5 && (now_ts - last_ts) < 900 {
                let locked_html = r#"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <title>Tạm Khóa Đăng Nhập</title>
    <link href="https://cdn.jsdelivr.net/npm/tailwindcss@2.2.19/dist/tailwind.min.css" rel="stylesheet">
</head>
<body class="flex items-center justify-center min-h-screen bg-gray-950 text-white p-4">
    <div class="max-w-md w-full bg-gray-900 border border-yellow-500/50 p-6 rounded-2xl text-center shadow-2xl">
        <div class="text-5xl mb-4">🛡️</div>
        <h2 class="text-xl font-bold text-yellow-400 mb-2">Tạm Khóa Do Nhập Sai Nhiều Lần</h2>
        <p class="text-sm text-gray-400 mb-6">Bạn đã nhập sai quá 5 lần. Hệ thống tạm khóa đăng nhập trong 15 phút để bảo vệ tài khoản Quản Trị.</p>
        <a href="/admin/login" class="inline-block px-6 py-2.5 bg-gray-800 hover:bg-gray-700 rounded-xl font-semibold text-white transition">Quay Lại</a>
    </div>
</body>
</html>"#;
                return (StatusCode::TOO_MANY_REQUESTS, Html(locked_html)).into_response();
            }
        }
    }

    let user_match: bool = form
        .username
        .as_bytes()
        .ct_eq(state.config.admin_username.as_bytes())
        .into();
    let pass_match: bool = form
        .password
        .as_bytes()
        .ct_eq(state.config.admin_password.as_bytes())
        .into();

    if user_match && pass_match {
        {
            let mut limiter = state.login_limiter.write().await;
            limiter.remove(&client_ip);
        }

        let token = Uuid::new_v4().to_string();
        {
            let mut sessions = state.sessions.write().await;
            if sessions.len() >= 100 {
                sessions.clear();
            }
            sessions.insert(token.clone());
        }
        if let Ok(conn) = state.db.conn() {
            let _ = Repository::create_admin_session(&conn, &token, 86400).await;
        }

        info!("✅ Quản trị viên đăng nhập thành công vào Web Dashboard!");
        let secure_flag = if state.config.public_base_url.is_some() {
            "; Secure"
        } else {
            ""
        };
        let cookie_header = format!(
            "dealx_session={}; HttpOnly; Path=/; SameSite=Lax; Max-Age=86400{}",
            token, secure_flag
        );

        let mut response = Redirect::to("/admin").into_response();
        if let Ok(val) = header::HeaderValue::from_str(&cookie_header) {
            response.headers_mut().insert(header::SET_COOKIE, val);
        }
        response
    } else {
        let attempts_now = {
            let mut limiter = state.login_limiter.write().await;
            if limiter.len() >= 2000 {
                limiter.clear();
            }
            let entry = limiter.entry(client_ip.clone()).or_insert((0, now_ts));
            if (now_ts - entry.1) >= 900 {
                *entry = (1, now_ts);
            } else {
                entry.0 += 1;
                entry.1 = now_ts;
            }
            entry.0
        };
        let remaining = 5u32.saturating_sub(attempts_now);

        error!(
            "❌ Thử đăng nhập trái phép vào Web Admin Dashboard từ IP {} (lần {})",
            sanitize_crlf(&client_ip),
            attempts_now
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
        let error_html = format!(
            r#"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <title>Đăng Nhập Thất Bại</title>
    <link href="https://cdn.jsdelivr.net/npm/tailwindcss@2.2.19/dist/tailwind.min.css" rel="stylesheet">
</head>
<body class="flex items-center justify-center min-h-screen bg-gray-950 text-white p-4">
    <div class="max-w-md w-full bg-gray-900 border border-red-500/50 p-6 rounded-2xl text-center shadow-2xl">
        <div class="text-5xl mb-4">⛔</div>
        <h2 class="text-xl font-bold text-red-400 mb-2">Đăng Nhập Thất Bại</h2>
        <p class="text-sm text-gray-400 mb-6">Tài khoản hoặc mật khẩu quản trị viên không chính xác (còn <b>{}</b> lần thử trước khi tạm khóa 15 phút).</p>
        <a href="/admin/login" class="inline-block px-6 py-2.5 bg-red-600 hover:bg-red-700 rounded-xl font-semibold text-white transition">Thử Lại</a>
    </div>
</body>
</html>"#,
            remaining
        );
        (StatusCode::UNAUTHORIZED, Html(error_html)).into_response()
    }
}

/// Đăng xuất quản trị viên (xoá cả RAM cache và bảng admin_sessions trong DB)
async fn handle_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = extract_session_token(&headers) {
        {
            let mut sessions = state.sessions.write().await;
            sessions.remove(&token);
        }
        if let Ok(conn) = state.db.conn() {
            let _ = Repository::delete_admin_session(&conn, &token).await;
        }
    }

    let mut response = Redirect::to("/admin/login").into_response();
    let secure_flag = if state.config.public_base_url.is_some() {
        "; Secure"
    } else {
        ""
    };
    let logout_cookie = format!("dealx_session=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0{}", secure_flag);
    if let Ok(val) = header::HeaderValue::from_str(&logout_cookie) {
        response.headers_mut().insert(header::SET_COOKIE, val);
    }
    response
}

fn render_platform_status(label: &str, matched: &Option<(String, String)>, manual_id: &str) -> String {
    if !manual_id.trim().is_empty() {
        return format!(
            "<div class=\"flex items-center justify-between py-1.5 border-b border-gray-800/60\">\
                <span class=\"text-gray-300\">{}</span>\
                <span class=\"text-xs px-2 py-0.5 bg-green-500/20 text-green-400 rounded-md font-mono\">🟢 Thủ công (ID: {})</span>\
             </div>",
            label,
            escape_html(manual_id.trim())
        );
    }
    match matched {
        Some((id, _)) => format!(
            "<div class=\"flex items-center justify-between py-1.5 border-b border-gray-800/60\">\
                <span class=\"text-gray-300\">{}</span>\
                <span class=\"text-xs px-2 py-0.5 bg-green-500/20 text-green-400 rounded-md font-mono\">🟢 Đã duyệt (ID: {})</span>\
             </div>",
            label,
            escape_html(id)
        ),
        None => format!(
            "<div class=\"flex items-center justify-between py-1.5 border-b border-gray-800/60\">\
                <span class=\"text-gray-300\">{}</span>\
                <span class=\"text-xs px-2 py-0.5 bg-yellow-500/20 text-yellow-400 rounded-md\">🟡 Chưa duyệt trên AccessTrade</span>\
             </div>",
            label
        ),
    }
}

#[derive(serde::Deserialize, Default)]
struct DashboardQuery {
    notice: Option<String>,
    q: Option<String>,
    plat: Option<String>,
}

async fn resolve_accesstrade_token(conn: &libsql::Connection, config: &Config) -> Option<String> {
    Repository::get_setting(conn, "accesstrade_api_token")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| config.accesstrade_api_token.clone())
}

/// Giao diện chính Dashboard Quản Trị
async fn handle_dashboard(
    headers: HeaderMap,
    Query(query): Query<DashboardQuery>,
    State(state): State<AppState>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let conn = match state.db.conn() {
        Ok(c) => c,
        Err(e) => {
            return Html(format!("<h1>Lỗi kết nối cơ sở dữ liệu: {}</h1>", escape_html(&e.to_string()))).into_response();
        }
    };

    let stats = Repository::get_admin_dashboard_stats(&conn).await.unwrap_or_default();
    let clicks = Repository::get_recent_clicks_for_admin(&conn, 50).await.unwrap_or_default();
    let saved_deals = Repository::get_all_deals_for_admin(&conn).await.unwrap_or_default();

    let affiliate_template = Repository::get_setting(&conn, "affiliate_template")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let db_at_token = resolve_accesstrade_token(&conn, &state.config).await;

    let has_turso = state.config.turso_url.is_some() && state.config.turso_auth_token.is_some();
    let has_env_token = state.config.accesstrade_api_token.is_some();
    let persistence_banner = if !has_env_token || !has_turso {
        let mut notes = Vec::new();
        if !has_env_token {
            notes.push("Chưa đặt <code>ACCESSTRADE_API_TOKEN</code> trong Render Environment — Token nhập trên web sẽ mất khi Render deploy lại.");
        }
        if !has_turso {
            notes.push("Đang dùng SQLite cục bộ (gói Free của Render làm mới ổ đĩa khi build lại). Nên gắn <code>TURSO_DATABASE_URL</code> &amp; <code>TURSO_AUTH_TOKEN</code> để lưu vĩnh viễn.");
        }
        format!(
            r#"<div class="bg-yellow-500/10 border border-yellow-500/40 rounded-2xl p-4 text-xs text-yellow-200 space-y-1">
                <div class="font-bold text-yellow-400">⚠️ Cảnh Báo Lưu Trữ Tạm Thời Trên Render Free</div>
                {}
            </div>"#,
            notes.into_iter().map(|n| format!("<div>• {}</div>", n)).collect::<Vec<_>>().join("")
        )
    } else {
        r#"<div class="bg-green-500/10 border border-green-500/30 rounded-2xl px-4 py-3 text-xs text-green-300">
            🟢 <b>Lưu trữ bền vững:</b> Đã cố định <code>ACCESSTRADE_API_TOKEN</code> &amp; kết nối Turso Cloud Database.
        </div>"#.to_string()
    };

    let notice_banner = match query.notice.as_deref() {
        Some(n) if !n.trim().is_empty() => format!(
            r#"<div class="bg-blue-500/15 border border-blue-500/40 rounded-2xl px-4 py-3 text-sm text-blue-200 font-medium">ℹ️ {}</div>"#,
            escape_html(n)
        ),
        _ => String::new(),
    };

    let camp_shopee = Repository::get_setting(&conn, "campaign_shopee").await.ok().flatten().unwrap_or_default();
    let camp_lazada = Repository::get_setting(&conn, "campaign_lazada").await.ok().flatten().unwrap_or_default();
    let camp_tiki = Repository::get_setting(&conn, "campaign_tiki").await.ok().flatten().unwrap_or_default();
    let camp_tiktok = Repository::get_setting(&conn, "campaign_tiktok").await.ok().flatten().unwrap_or_default();

    let shopee_affiliate_id = Repository::get_setting(&conn, "shopee_affiliate_id")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| state.config.shopee_affiliate_id.clone())
        .unwrap_or_default();
    let shopee_app_id = Repository::get_setting(&conn, "shopee_app_id")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| state.config.shopee_app_id.clone())
        .unwrap_or_default();
    let shopee_app_secret = Repository::get_setting(&conn, "shopee_app_secret")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| state.config.shopee_app_secret.clone())
        .unwrap_or_default();
    let shopee_social_params = Repository::get_setting(&conn, "shopee_social_params")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| state.config.shopee_social_params.clone())
        .unwrap_or_default();
    let shortlink_base_url = Repository::get_setting(&conn, "shortlink_base_url")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| state.config.shortlink_base_url.clone())
        .unwrap_or_default();
    let shortlink_mobile_only = Repository::get_setting(&conn, "shortlink_mobile_only")
        .await
        .ok()
        .flatten()
        .map(|s| s.trim() != "0")
        .unwrap_or(true);
    let public_home_cloak = Repository::get_setting(&conn, "public_home_cloak")
        .await
        .ok()
        .flatten()
        .map(|s| s.trim() == "1")
        .unwrap_or(false);
    let origin_base_for_worker = state
        .config
        .public_base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("https://bot-sale.onrender.com");

    let admin_telegram_id_val = Repository::get_setting(&conn, "admin_telegram_id")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| state.config.admin_telegram_id.map(|id| id.to_string()))
        .unwrap_or_default();
    let tracked_orders = Repository::get_recent_tracked_orders(&conn, 20)
        .await
        .unwrap_or_default();

    let shopee_card_html = render_shopee_card(
        &shopee_affiliate_id,
        &shopee_app_id,
        &shopee_app_secret,
        &shopee_social_params,
        &shortlink_base_url,
        shortlink_mobile_only,
        public_home_cloak,
        origin_base_for_worker,
        &state.config.bot_username,
    );
    let order_alerts_card_html = render_order_alerts_card(&admin_telegram_id_val, &tracked_orders);

    let mut approved_opt: Option<ApprovedCampaigns> = None;
    let (at_status_badge, campaign_status_html) = if let Some(ref token) = db_at_token {
        let masked = mask_token(token);
        match state.accesstrade.get_approved_campaigns(token, false).await {
            Ok(approved) => {
                let badge = format!(
                    "<span class=\"px-2.5 py-1 bg-green-500/20 text-green-400 rounded-lg text-xs font-semibold\">🟢 Đã kết nối ({})</span>",
                    masked
                );
                let dyn_list = approved.dynamic_campaigns();
                let dyn_line = if dyn_list.is_empty() {
                    r#"<div class="text-xs text-gray-400 pt-2">✨ <b>Tự động nhận diện đa sàn:</b> Sẵn sàng — đăng ký thêm <i>Shein, CellphoneS, FPT Shop, Con Cưng, Fado, Klook...</i> trên <code>pub2.accesstrade.vn</code> sẽ tự kích hoạt không cần sửa code.</div>"#.to_string()
                } else {
                    let labels: Vec<String> = dyn_list
                        .iter()
                        .take(8)
                        .map(|c| format!("{} (<code>{}</code>)", escape_html(&c.platform_group), escape_html(&c.domain)))
                        .collect();
                    format!(
                        "<div class=\"text-xs text-green-300 pt-2\">✨ <b>Đã tự mở thêm {} sàn/hãng mở rộng:</b> {}</div>",
                        dyn_list.len(),
                        labels.join(", ")
                    )
                };
                let rows = format!(
                    "{}{}{}{}{}{}",
                    render_platform_status("🟠 Shopee", &approved.shopee, &camp_shopee),
                    render_platform_status("🔵 Lazada", &approved.lazada, &camp_lazada),
                    render_platform_status("🔹 Tiki", &approved.tiki, &camp_tiki),
                    render_platform_status("🎵 TikTok Shop", &approved.tiktok, &camp_tiktok),
                    render_platform_status("🌐 Alibaba", &approved.alibaba, ""),
                    dyn_line,
                );
                approved_opt = Some(approved);
                (badge, rows)
            }
            Err(err) => {
                let badge = format!(
                    "<span class=\"px-2.5 py-1 bg-red-500/20 text-red-400 rounded-lg text-xs font-semibold\">🔴 Lỗi kết nối ({})</span>",
                    masked
                );
                let err_box = format!("<p class=\"text-xs text-red-400\">{}</p>", escape_html(&err));
                (badge, err_box)
            }
        }
    } else {
        (
            "<span class=\"px-2.5 py-1 bg-gray-700 text-gray-300 rounded-lg text-xs font-semibold\">⚪ Chưa nhập API Key</span>".to_string(),
            "<p class=\"text-xs text-gray-400\">Vui lòng nhập <b>ACCESSTRADE API Access Key</b> bên dưới hoặc thiết lập biến môi trường <code>ACCESSTRADE_API_TOKEN</code> trên Render.</p>".to_string(),
        )
    };

    // Kho mã toàn thị trường đang cache (tự động cập nhật mỗi 60 giây)
    let sync_meta = state.accesstrade.get_voucher_sync_meta().await;
    let last_sync_display = if sync_meta.last_sync_vn.is_empty() {
        "Đang khởi tạo chu kỳ đầu tiên...".to_string()
    } else {
        format!("{} (Chu kỳ #{})", sync_meta.last_sync_vn, sync_meta.sync_cycles)
    };

    let cached_vouchers = state.accesstrade.peek_cached_high_discount_vouchers().await;
    let total_market_vouchers = cached_vouchers.len();
    let high_pct_vouchers = cached_vouchers.iter().filter(|v| v.discount_percent >= 20).count();
    let shopee_vouchers = cached_vouchers.iter().filter(|v| v.platform == "Shopee").count();
    let tiki_vouchers = cached_vouchers.iter().filter(|v| v.platform == "Tiki").count();

    let search_q = query.q.as_deref().unwrap_or("").trim().to_string();
    let filter_plat = query.plat.as_deref().unwrap_or("all").trim().to_string();
    let search_q_lower = search_q.to_lowercase();

    let filtered_vouchers: Vec<_> = cached_vouchers
        .iter()
        .filter(|v| {
            let plat_ok = match filter_plat.as_str() {
                "pct20" => v.discount_percent >= 20,
                "Shopee" => v.platform == "Shopee",
                "Tiki" => v.platform == "Tiki",
                "other" => v.discount_percent < 20,
                _ => true,
            };
            if !plat_ok {
                return false;
            }
            if search_q_lower.is_empty() {
                return true;
            }
            v.shop_name.to_lowercase().contains(&search_q_lower)
                || v.coupon_code.to_lowercase().contains(&search_q_lower)
                || v.discount_desc.to_lowercase().contains(&search_q_lower)
                || v.platform.to_lowercase().contains(&search_q_lower)
        })
        .collect();

    let mut voucher_rows = String::new();
    for v in filtered_vouchers.iter().take(50) {
        let target_url = v.best_target_url(approved_opt.as_ref());
        voucher_rows.push_str(&format!(
            r#"<tr class="border-b border-gray-800/80 hover:bg-gray-800/40">
                <td class="py-2.5 px-3 text-xs font-bold text-orange-400">{}</td>
                <td class="py-2.5 px-3 text-xs text-white font-semibold">{}</td>
                <td class="py-2.5 px-3 text-xs font-mono text-yellow-300">{}</td>
                <td class="py-2.5 px-3 text-xs text-gray-200">{}<div class="text-gray-400">{}</div></td>
                <td class="py-2.5 px-3 text-xs text-gray-400">{}</td>
                <td class="py-2.5 px-3 text-xs"><a href="{}" target="_blank" rel="noopener noreferrer" class="text-blue-400 hover:underline">Mở Link Chuẩn ↗</a></td>
            </tr>"#,
            escape_html(&v.discount_badge()),
            escape_html(&v.platform),
            escape_html(&v.coupon_code),
            escape_html(&v.shop_name),
            escape_html(&v.discount_desc),
            escape_html(&v.end_time),
            escape_html(&target_url)
        ));
    }
    if voucher_rows.is_empty() {
        voucher_rows = r#"<tr><td colspan="6" class="py-6 text-center text-xs text-gray-500">Không có mã nào khớp bộ lọc (hoặc đang khởi tạo lần quét đầu tiên). Bấm nút <b>Quét Ngay Toàn Bộ Kho Mã</b> nếu cần làm mới ngay.</td></tr>"#.to_string();
    }

    // Bảng danh sách Mã Độc Quyền Hợp Tác & Deals trong DB
    let mut exclusive_rows = String::new();
    for d in &saved_deals {
        let kind_badge = if d.is_exclusive {
            r#"<span class="px-2 py-0.5 bg-purple-500/20 text-purple-300 rounded text-xs font-semibold">👑 Độc Quyền</span>"#
        } else {
            r#"<span class="px-2 py-0.5 bg-gray-700 text-gray-300 rounded text-xs">Thường</span>"#
        };
        let status_badge = if d.is_active {
            r#"<span class="text-green-400 text-xs font-semibold">🟢 Đang bật</span>"#
        } else {
            r#"<span class="text-gray-500 text-xs">⚫ Đang tắt</span>"#
        };
        let code_display = d.coupon_code.as_deref().unwrap_or("-");
        let min_note = if d.min_spend > 0.0 {
            format!(" · Đơn từ {}", escape_html(&format_vnd(d.min_spend)))
        } else {
            String::new()
        };
        exclusive_rows.push_str(&format!(
            r#"<tr class="border-b border-gray-800/80 hover:bg-gray-800/40">
                <td class="py-2.5 px-3">{}</td>
                <td class="py-2.5 px-3 text-xs font-semibold text-white">{}</td>
                <td class="py-2.5 px-3 text-xs text-gray-200">{}<div class="text-gray-400">{}{}</div></td>
                <td class="py-2.5 px-3 text-xs font-mono text-yellow-300">{}</td>
                <td class="py-2.5 px-3">{}</td>
                <td class="py-2.5 px-3 text-xs">
                    <div class="flex items-center space-x-2">
                        <form method="POST" action="/admin/deals/toggle">
                            <input type="hidden" name="deal_id" value="{}">
                            <button type="submit" class="px-2 py-1 bg-gray-800 hover:bg-gray-700 border border-gray-700 rounded text-gray-200">Bật/Tắt</button>
                        </form>
                        <form method="POST" action="/admin/deals/delete">
                            <input type="hidden" name="deal_id" value="{}">
                            <button type="submit" class="px-2 py-1 bg-red-600/20 hover:bg-red-600/40 border border-red-500/40 rounded text-red-300">Xoá</button>
                        </form>
                    </div>
                </td>
            </tr>"#,
            kind_badge,
            escape_html(&d.platform),
            escape_html(&d.title),
            escape_html(&d.discount_desc),
            min_note,
            escape_html(code_display),
            status_badge,
            d.id,
            d.id
        ));
    }
    if exclusive_rows.is_empty() {
        exclusive_rows = r#"<tr><td colspan="6" class="py-6 text-center text-xs text-gray-500">Chưa có Mã Độc Quyền Hợp Tác nào. Hãy dán danh sách mã từ AM ACCESSTRADE / MCN / Nhãn hàng vào ô bên trên.</td></tr>"#.to_string();
    }

    // Render danh sách clicks gần đây (có escape chống XSS)
    let mut clicks_rows = String::new();
    for c in &clicks {
        clicks_rows.push_str(&format!(
            r#"<tr class="border-b border-gray-800 hover:bg-gray-800/40 transition">
                <td class="py-3 px-4 text-xs font-mono text-gray-400">{}</td>
                <td class="py-3 px-4 font-semibold text-white">{}</td>
                <td class="py-3 px-4 text-xs text-gray-400 max-w-xs truncate" title="{}">{}</td>
                <td class="py-3 px-4 text-xs text-blue-400 max-w-xs truncate"><a href="{}" target="_blank" rel="noopener noreferrer" class="hover:underline">Mở Link Đầu Ra ↗</a></td>
                <td class="py-3 px-4 text-xs text-gray-500 font-mono">{}</td>
            </tr>"#,
            c.user_id,
            escape_html(&c.platform),
            escape_html(&c.clean_url),
            escape_html(&c.clean_url),
            escape_html(&c.affiliate_url),
            escape_html(&c.created_at)
        ));
    }

    if clicks_rows.is_empty() {
        clicks_rows = r#"<tr><td colspan="5" class="py-8 text-center text-gray-500">Chưa có lượt dán link nào từ khách hàng.</td></tr>"#.to_string();
    }

    let bot_mode_badge = if state.webhook_tx.is_some() {
        r#"<span class="text-green-400 font-semibold">🟢 Webhook 24/7</span>"#
    } else {
        r#"<span class="text-blue-400 font-semibold">🔵 Polling</span>"#
    };

    let sel_all = if filter_plat == "all" { "selected" } else { "" };
    let sel_pct20 = if filter_plat == "pct20" { "selected" } else { "" };
    let sel_other = if filter_plat == "other" { "selected" } else { "" };
    let sel_shopee = if filter_plat == "Shopee" { "selected" } else { "" };
    let sel_tiki = if filter_plat == "Tiki" { "selected" } else { "" };
    let shown_vouchers_count = filtered_vouchers.len().min(50);
    let matched_vouchers_count = filtered_vouchers.len();
    let other_vouchers_count = total_market_vouchers.saturating_sub(high_pct_vouchers);

    let dashboard_html = format!(
        r#"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Thế Giới Siêu Sale - Quản Trị Hệ Thống Săn Sale</title>
    <link href="https://cdn.jsdelivr.net/npm/tailwindcss@2.2.19/dist/tailwind.min.css" rel="stylesheet">
    <style>
        body {{ background: #0b0f19; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }}
    </style>
</head>
<body class="text-gray-100 min-h-screen">
    <!-- Navbar -->
    <header class="border-b border-gray-800 bg-gray-900/60 backdrop-blur sticky top-0 z-50">
        <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 h-16 flex items-center justify-between">
            <div class="flex items-center space-x-3">
                <div class="w-10 h-10 rounded-xl bg-gradient-to-tr from-yellow-500 to-orange-500 flex items-center justify-center font-black text-xl text-white shadow-lg">
                    ⚡
                </div>
                <div>
                    <h1 class="font-extrabold text-lg text-white leading-tight">THẾ GIỚI SIÊU SALE</h1>
                    <p class="text-xs text-gray-400">Quản Trị Bot Săn Sale Đa Sàn</p>
                </div>
            </div>

            <div class="flex items-center space-x-4">
                <div class="hidden sm:flex items-center space-x-2 text-xs text-gray-400 bg-gray-800 px-3 py-1.5 rounded-xl border border-gray-700">
                    <span class="w-2 h-2 rounded-full bg-green-400 animate-pulse"></span>
                    <span>Admin: <strong class="text-white">{}</strong></span>
                    <span class="text-gray-600">|</span>
                    <span>Bot: <a href="https://t.me/{}" target="_blank" rel="noopener noreferrer" class="text-yellow-400 hover:underline">@{}</a></span>
                    <span class="text-gray-600">|</span>
                    {}
                </div>
                <a href="/admin/logout" class="px-3.5 py-1.5 bg-red-600/80 hover:bg-red-600 text-white rounded-xl text-xs font-semibold transition">
                    Đăng Xuất
                </a>
            </div>
        </div>
    </header>

    <main class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 space-y-8">
        {}
        {}
        <!-- Thống kê tổng quan & Đồng bộ Thời Gian Thực -->
        <section class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-5">
            <div class="bg-gray-900 border border-gray-800 p-5 rounded-2xl shadow-lg">
                <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Người Dùng Telegram</div>
                <div class="text-3xl font-black text-white">{}</div>
                <div class="text-xs text-green-400 mt-2">👥 Khách đã tương tác bot</div>
            </div>

            <div class="bg-gray-900 border border-gray-800 p-5 rounded-2xl shadow-lg">
                <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Tổng Lượt Dán Link</div>
                <div class="text-3xl font-black text-yellow-400">{}</div>
                <div class="text-xs space-y-0.5 mt-2 text-gray-400">
                    <div>🟠 Shopee: <strong class="text-white">{}</strong> · 🔵 Lazada: <strong class="text-white">{}</strong></div>
                    <div>🎵 TikTok: <strong class="text-white">{}</strong> · 🔹 Tiki: <strong class="text-white">{}</strong></div>
                </div>
            </div>

            <div class="bg-gray-900 border border-orange-500/30 p-5 rounded-2xl shadow-lg">
                <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Kho Mã Toàn Thị Trường</div>
                <div class="text-3xl font-black text-orange-400">{}</div>
                <div class="text-xs space-y-0.5 mt-2 text-gray-300">
                    <div>🔥 &gt;=20%: <strong class="text-white">{}</strong> · 💸 Tiền/Khác: <strong class="text-white">{}</strong></div>
                    <div>🟠 Shopee: <strong class="text-white">{}</strong> · 🔹 Tiki: <strong class="text-white">{}</strong> · 👑 Độc quyền: <strong class="text-purple-300">{}</strong></div>
                </div>
            </div>

            <div class="bg-gray-900 border border-green-500/30 p-5 rounded-2xl shadow-lg flex flex-col justify-between">
                <div>
                    <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Tự Động Quét Mỗi Phút</div>
                    <div class="text-sm font-bold text-green-400">⏱️ Chu kỳ 60 giây / lần</div>
                    <div class="text-xs text-gray-300 mt-1">Lần cuối: <strong class="text-white">{}</strong></div>
                </div>
                <div class="mt-3">
                    <a href="/admin" class="inline-block px-3 py-1.5 bg-gray-800 hover:bg-gray-700 border border-gray-700 rounded-lg text-xs text-gray-200 font-semibold transition">
                        🔄 Làm Mới Số Liệu Web
                    </a>
                </div>
            </div>
        </section>

        <!-- Kết nối ACCESSTRADE API Full 4 Sàn -->
        <section class="bg-gray-900 border border-gray-800 rounded-2xl p-6 shadow-xl space-y-5">
            <div class="flex flex-wrap items-center justify-between gap-2 border-b border-gray-800 pb-4">
                <div>
                    <h2 class="text-lg font-bold text-white flex items-center space-x-2">
                        <span>🚀</span>
                        <span>Kết Nối ACCESSTRADE API (Tự Động Full 4 Sàn)</span>
                    </h2>
                    <p class="text-xs text-gray-400 mt-1">
                        Bot tự động gọi API <code>v1/product_link/create</code> (kèm <code>utm_source=facebook</code>) &amp; TikTok v2 Universal Deeplink, đồng thời kiểm tra HTTP 302/200 chống lỗi link 404/500.
                    </p>
                </div>
                <div class="flex items-center space-x-2">
                    {}
                    <form method="POST" action="/admin/settings/accesstrade/sync" class="inline">
                        <button type="submit" class="px-3 py-1 bg-gray-800 hover:bg-gray-700 border border-gray-700 rounded-lg text-xs text-yellow-400 font-semibold transition">
                            🔄 Đồng Bộ Lại Chiến Dịch
                        </button>
                    </form>
                </div>
            </div>

            <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
                <div class="bg-gray-950/60 border border-gray-800 rounded-xl p-4 space-y-3">
                    <div class="text-xs font-bold uppercase tracking-wider text-gray-400">Trạng Thái Duyệt &amp; Chuẩn Hóa Link</div>
                    <div class="text-sm space-y-1">
                        {}
                    </div>
                    <div class="text-xs text-gray-400 bg-gray-900/80 border border-gray-800 rounded-lg p-2.5 space-y-1">
                        <div class="font-semibold text-gray-200">🛡️ Cơ chế bảo vệ Link Đầu Ra 100% Sống:</div>
                        <div>• <b>TikTok Shop:</b> Link v2 <code>tiktok.com/t/...</code> mở thẳng sản phẩm trên App.</div>
                        <div>• <b>Tiki &amp; Alibaba:</b> Tự dùng gói <b>TIKI CREATOR</b> (v2/3744) &amp; <b>ALIBABA</b> (v2/3713) đã duyệt + kiểm tra gateway <code>HTTP 200</code> trước khi gửi.</div>
                        <div>• <b>Shopee / Lazada:</b> Chờ duyệt ACCESSTRADE — có thể bật ngay Shopee Chính Chủ bên dưới để chạy song song.</div>
                    </div>
                    <p class="text-xs text-gray-500">
                        Sàn nào báo <b>Chưa duyệt</b>: Hãy vào <a href="https://pub2.accesstrade.vn" target="_blank" rel="noopener noreferrer" class="text-yellow-400 underline">pub2.accesstrade.vn</a> bấm Đăng ký chiến dịch sàn đó rồi nhấn <b>Đồng Bộ Lại</b>.
                    </p>
                </div>

                <form method="POST" action="/admin/settings/accesstrade" class="lg:col-span-2 space-y-4">
                    <div>
                        <label class="block text-xs font-semibold text-gray-400 mb-1">ACCESSTRADE API Access Key (Token)</label>
                        <input type="password" name="api_token" placeholder="Dán API Access Key mới (hoặc để trống để giữ Token hiện tại)..." class="w-full bg-gray-800 border border-gray-700 rounded-xl px-4 py-2.5 text-white font-mono text-sm focus:outline-none focus:ring-2 focus:ring-yellow-500">
                        <p class="text-xs text-gray-500 mt-1">💡 Đã tích hợp sẵn API Token dự phòng trong cấu hình — không bao giờ mất kết nối khi deploy lại.</p>
                    </div>

                    <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                        <div>
                            <label class="block text-xs font-semibold text-gray-400 mb-1">Campaign ID Shopee (Tùy chọn)</label>
                            <input type="text" name="campaign_shopee" value="{}" placeholder="Tự động nhận diện nếu để trống" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-yellow-500">
                        </div>
                        <div>
                            <label class="block text-xs font-semibold text-gray-400 mb-1">Campaign ID Lazada (Tùy chọn)</label>
                            <input type="text" name="campaign_lazada" value="{}" placeholder="Tự động nhận diện nếu để trống" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-yellow-500">
                        </div>
                        <div>
                            <label class="block text-xs font-semibold text-gray-400 mb-1">Campaign ID Tiki (Tùy chọn)</label>
                            <input type="text" name="campaign_tiki" value="{}" placeholder="Tự động nhận diện nếu để trống" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-yellow-500">
                        </div>
                        <div>
                            <label class="block text-xs font-semibold text-gray-400 mb-1">Campaign ID TikTok Shop (Tùy chọn)</label>
                            <input type="text" name="campaign_tiktok" value="{}" placeholder="Tự động nhận diện nếu để trống" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-yellow-500">
                        </div>
                    </div>

                    <button type="submit" class="px-6 py-2.5 bg-gradient-to-r from-yellow-500 to-orange-500 hover:from-yellow-600 hover:to-orange-600 text-gray-900 font-bold rounded-xl shadow-lg transition">
                        Lưu &amp; Kích Hoạt ACCESSTRADE API
                    </button>
                </form>
            </div>
        </section>

        {}

        {}

        <!-- Kho Mã Giảm Giá Toàn Thị Trường (Quét 100% không bỏ sót + Cập nhật mỗi 60 giây) -->
        <section class="bg-gray-900 border border-gray-800 rounded-2xl p-6 shadow-xl space-y-4">
            <div class="flex flex-wrap items-center justify-between gap-2 border-b border-gray-800 pb-4">
                <div>
                    <h2 class="text-lg font-bold text-white flex flex-wrap items-center gap-2">
                        <span>🔥</span>
                        <span>Kho Mã Giảm Giá Toàn Thị Trường (Cập Nhật Mỗi 60 Giây)</span>
                        <span class="px-2.5 py-0.5 bg-orange-500/20 text-orange-300 rounded-lg text-xs font-semibold">{} mã đang sẵn ({} mã &gt;=20%)</span>
                    </h2>
                    <p class="text-xs text-gray-400 mt-1">
                        Quét vét cạn từ 7 cổng ACCESSTRADE (mã %, giảm tiền K/VNĐ, mã hot, coupon Tiki/Shopee/Lazada/TikTok). Lần quét gần nhất: <strong class="text-green-400">{}</strong>.
                    </p>
                </div>
                <form method="POST" action="/admin/vouchers/sync">
                    <button type="submit" class="px-4 py-2 bg-orange-500/20 hover:bg-orange-500/30 border border-orange-500/40 text-orange-300 rounded-xl text-xs font-semibold transition">
                        🔄 Quét Sâu Toàn Bộ Ngay
                    </button>
                </form>
            </div>

            <!-- Thanh tìm kiếm & lọc trên Web -->
            <form method="GET" action="/admin" class="flex flex-wrap items-center gap-3 bg-gray-950/60 border border-gray-800 p-3 rounded-xl">
                <input type="text" name="q" value="{}" placeholder="Tìm tên shop, mã code, mức giảm (VD: 35%, LANGXDKKP, Tiki)..." class="flex-1 min-w-52 bg-gray-800 border border-gray-700 rounded-lg px-3 py-1.5 text-white text-xs focus:outline-none focus:ring-2 focus:ring-orange-500">
                <select name="plat" class="bg-gray-800 border border-gray-700 rounded-lg px-3 py-1.5 text-white text-xs">
                    <option value="all" {}>🌟 Tất Cả Sàn &amp; Loại</option>
                    <option value="pct20" {}>🔥 Giảm Sâu 20% - 35%+</option>
                    <option value="other" {}>💸 Giảm Tiền K/VNĐ &amp; Khác</option>
                    <option value="Shopee" {}>🟠 Chỉ Shopee</option>
                    <option value="Tiki" {}>🔹 Chỉ Tiki</option>
                </select>
                <button type="submit" class="px-4 py-1.5 bg-gray-800 hover:bg-gray-700 border border-gray-700 text-white rounded-lg text-xs font-semibold transition">
                    🔍 Lọc Mã
                </button>
                <span class="text-xs text-gray-400">Hiển thị <strong class="text-white">{}</strong> / <strong>{}</strong> mã khớp</span>
            </form>

            <div class="overflow-x-auto">
                <table class="w-full text-left">
                    <thead class="bg-gray-800/60 text-xs uppercase text-gray-400 border-b border-gray-800">
                        <tr>
                            <th class="py-2.5 px-3">Mức Giảm</th>
                            <th class="py-2.5 px-3">Sàn</th>
                            <th class="py-2.5 px-3">Mã Code</th>
                            <th class="py-2.5 px-3">Gian Hàng &amp; Điều Kiện Áp Mã</th>
                            <th class="py-2.5 px-3">HSD</th>
                            <th class="py-2.5 px-3">Link Áp Mã</th>
                        </tr>
                    </thead>
                    <tbody>
                        {}
                    </tbody>
                </table>
            </div>
        </section>

        <!-- Kho Mã Độc Quyền Hợp Tác (Cấp Riêng AM / MCN / Shop) -->
        <section class="bg-gray-900 border border-purple-500/30 rounded-2xl p-6 shadow-xl space-y-5">
            <div class="flex flex-wrap items-center justify-between gap-2 border-b border-gray-800 pb-4">
                <div>
                    <h2 class="text-lg font-bold text-white flex items-center space-x-2">
                        <span>👑</span>
                        <span>Kho Mã Độc Quyền Hợp Tác (Cấp Riêng AM / MCN / Nhãn Hàng)</span>
                        <span class="px-2.5 py-0.5 bg-purple-500/20 text-purple-300 rounded-lg text-xs font-semibold">{} mã đã lưu</span>
                    </h2>
                    <p class="text-xs text-gray-400 mt-1">
                        Dán hàng loạt mã độc quyền nhận được từ Account Manager ACCESSTRADE (MCN / Super Hoa Hồng) hoặc hợp tác trực tiếp với Shop. Các mã này luôn được <b>ưu tiên hàng đầu</b> trên Telegram Bot!
                    </p>
                </div>
            </div>

            <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
                <!-- Nhập hàng loạt -->
                <form method="POST" action="/admin/deals/import" class="bg-gray-950/60 border border-gray-800 rounded-xl p-4 space-y-3">
                    <div class="text-xs font-bold uppercase tracking-wider text-purple-300">⚡ Nhập Nhanh Hàng Loạt (Hỗ trợ dấu | hoặc copy thẳng từ Excel/Google Sheets)</div>
                    <p class="text-xs text-gray-400">
                        Mỗi dòng 1 mã theo thứ tự: <code>Sàn | Tên/Shop | Mã Code | Mức giảm | Link áp mã | Đơn tối thiểu</code>
                    </p>
                    <textarea name="bulk_lines" rows="5" required placeholder="Shopee | Deal Độc Quyền MCN 30% | ATVIP30 | Giảm 30% tối đa 100k | https://shopee.vn/search?... | 200000" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-purple-500"></textarea>
                    <button type="submit" class="px-4 py-2 bg-purple-600 hover:bg-purple-700 text-white font-semibold rounded-xl text-xs transition">
                        📥 Nhập Danh Sách Mã Độc Quyền
                    </button>
                </form>

                <!-- Thêm lẻ 1 mã -->
                <form method="POST" action="/admin/deals/add" class="bg-gray-950/60 border border-gray-800 rounded-xl p-4 space-y-3">
                    <div class="text-xs font-bold uppercase tracking-wider text-gray-300">➕ Thêm Lẻ 1 Mã Độc Quyền</div>
                    <div class="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
                        <select name="platform" class="bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white text-xs">
                            <option value="Shopee">Shopee</option>
                            <option value="TikTok Shop">TikTok Shop</option>
                            <option value="Lazada">Lazada</option>
                            <option value="Tiki">Tiki</option>
                            <option value="Alibaba">Alibaba</option>
                        </select>
                        <input type="text" name="coupon_code" required placeholder="Mã Code (VD: ATVIP35)" class="bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs">
                        <input type="text" name="title" required placeholder="Tên Shop / Chương trình" class="bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white text-xs">
                        <input type="text" name="discount_desc" required placeholder="Mức giảm (VD: Giảm 35% tối đa 120k)" class="bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white text-xs">
                        <input type="url" name="deal_url" required placeholder="Link áp mã (https://...)" class="bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs">
                        <input type="number" name="min_spend" min="0" step="1000" value="0" placeholder="Đơn tối thiểu (VNĐ)" class="bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs">
                    </div>
                    <label class="inline-flex items-center space-x-2 text-xs text-gray-300">
                        <input type="checkbox" name="is_exclusive" value="1" checked class="rounded bg-gray-800 border-gray-600">
                        <span>Đánh dấu là 👑 Mã Độc Quyền Hợp Tác (Ưu tiên hiển thị đầu tiên)</span>
                    </label>
                    <div>
                        <button type="submit" class="px-4 py-2 bg-gray-800 hover:bg-gray-700 border border-gray-700 text-white font-semibold rounded-xl text-xs transition">
                            💾 Lưu Mã Độc Quyền
                        </button>
                    </div>
                </form>
            </div>

            <div class="overflow-x-auto">
                <table class="w-full text-left">
                    <thead class="bg-gray-800/60 text-xs uppercase text-gray-400 border-b border-gray-800">
                        <tr>
                            <th class="py-2.5 px-3">Loại</th>
                            <th class="py-2.5 px-3">Sàn</th>
                            <th class="py-2.5 px-3">Tên &amp; Mô Tả</th>
                            <th class="py-2.5 px-3">Mã Code</th>
                            <th class="py-2.5 px-3">Trạng Thái</th>
                            <th class="py-2.5 px-3">Hành Động</th>
                        </tr>
                    </thead>
                    <tbody>
                        {}
                    </tbody>
                </table>
            </div>
        </section>

        <!-- Cấu hình Link Tiếp Thị Dự Phòng (Deeplink Template) -->
        <section class="bg-gray-900 border border-gray-800 rounded-2xl p-6 shadow-xl">
            <h2 class="text-lg font-bold text-white mb-2 flex items-center space-x-2">
                <span>🔗</span>
                <span>Cấu Hình Deeplink Dự Phòng (Nếu Chưa Dùng API Token)</span>
            </h2>
            <p class="text-xs text-gray-400 mb-4">
                Nếu chiến dịch trên API chưa duyệt, hệ thống sẽ kiểm tra mẫu link dự phòng này. Nếu để trống, bot tự động trả về link sản phẩm sạch để khách mở App mua hàng không bị lỗi 404.
            </p>
            <form method="POST" action="/admin/settings/affiliate" class="space-y-4">
                <div>
                    <label class="block text-xs font-semibold text-gray-400 mb-1">Mẫu Deeplink Template</label>
                    <input type="text" name="template" value="{}" placeholder="Ví dụ: https://go.isclix.com/deep_link/PUB_ID/CAMPAIGN_ID?url={{CLEAN_URL}}&utm_source=facebook&sub1={{USER_ID}}" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-4 py-2.5 text-white font-mono text-sm focus:outline-none focus:ring-2 focus:ring-yellow-500">
                </div>
                <button type="submit" class="px-5 py-2 bg-gray-800 hover:bg-gray-700 border border-gray-700 text-white font-semibold rounded-xl transition text-sm">
                    Lưu Deeplink Dự Phòng
                </button>
            </form>
        </section>

        <!-- Lịch sử dán link gần nhất -->
        <section class="bg-gray-900 border border-gray-800 rounded-2xl overflow-hidden shadow-xl">
            <div class="p-5 border-b border-gray-800">
                <h2 class="text-lg font-bold text-white flex items-center space-x-2">
                    <span>📋</span>
                    <span>Lịch Sử Khách Dán Link Săn Sale Gần Đây (50 lượt mới nhất)</span>
                </h2>
            </div>
            <div class="overflow-x-auto">
                <table class="w-full text-left">
                    <thead class="bg-gray-800/60 text-xs uppercase text-gray-400 border-b border-gray-800">
                        <tr>
                            <th class="py-3 px-4">Telegram ID</th>
                            <th class="py-3 px-4">Sàn</th>
                            <th class="py-3 px-4">Link Khách Gửi</th>
                            <th class="py-3 px-4">Link Đầu Ra</th>
                            <th class="py-3 px-4">Thời Gian</th>
                        </tr>
                    </thead>
                    <tbody>
                        {}
                    </tbody>
                </table>
            </div>
        </section>
    </main>

    <footer class="max-w-7xl mx-auto px-4 py-8 text-center text-xs text-gray-500 border-t border-gray-800/50 mt-12">
        Thế Giới Siêu Sale Affiliate System · Bản Quyền 2026 · Vận hành bảo mật trên Rust + LibSQL
    </footer>
    <script src="/assets/home.js" defer></script>
</body>
</html>"#,
        escape_html(&state.config.admin_username),
        escape_html(&state.config.bot_username),
        escape_html(&state.config.bot_username),
        bot_mode_badge,
        notice_banner,
        persistence_banner,
        stats.total_users,
        stats.total_clicks,
        stats.shopee_clicks,
        stats.lazada_clicks,
        stats.tiktok_clicks,
        stats.tiki_clicks,
        total_market_vouchers,
        high_pct_vouchers,
        other_vouchers_count,
        shopee_vouchers,
        tiki_vouchers,
        saved_deals.len(),
        escape_html(&last_sync_display),
        at_status_badge,
        campaign_status_html,
        escape_html(&camp_shopee),
        escape_html(&camp_lazada),
        escape_html(&camp_tiki),
        escape_html(&camp_tiktok),
        shopee_card_html,
        order_alerts_card_html,
        total_market_vouchers,
        high_pct_vouchers,
        escape_html(&last_sync_display),
        escape_html(&search_q),
        sel_all,
        sel_pct20,
        sel_other,
        sel_shopee,
        sel_tiki,
        shown_vouchers_count,
        matched_vouchers_count,
        voucher_rows,
        saved_deals.len(),
        exclusive_rows,
        escape_html(&affiliate_template),
        clicks_rows
    );

    Html(dashboard_html).into_response()
}

/// Form cập nhật cấu hình ACCESSTRADE API & Campaign ID
#[derive(serde::Deserialize)]
pub struct AccessTradeForm {
    pub api_token: String,
    pub campaign_shopee: String,
    pub campaign_lazada: String,
    pub campaign_tiki: String,
    pub campaign_tiktok: String,
}

async fn handle_update_accesstrade(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<AccessTradeForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let token_trimmed = form.api_token.trim();
        if !token_trimmed.is_empty() {
            let _ = Repository::set_setting(&conn, "accesstrade_api_token", token_trimmed).await;
            info!("✅ Quản trị viên đã cập nhật ACCESSTRADE API Token mới qua Web Admin.");
        }
        let _ = Repository::set_setting(&conn, "campaign_shopee", form.campaign_shopee.trim()).await;
        let _ = Repository::set_setting(&conn, "campaign_lazada", form.campaign_lazada.trim()).await;
        let _ = Repository::set_setting(&conn, "campaign_tiki", form.campaign_tiki.trim()).await;
        let _ = Repository::set_setting(&conn, "campaign_tiktok", form.campaign_tiktok.trim()).await;
    }

    state.accesstrade.invalidate_cache().await;
    Redirect::to("/admin").into_response()
}

fn render_shopee_card(
    aff_id: &str,
    app_id: &str,
    app_secret: &str,
    social_params: &str,
    shortlink_base_url: &str,
    shortlink_mobile_only: bool,
    public_home_cloak: bool,
    origin_base_url: &str,
    bot_username: &str,
) -> String {
    let status_badge = if !app_id.trim().is_empty() && !app_secret.trim().is_empty() {
        format!(
            r#"<span class="px-2.5 py-1 bg-green-500/20 text-green-400 rounded-lg text-xs font-semibold">🟢 Đang bật Open API (Secret: {})</span>"#,
            escape_html(&mask_token(app_secret))
        )
    } else if is_valid_shopee_affiliate_id(aff_id) {
        format!(
            r#"<span class="px-2.5 py-1 bg-green-500/20 text-green-400 rounded-lg text-xs font-semibold">🟢 Đang bật Direct Affiliate ID: {}</span>"#,
            escape_html(aff_id.trim())
        )
    } else {
        r#"<span class="px-2.5 py-1 bg-gray-700 text-gray-300 rounded-lg text-xs font-semibold">⚪ Chưa cấu hình — đang dùng luồng ACCESSTRADE</span>"#.to_string()
    };

    let secret_placeholder = if app_secret.trim().is_empty() {
        "Nhập Secret Key nếu đã được Shopee cấp Open API..."
    } else {
        "Để trống để giữ Secret Key hiện tại (đã lưu an toàn)..."
    };
    let social_placeholder = if social_params.trim().is_empty() {
        "Dán link/query phiên IG/FB Reels (có sm_sig_id / encrypted_payload) nếu có..."
    } else {
        "Đã lưu mẫu phiên IG/FB Reels (để trống để giữ nguyên mẫu hiện tại)..."
    };
    let social_badge = if social_params.trim().is_empty() {
        "Chưa gắn mẫu phiên IG/FB"
    } else {
        "🟢 Đã gắn bộ tham số IG/FB Reels"
    };
    let mobile_checked = if shortlink_mobile_only { "checked" } else { "" };
    let home_cloak_checked = if public_home_cloak { "checked" } else { "" };
    let mut worker_code = CLOUDFLARE_WORKER_TEMPLATE.replace(
        "https://bot-sale.onrender.com",
        origin_base_url.trim_end_matches('/'),
    );
    let bot_clean = bot_username.trim().trim_start_matches('@');
    if !bot_clean.is_empty() {
        worker_code = worker_code.replace(
            r#"const BOT_USERNAME = "vvipass_vip_bot";"#,
            &format!(r#"const BOT_USERNAME = "{}";"#, bot_clean),
        );
    }

    format!(
        r##"<section class="bg-gray-900 border border-orange-500/30 rounded-2xl p-6 shadow-xl space-y-4">
            <div class="flex flex-wrap items-center justify-between gap-2 border-b border-gray-800 pb-4">
                <div>
                    <h2 class="text-lg font-bold text-white flex items-center space-x-2">
                        <span>🟠</span>
                        <span>Kết Nối Shopee Affiliate Chính Chủ &amp; Bọc Link Riêng</span>
                    </h2>
                    <p class="text-xs text-gray-400 mt-1">
                        Bot tự động bọc 100% link đầu ra dạng <code>/8_ký_tự</code>, ưu tiên link <code>s.shopee.vn</code> chính chủ và che hoàn toàn tên miền Web Chính khi gắn Cloudflare Worker hoặc tên miền bọc riêng.
                    </p>
                </div>
                <div>{}</div>
            </div>
            <form method="POST" action="/admin/settings/shopee" class="space-y-4">
                <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                    <div>
                        <label class="block text-xs font-semibold text-gray-400 mb-1">Shopee Affiliate ID (Dùng ngay qua an_redir)</label>
                        <input type="text" name="shopee_affiliate_id" value="{}" placeholder="Ví dụ: 17375520550 (5-20 chữ số)" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-orange-500">
                    </div>
                    <div>
                        <label class="block text-xs font-semibold text-gray-400 mb-1">Shopee Open API AppID (Tùy chọn)</label>
                        <input type="text" name="shopee_app_id" value="{}" placeholder="Nhập AppID từ affiliate.shopee.vn..." class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-orange-500">
                    </div>
                    <div>
                        <label class="block text-xs font-semibold text-gray-400 mb-1">Shopee Open API Secret Key (Tùy chọn)</label>
                        <input type="password" name="shopee_app_secret" placeholder="{}" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-orange-500">
                    </div>
                </div>
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4 pt-2 border-t border-gray-800/80">
                    <div>
                        <label class="block text-xs font-semibold text-gray-400 mb-1">Bộ Tham Số Phiên IG/FB Reels Passthrough (<span class="text-orange-300">{}</span>)</label>
                        <input type="password" name="shopee_social_params" placeholder="{}" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-orange-500">
                        <label class="inline-flex items-center space-x-2 text-xs text-gray-400 mt-1.5">
                            <input type="checkbox" name="clear_social_params" value="1" class="rounded bg-gray-800 border-gray-600">
                            <span>Xóa bộ tham số phiên IG/FB đang lưu</span>
                        </label>
                    </div>
                    <div>
                        <label class="block text-xs font-semibold text-gray-400 mb-1">Tên Miền Bọc Link Riêng / Cloudflare Worker (Tùy chọn)</label>
                        <input type="text" name="shortlink_base_url" value="{}" placeholder="Mặc định: https://thegioisieusale.click (hoặc URL Cloudflare Worker khác)" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-orange-500">
                        <label class="inline-flex items-center space-x-2 text-xs text-gray-300 mt-1.5">
                            <input type="checkbox" name="shortlink_mobile_only" value="1" {} class="rounded bg-gray-800 border-gray-600">
                            <span>🛡️ Chặn máy tính soi link bọc (Chỉ cho điện thoại mở giống godeal.online)</span>
                        </label>
                        <label class="inline-flex items-center space-x-2 text-xs text-gray-300 mt-1.5 block">
                            <input type="checkbox" name="public_home_cloak" value="1" {} class="rounded bg-gray-800 border-gray-600">
                            <span>🙈 Ẩn luôn Trang chủ <code>/</code> trên Render thành <code>404 Not Found</code> (Chỉ bật nếu không chờ duyệt web Shopee/AT)</span>
                        </label>
                    </div>
                </div>
                <button type="submit" class="px-5 py-2 bg-orange-600 hover:bg-orange-700 text-white font-bold rounded-xl text-xs shadow-lg transition">
                    💾 Lưu Cấu Hình Shopee &amp; Bọc Link
                </button>
            </form>
            <details class="bg-gray-950/70 border border-gray-800 rounded-xl p-4">
                <summary class="cursor-pointer text-xs font-bold text-orange-300 select-none">
                    ☁️ Hướng dẫn tạo link bọc riêng 100% miễn phí bằng Cloudflare Worker (Che hoàn toàn web Render, trang chủ / hiện Trang Đích Bot Telegram)
                </summary>
                <div class="mt-3 space-y-3 text-xs text-gray-300">
                    <ol class="list-decimal list-inside space-y-1 text-gray-400">
                        <li>Vào <span class="text-white font-semibold">dash.cloudflare.com</span> (tài khoản miễn phí) → chọn <span class="text-white font-semibold">Workers &amp; Pages</span> → <span class="text-white font-semibold">Create Worker</span> → <span class="text-white font-semibold">Deploy</span> → <span class="text-white font-semibold">Edit code</span>.</li>
                        <li>Xóa hết code mẫu, dán toàn bộ đoạn code bên dưới vào rồi bấm <span class="text-white font-semibold">Deploy</span>.</li>
                        <li>Copy địa chỉ Worker (dạng <code class="text-orange-300">https://ten-worker.ten-ban.workers.dev</code>) dán vào ô <span class="text-white font-semibold">Tên Miền Bọc Link Riêng</span> ở trên và bấm Lưu.</li>
                        <li>Từ đó link bot Telegram trả ra hoàn toàn dùng domain mới; trang chủ <code>/</code> hiển thị Trang Đích Bot Telegram riêng và mọi đường dẫn nội bộ (<code>/admin</code>, <code>/convert</code>, <code>/health</code>...) đều trả <code>404 Not Found</code>!</li>
                    </ol>
                    <textarea id="cf-worker-code" readonly rows="10" class="w-full bg-gray-900 border border-gray-800 rounded-xl p-3 text-green-300 font-mono text-xs focus:outline-none">{}</textarea>
                    <button type="button" class="js-copy-link px-4 py-2 bg-gray-800 hover:bg-gray-700 border border-gray-700 text-white font-semibold rounded-xl text-xs transition" data-copy-target="cf-worker-code">
                        📋 Sao chép Code Cloudflare Worker
                    </button>
                </div>
            </details>
        </section>"##,
        status_badge,
        escape_html(aff_id),
        escape_html(app_id),
        secret_placeholder,
        social_badge,
        social_placeholder,
        escape_html(shortlink_base_url),
        mobile_checked,
        home_cloak_checked,
        escape_html(&worker_code)
    )
}

#[derive(serde::Deserialize)]
pub struct ShopeeSettingsForm {
    pub shopee_affiliate_id: String,
    pub shopee_app_id: String,
    pub shopee_app_secret: String,
    #[serde(default)]
    pub shopee_social_params: String,
    #[serde(default)]
    pub clear_social_params: Option<String>,
    #[serde(default)]
    pub shortlink_base_url: String,
    #[serde(default)]
    pub shortlink_mobile_only: Option<String>,
    #[serde(default)]
    pub public_home_cloak: Option<String>,
}

async fn handle_update_shopee_settings(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<ShopeeSettingsForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let aff_trimmed = form.shopee_affiliate_id.trim();
    if !aff_trimmed.is_empty() && !is_valid_shopee_affiliate_id(aff_trimmed) {
        return redirect_with_notice("Shopee Affiliate ID phải là dãy gồm 5 đến 20 chữ số (hoặc để trống để tắt).");
    }

    let short_base_trimmed = form.shortlink_base_url.trim().trim_end_matches('/');
    if !short_base_trimmed.is_empty() && !is_valid_http_url(short_base_trimmed) {
        return redirect_with_notice("Tên miền bọc link riêng phải bắt đầu bằng https:// hoặc http://.");
    }

    let social_trimmed = form.shopee_social_params.trim();
    let validated_social = if form.clear_social_params.as_deref() == Some("1") {
        Some(String::new())
    } else if !social_trimmed.is_empty() {
        match extract_social_passthrough_params(social_trimmed) {
            Some(clean) => Some(clean),
            None => {
                return redirect_with_notice(
                    "Mẫu phiên IG/FB Reels chưa hợp lệ (phải chứa sm_sig_id hoặc encrypted_payload).",
                );
            }
        }
    } else {
        None
    };

    let Ok(conn) = state.db.conn() else {
        return redirect_with_notice("Lỗi kết nối cơ sở dữ liệu.");
    };

    let _ = Repository::set_setting(&conn, "shopee_affiliate_id", aff_trimmed).await;
    let _ = Repository::set_setting(&conn, "shopee_app_id", form.shopee_app_id.trim()).await;
    let secret_trimmed = form.shopee_app_secret.trim();
    if !secret_trimmed.is_empty() {
        let _ = Repository::set_setting(&conn, "shopee_app_secret", secret_trimmed).await;
    }
    if let Some(social_val) = validated_social {
        let _ = Repository::set_setting(&conn, "shopee_social_params", &social_val).await;
    }
    let _ = Repository::set_setting(&conn, "shortlink_base_url", short_base_trimmed).await;
    let mobile_flag = if form.shortlink_mobile_only.as_deref() == Some("1") {
        "1"
    } else {
        "0"
    };
    let _ = Repository::set_setting(&conn, "shortlink_mobile_only", mobile_flag).await;
    let home_cloak_flag = if form.public_home_cloak.as_deref() == Some("1") {
        "1"
    } else {
        "0"
    };
    let _ = Repository::set_setting(&conn, "public_home_cloak", home_cloak_flag).await;

    info!("✅ Quản trị viên đã cập nhật cấu hình Shopee Chính Chủ & Bọc Link trên Web Admin.");
    redirect_with_notice("Đã lưu cấu hình Shopee Chính Chủ & Bộ Bọc Link an toàn!")
}

async fn handle_sync_accesstrade(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        if let Some(token) = resolve_accesstrade_token(&conn, &state.config).await {
            let _ = state.accesstrade.get_approved_campaigns(&token, true).await;
        }
    }

    Redirect::to("/admin").into_response()
}

fn render_order_alerts_card(admin_telegram_id: &str, orders: &[TrackedOrder]) -> String {
    let status_badge = if admin_telegram_id.trim().is_empty() {
        r#"<span class="px-2.5 py-1 bg-yellow-500/20 text-yellow-300 rounded-lg text-xs font-semibold">🟡 Chưa gắn Telegram ID nhận báo đơn</span>"#.to_string()
    } else {
        format!(
            r#"<span class="px-2.5 py-1 bg-green-500/20 text-green-400 rounded-lg text-xs font-semibold">🟢 Đang báo đơn tới ID: {}</span>"#,
            escape_html(admin_telegram_id.trim())
        )
    };

    let mut order_rows = String::new();
    for ord in orders {
        let status_cls = match ord.status.as_str() {
            "APPROVED" => "text-green-400",
            "REJECTED" => "text-red-400",
            _ => "text-yellow-300",
        };
        order_rows.push_str(&format!(
            r#"<tr class="border-b border-gray-800/80 hover:bg-gray-800/40">
                <td class="py-2.5 px-3 text-xs font-semibold text-white">{}</td>
                <td class="py-2.5 px-3 text-xs font-mono text-gray-200">{}</td>
                <td class="py-2.5 px-3 text-xs font-bold {}">{}</td>
                <td class="py-2.5 px-3 text-xs text-gray-200">{}</td>
                <td class="py-2.5 px-3 text-xs font-bold text-green-400">{}</td>
                <td class="py-2.5 px-3 text-xs font-mono text-gray-400">{}</td>
            </tr>"#,
            escape_html(&ord.platform),
            escape_html(&ord.order_id),
            status_cls,
            escape_html(&ord.status),
            escape_html(&format_vnd(ord.order_amount)),
            escape_html(&format_vnd(ord.commission)),
            escape_html(&ord.order_time)
        ));
    }
    if order_rows.is_empty() {
        order_rows = r#"<tr><td colspan="6" class="py-6 text-center text-xs text-gray-500">Chưa có đơn hàng phát sinh được ghi nhận trong 7 ngày gần nhất. Khi có đơn thật từ Shopee / TikTok Shop / ACCESSTRADE, hệ thống sẽ tự lưu và báo ngay về Telegram!</td></tr>"#.to_string();
    }

    format!(
        r#"<section class="bg-gray-900 border border-green-500/30 rounded-2xl p-6 shadow-xl space-y-4">
            <div class="flex flex-wrap items-center justify-between gap-2 border-b border-gray-800 pb-4">
                <div>
                    <h2 class="text-lg font-bold text-white flex items-center space-x-2">
                        <span>🔔</span>
                        <span>Báo Đơn Tự Động Về Telegram &amp; Đối Soát Hoa Hồng (Mỗi 10 Phút)</span>
                    </h2>
                    <p class="text-xs text-gray-400 mt-1">
                        Gõ lệnh <code>/stats</code> trong bot Telegram để lấy ID số của bạn rồi dán vào ô bên dưới. Mỗi khi có đơn mới trên ACCESSTRADE / Shopee Open API, bot sẽ nhắn tin báo ngay!
                    </p>
                </div>
                <div>{}</div>
            </div>
            <div class="flex flex-wrap items-end gap-3">
                <form method="POST" action="/admin/settings/notifications" class="flex flex-wrap items-end gap-3 flex-1">
                    <div class="flex-1 min-w-52">
                        <label class="block text-xs font-semibold text-gray-400 mb-1">Telegram ID Admin Nhận Thông Báo Đơn</label>
                        <input type="text" name="admin_telegram_id" value="{}" placeholder="Ví dụ: 5123456789 (hoặc để trống để tắt)" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white font-mono text-xs focus:outline-none focus:ring-2 focus:ring-green-500">
                    </div>
                    <button type="submit" class="px-4 py-2 bg-green-600 hover:bg-green-700 text-white font-bold rounded-xl text-xs transition">
                        💾 Lưu Telegram ID Báo Đơn
                    </button>
                </form>
                <form method="POST" action="/admin/orders/sync">
                    <button type="submit" class="px-4 py-2 bg-gray-800 hover:bg-gray-700 border border-gray-700 text-green-300 font-semibold rounded-xl text-xs transition">
                        🔄 Quét Đơn Mới Ngay
                    </button>
                </form>
            </div>
            <div class="overflow-x-auto">
                <table class="w-full text-left">
                    <thead class="bg-gray-800/60 text-xs uppercase text-gray-400 border-b border-gray-800">
                        <tr>
                            <th class="py-2.5 px-3">Sàn</th>
                            <th class="py-2.5 px-3">Mã Đơn Hàng</th>
                            <th class="py-2.5 px-3">Trạng Thái</th>
                            <th class="py-2.5 px-3">Giá Trị Đơn</th>
                            <th class="py-2.5 px-3">Hoa Hồng</th>
                            <th class="py-2.5 px-3">Thời Gian Đặt</th>
                        </tr>
                    </thead>
                    <tbody>
                        {}
                    </tbody>
                </table>
            </div>
        </section>"#,
        status_badge,
        escape_html(admin_telegram_id.trim()),
        order_rows
    )
}

#[derive(serde::Deserialize)]
pub struct NotificationsForm {
    pub admin_telegram_id: String,
}

async fn handle_update_notifications(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<NotificationsForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let trimmed = form.admin_telegram_id.trim();
    if !trimmed.is_empty() && trimmed.parse::<i64>().is_err() {
        return redirect_with_notice("Telegram ID phải là dãy số nguyên (lấy bằng cách gõ /stats trong Bot).");
    }

    let Ok(conn) = state.db.conn() else {
        return redirect_with_notice("Lỗi kết nối cơ sở dữ liệu.");
    };

    let _ = Repository::set_setting(&conn, "admin_telegram_id", trimmed).await;
    if trimmed.is_empty() {
        redirect_with_notice("Đã tắt thông báo đơn hàng qua Telegram.")
    } else {
        redirect_with_notice(&format!("Đã lưu Telegram ID {} để nhận báo đơn tự động!", trimmed))
    }
}

async fn handle_sync_orders(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let summary = order_tracker::sync_orders_once(&state.db, &state.config, &state.accesstrade).await;
    if summary.errors.is_empty() {
        redirect_with_notice(&format!(
            "Đối soát xong: Quét {} đơn trong 7 ngày qua, phát hiện {} đơn mới!",
            summary.fetched, summary.newly_inserted
        ))
    } else {
        redirect_with_notice(&format!(
            "Quét {} đơn (mới {}). Lưu ý: {}",
            summary.fetched,
            summary.newly_inserted,
            summary.errors.join("; ")
        ))
    }
}

fn redirect_with_notice(msg: &str) -> Response {
    let encoded: String = url::form_urlencoded::byte_serialize(msg.as_bytes()).collect();
    Redirect::to(&format!("/admin?notice={}", encoded)).into_response()
}

async fn handle_sync_vouchers(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let Ok(conn) = state.db.conn() else {
        return redirect_with_notice("Lỗi kết nối cơ sở dữ liệu.");
    };

    let Some(token) = resolve_accesstrade_token(&conn, &state.config).await else {
        return redirect_with_notice("Chưa cấu hình ACCESSTRADE API Token.");
    };

    match state.accesstrade.fetch_high_discount_vouchers(&token, 0, true).await {
        Ok(list) => redirect_with_notice(&format!(
            "Đã đồng bộ thành công tổng cộng {} mã giảm giá toàn thị trường từ ACCESSTRADE (không bỏ sót mã nào)!",
            list.len()
        )),
        Err(err) => redirect_with_notice(&format!("Lỗi đồng bộ kho mã: {}", err)),
    }
}

#[derive(serde::Deserialize)]
pub struct AddDealForm {
    pub platform: String,
    pub title: String,
    pub coupon_code: String,
    pub discount_desc: String,
    pub deal_url: String,
    pub min_spend: Option<f64>,
    pub is_exclusive: Option<String>,
}

fn is_valid_http_url(u: &str) -> bool {
    let trimmed = u.trim();
    trimmed.starts_with("https://") || trimmed.starts_with("http://")
}

async fn handle_add_deal(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<AddDealForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if !is_valid_http_url(&form.deal_url) {
        return redirect_with_notice("Link áp mã phải bắt đầu bằng https:// hoặc http://");
    }

    let Ok(conn) = state.db.conn() else {
        return redirect_with_notice("Lỗi kết nối cơ sở dữ liệu.");
    };

    let code_clean = form.coupon_code.trim();
    let item = NewDealInput {
        platform: form.platform.trim().to_string(),
        title: form.title.trim().to_string(),
        coupon_code: if code_clean.is_empty() {
            None
        } else {
            Some(code_clean.to_string())
        },
        discount_desc: form.discount_desc.trim().to_string(),
        min_spend: form.min_spend.unwrap_or(0.0).max(0.0),
        deal_url: form.deal_url.trim().to_string(),
        expire_at: None,
        is_exclusive: form.is_exclusive.is_some(),
    };

    match Repository::add_deal(&conn, &item).await {
        Ok(inserted) => {
            info!(
                "👑 Admin đã {} mã '{}' ({})",
                if inserted { "thêm mới" } else { "cập nhật" },
                sanitize_crlf(&item.title),
                sanitize_crlf(&item.platform)
            );
            redirect_with_notice(if inserted {
                "Đã lưu Mã Độc Quyền mới vào kho Bot!"
            } else {
                "Đã cập nhật lại Mã Độc Quyền trùng code trong kho!"
            })
        }
        Err(err) => redirect_with_notice(&format!("Lỗi lưu mã: {}", err)),
    }
}

#[derive(serde::Deserialize)]
pub struct BulkImportDealsForm {
    pub bulk_lines: String,
}

async fn handle_import_deals(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<BulkImportDealsForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let (items, skipped) = parse_bulk_exclusive_deals(&form.bulk_lines);
    if items.is_empty() {
        return redirect_with_notice(
            "Không có dòng nào đúng định dạng (Sàn | Tên | Mã Code | Mức giảm | Link https://... | Đơn tối thiểu).",
        );
    }

    let Ok(conn) = state.db.conn() else {
        return redirect_with_notice("Lỗi kết nối cơ sở dữ liệu.");
    };

    match Repository::add_deals_batch(&conn, &items).await {
        Ok((inserted, updated)) => {
            info!(
                "📥 Admin nhập hàng loạt mã độc quyền: +{} mới, ~{} cập nhật, bỏ qua {} dòng sai định dạng.",
                inserted, updated, skipped
            );
            redirect_with_notice(&format!(
                "Nhập xong: +{} mã mới, cập nhật {} mã (bỏ qua {} dòng sai định dạng).",
                inserted, updated, skipped
            ))
        }
        Err(err) => redirect_with_notice(&format!("Lỗi nhập hàng loạt: {}", err)),
    }
}

#[derive(serde::Deserialize)]
pub struct DealIdForm {
    pub deal_id: i64,
}

async fn handle_toggle_deal(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<DealIdForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let _ = Repository::toggle_deal(&conn, form.deal_id).await;
    }

    Redirect::to("/admin").into_response()
}

async fn handle_delete_deal(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<DealIdForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let _ = Repository::delete_deal(&conn, form.deal_id).await;
    }

    Redirect::to("/admin").into_response()
}

/// Phân tích văn bản nhập hàng loạt (hỗ trợ dấu `|` hoặc Tab từ Excel/Google Sheets)
pub fn parse_bulk_exclusive_deals(raw_text: &str) -> (Vec<NewDealInput>, usize) {
    let mut items = Vec::new();
    let mut skipped = 0usize;

    for raw_line in raw_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let sep = if line.contains('\t') { '\t' } else { '|' };
        let cols: Vec<&str> = line.split(sep).map(str::trim).collect();
        if cols.len() < 5 {
            skipped += 1;
            continue;
        }

        let platform = cols[0];
        let title = cols[1];
        let code = cols[2];
        let desc = cols[3];
        let url = cols[4];
        let min_spend = cols
            .get(5)
            .and_then(|s| s.replace([',', '.'], "").parse::<f64>().ok())
            .unwrap_or(0.0);

        if platform.is_empty() || title.is_empty() || desc.is_empty() || !is_valid_http_url(url) {
            skipped += 1;
            continue;
        }

        items.push(NewDealInput {
            platform: platform.to_string(),
            title: title.to_string(),
            coupon_code: if code.is_empty() {
                None
            } else {
                Some(code.to_string())
            },
            discount_desc: desc.to_string(),
            min_spend: min_spend.max(0.0),
            deal_url: url.to_string(),
            expire_at: None,
            is_exclusive: true,
        });
    }

    (items, skipped)
}

/// Form cập nhật cấu hình link Affiliate dự phòng
#[derive(serde::Deserialize)]
pub struct AffiliateSettingsForm {
    pub template: String,
}

/// Xử lý cập nhật mẫu link Affiliate
async fn handle_update_affiliate_settings(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<AffiliateSettingsForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let _ = Repository::set_setting(&conn, "affiliate_template", form.template.trim()).await;
        info!("✅ Đã cập nhật mẫu link Affiliate dự phòng trên Web Admin.");
    }

    Redirect::to("/admin").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn make_test_state(
        test_db_path: &str,
        webhook_tx: Option<WebhookUpdateSender>,
    ) -> AppState {
        let config = Config {
            telegram_token: "test".to_string(),
            bot_username: "vvipass_vip_bot".to_string(),
            database_path: test_db_path.to_string(),
            turso_url: None,
            turso_auth_token: None,
            default_cashback_rate: 70.0,
            default_referral_bonus_rate: 10.0,
            min_withdrawal_vnd: 50000.0,
            accesstrade_deeplink_template: "template".to_string(),
            accesstrade_api_token: None,
            shopee_affiliate_id: None,
            shopee_app_id: None,
            shopee_app_secret: None,
            shopee_social_params: None,
            shortlink_base_url: None,
            admin_username: "admin".to_string(),
            admin_password: "password".to_string(),
            admin_telegram_id: None,
            port: 10000,
            public_base_url: Some("https://bot-sale.onrender.com".to_string()),
            telegram_webhook_secret: "valid_secret_xyz".to_string(),
        };
        let db = AppDb::init(&config).await.unwrap();
        let accesstrade = Arc::new(AccessTradeClient::new());
        let parser = Arc::new(DealParser::with_accesstrade(accesstrade.clone()));
        AppState {
            db,
            config,
            accesstrade,
            parser,
            sessions: Arc::new(RwLock::new(HashSet::new())),
            login_limiter: Arc::new(RwLock::new(HashMap::new())),
            convert_limiter: Arc::new(RwLock::new(HashMap::new())),
            webhook_tx,
        }
    }

    #[test]
    fn test_extract_session_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "dealx_session=test-123-abc; other=xyz".parse().unwrap(),
        );
        assert_eq!(extract_session_token(&headers), Some("test-123-abc".to_string()));

        let empty_headers = HeaderMap::new();
        assert_eq!(extract_session_token(&empty_headers), None);
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<script>alert('1')</script>"), "&lt;script&gt;alert(&#39;1&#39;)&lt;/script&gt;");
    }

    #[tokio::test]
    async fn test_router_build() {
        let test_db_path = format!("target/test_web_{}.db", uuid::Uuid::new_v4().simple());
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = make_test_state(&test_db_path, Some(tx)).await;

        let _app: Router = build_router(state);
        let _ = std::fs::remove_file(&test_db_path);
    }

    #[tokio::test]
    async fn test_webhook_secret_verification_and_dispatch() {
        let test_db_path = format!("target/test_wh_{}.db", uuid::Uuid::new_v4().simple());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let state = make_test_state(&test_db_path, Some(tx)).await;

        // 1. Sai secret header phải bị từ chối 401
        let mut bad_headers = HeaderMap::new();
        bad_headers.insert("x-telegram-bot-api-secret-token", "wrong".parse().unwrap());
        let status_bad = handle_telegram_webhook(
            State(state.clone()),
            bad_headers,
            "{}".to_string(),
        )
        .await;
        assert_eq!(status_bad, StatusCode::UNAUTHORIZED);
        assert!(rx.try_recv().is_err());

        // 2. Đúng secret header + Update JSON hợp lệ phải nhận 200 và đẩy vào channel
        let mut good_headers = HeaderMap::new();
        good_headers.insert(
            "x-telegram-bot-api-secret-token",
            "valid_secret_xyz".parse().unwrap(),
        );
        let sample_update = r#"{"update_id":123456,"message":{"message_id":1,"date":1700000000,"chat":{"id":1001,"type":"private","first_name":"Test"},"text":"/start"}}"#;
        let status_ok = handle_telegram_webhook(
            State(state),
            good_headers,
            sample_update.to_string(),
        )
        .await;
        assert_eq!(status_ok, StatusCode::OK);
        let received = rx.try_recv().expect("Must receive dispatched update");
        assert_eq!(received.unwrap().id.0, 123456);

        let _ = std::fs::remove_file(&test_db_path);
    }

    #[test]
    fn test_parse_bulk_exclusive_deals_pipe_tab_and_xss_guard() {
        let input = "\
Shopee | Deal MCN Độc Quyền | MCN30 | Giảm 30% tối đa 100k | https://shopee.vn/promo | 200.000
TikTok Shop\tLive Độc Quyền\tTTS35\tGiảm 35% tối đa 120k\thttps://www.tiktok.com/shop\t150000
Shopee | Link XSS | BAD | Giảm 50% | javascript:alert(1) | 0
DongLoiThieuCot | 123
";
        let (parsed, skipped) = parse_bulk_exclusive_deals(input);
        assert_eq!(parsed.len(), 2);
        assert_eq!(skipped, 2);

        assert_eq!(parsed[0].platform, "Shopee");
        assert_eq!(parsed[0].coupon_code.as_deref(), Some("MCN30"));
        assert_eq!(parsed[0].min_spend, 200000.0);
        assert!(parsed[0].is_exclusive);

        assert_eq!(parsed[1].platform, "TikTok Shop");
        assert_eq!(parsed[1].coupon_code.as_deref(), Some("TTS35"));
        assert_eq!(parsed[1].min_spend, 150000.0);
    }

    #[tokio::test]
    async fn test_public_home_renders_200_and_contains_disclosure() {
        let test_db_path = format!("target/test_home_{}.db", uuid::Uuid::new_v4().simple());
        let state = make_test_state(&test_db_path, None).await;

        let resp = handle_root(Query(PublicHomeQuery::default()), State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let html = String::from_utf8(body_bytes.to_vec()).unwrap();

        assert!(html.contains(r#"<html lang="vi">"#));
        assert!(html.contains("vvipass_vip_bot"));
        assert!(html.contains("Thế Giới Siêu Sale"));
        assert!(!html.contains("DealX"));
        assert!(html.contains("action=\"/convert\""));
        assert!(html.contains("<form"));
        assert!(html.contains("Affiliate Disclosure"));
        assert!(!html.contains("Đăng Nhập Quản Trị"));

        let _ = std::fs::remove_file(&test_db_path);
    }

    #[tokio::test]
    async fn test_shortlink_redirect_preview_desktop_and_mobile() {
        let test_db_path = format!("target/test_sl_{}.db", uuid::Uuid::new_v4().simple());
        let state = make_test_state(&test_db_path, None).await;
        let secret_target = "https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fproduct%2F1%2F2&affiliate_id=17375520550&sm_sig_id=SIGTEST123";
        let code = {
            let conn = state.db.conn().unwrap();
            Repository::create_or_get_short_link(&conn, secret_target, "Sữa bột Enfagrow Test", "Shopee")
                .await
                .unwrap()
        };

        // 1. TelegramBot preview -> 200 HTML sạch, không lộ secret_target
        let mut preview_hdrs = HeaderMap::new();
        preview_hdrs.insert(header::USER_AGENT, "TelegramBot (like TwitterBot)".parse().unwrap());
        let resp_preview = handle_shortlink_redirect(
            Path(code.clone()),
            preview_hdrs,
            State(state.clone()),
        )
        .await;
        assert_eq!(resp_preview.status(), StatusCode::OK);
        let preview_body = String::from_utf8(
            axum::body::to_bytes(resp_preview.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(preview_body.contains("Sữa bột Enfagrow Test"));
        assert!(!preview_body.contains("SIGTEST123"));

        // 2. Desktop Windows -> 403 Forbidden (mặc định mobile_only bật), không có location & không lộ secret_target
        let mut desktop_hdrs = HeaderMap::new();
        desktop_hdrs.insert(
            header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/122.0.0.0 Safari/537.36"
                .parse()
                .unwrap(),
        );
        let resp_desktop = handle_shortlink_redirect(
            Path(code.clone()),
            desktop_hdrs,
            State(state.clone()),
        )
        .await;
        assert_eq!(resp_desktop.status(), StatusCode::FORBIDDEN);
        assert!(resp_desktop.headers().get(header::LOCATION).is_none());
        let desktop_body = String::from_utf8(
            axum::body::to_bytes(resp_desktop.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!desktop_body.contains("SIGTEST123"));

        // 3. Mobile iPhone khi mobile_only bật -> 200 HTML bridge (mã hóa XOR + /assets/bridge.js, không lộ thô SIGTEST123)
        let mut mobile_hdrs = HeaderMap::new();
        mobile_hdrs.insert(
            header::USER_AGENT,
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148 Safari/604.1"
                .parse()
                .unwrap(),
        );
        let resp_mobile = handle_shortlink_redirect(
            Path(code.clone()),
            mobile_hdrs.clone(),
            State(state.clone()),
        )
        .await;
        assert_eq!(resp_mobile.status(), StatusCode::OK);
        assert_eq!(
            resp_mobile.headers().get("referrer-policy").unwrap().to_str().unwrap(),
            "unsafe-url"
        );
        let mobile_body = String::from_utf8(
            axum::body::to_bytes(resp_mobile.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(mobile_body.contains("/assets/bridge.js"));
        assert!(!mobile_body.contains("SIGTEST123"));

        // Trích xuất payload/key và xác nhận decode_bridge_payload giải mã khớp 100%
        let p_hex = mobile_body.split("data-p=\"").nth(1).unwrap().split('"').next().unwrap();
        let k_hex = mobile_body.split("data-k=\"").nth(1).unwrap().split('"').next().unwrap();
        assert_eq!(decode_bridge_payload(p_hex, k_hex).as_deref(), Some(secret_target));

        // 4. Tắt shortlink_mobile_only -> trả 302 Found trực tiếp tới secret_target
        {
            let conn = state.db.conn().unwrap();
            Repository::set_setting(&conn, "shortlink_mobile_only", "0").await.unwrap();
        }
        let resp_302 = handle_shortlink_redirect(
            Path(code),
            mobile_hdrs,
            State(state),
        )
        .await;
        assert_eq!(resp_302.status(), StatusCode::FOUND);
        assert_eq!(
            resp_302.headers().get(header::LOCATION).unwrap().to_str().unwrap(),
            secret_target
        );

        let _ = std::fs::remove_file(&test_db_path);
    }

    #[tokio::test]
    async fn test_login_rate_limiter_blocks_brute_force() {
        let test_db_path = format!("target/test_rl_{}.db", uuid::Uuid::new_v4().simple());
        let state = make_test_state(&test_db_path, None).await;

        let mut hdrs = HeaderMap::new();
        hdrs.insert("x-forwarded-for", "203.0.113.77".parse().unwrap());

        for _ in 0..5 {
            let resp = handle_login_submit(
                hdrs.clone(),
                State(state.clone()),
                Form(LoginForm {
                    username: "admin".to_string(),
                    password: "wrong-password".to_string(),
                }),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        // Lần thứ 6 cùng IP phải bị chặn 429 Too Many Requests ngay (kể cả khi nhập đúng)
        let resp_locked = handle_login_submit(
            hdrs,
            State(state),
            Form(LoginForm {
                username: "admin".to_string(),
                password: "password".to_string(),
            }),
        )
        .await;
        assert_eq!(resp_locked.status(), StatusCode::TOO_MANY_REQUESTS);

        let _ = std::fs::remove_file(&test_db_path);
    }

    #[tokio::test]
    async fn test_public_convert_empty_or_invalid_link_returns_200_error_banner() {
        let test_db_path = format!("target/test_conv_{}.db", uuid::Uuid::new_v4().simple());
        let state = make_test_state(&test_db_path, None).await;

        let resp = handle_public_convert(
            HeaderMap::new(),
            State(state),
            Form(PublicConvertForm {
                url: "khong-phai-link".to_string(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = String::from_utf8(
            axum::body::to_bytes(resp.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(body.contains("convert-alert--error"));
        assert!(body.contains("Vui lòng dán đúng đường link sản phẩm"));

        let _ = std::fs::remove_file(&test_db_path);
    }

    #[test]
    fn test_shortlink_cloak_path_and_host_helpers() {
        assert!(is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/AbCd1234"
        ));
        assert!(is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/go/AbCd1234"
        ));
        assert!(is_allowed_shortlink_cloak_path(
            &axum::http::Method::HEAD,
            "/assets/bridge.js"
        ));
        assert!(!is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/"
        ));
        assert!(!is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/admin"
        ));
        assert!(!is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/admin/login"
        ));
        assert!(!is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/assets/home.js"
        ));
        assert!(!is_allowed_shortlink_cloak_path(
            &axum::http::Method::POST,
            "/convert"
        ));
        assert!(!is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/health"
        ));
        assert!(!is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/convert"
        ));

        assert_eq!(
            extract_host_only("https://www.GoDeal.online:8443/Ab12"),
            Some("godeal.online".to_string())
        );
        assert_eq!(
            extract_host_only("my-worker.workers.dev"),
            Some("my-worker.workers.dev".to_string())
        );
        assert_eq!(extract_host_only("   "), None);
    }

    #[tokio::test]
    async fn test_shortlink_cloak_router_e2e_header_and_custom_host() {
        let test_db_path = format!("target/test_cloak_e2e_{}.db", uuid::Uuid::new_v4().simple());
        let mut state = make_test_state(&test_db_path, None).await;
        state.config.public_base_url = Some("https://bot-sale.onrender.com".to_string());

        let code = {
            let conn = state.db.conn().unwrap();
            Repository::set_setting(&conn, "shortlink_base_url", "https://short.godeal-test.online")
                .await
                .unwrap();
            Repository::create_or_get_short_link(
                &conn,
                "https://s.shopee.vn/cloak-test",
                "",
                "Shopee",
            )
            .await
            .unwrap()
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = build_router(state);
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = reqwest::Client::new();
        let base = format!("http://{}", addr);

        // 1. Web chính: / mở được 200 OK nhưng KHÔNG chứa đường dẫn /admin
        let home_resp = client.get(format!("{}/", base)).send().await.unwrap();
        assert_eq!(home_resp.status().as_u16(), 200);
        let home_body = home_resp.text().await.unwrap();
        assert!(!home_body.contains("href=\"/admin\""));
        assert!(!home_body.contains("Quản trị hệ thống"));

        // 2. Truy cập qua Cloudflare Worker (X-DealX-Cloak: 1): khóa sạch /, /admin, /convert, /health -> 404
        for hidden_path in ["/", "/admin", "/admin/login", "/health", "/convert", "/assets/home.js"] {
            let resp = client
                .get(format!("{}{}", base, hidden_path))
                .header("x-dealx-cloak", "1")
                .send()
                .await
                .unwrap();
            assert_eq!(
                resp.status().as_u16(),
                404,
                "Path {} phai tra 404 khi co X-DealX-Cloak",
                hidden_path
            );
        }
        let convert_cloaked = client
            .post(format!("{}/convert", base))
            .header("x-dealx-cloak", "1")
            .header("content-type", "application/x-www-form-urlencoded")
            .body("url=https%3A%2F%2Fshopee.vn%2Ftest")
            .send()
            .await
            .unwrap();
        assert_eq!(convert_cloaked.status().as_u16(), 404);

        // Bridge JS va ma shortlink van hoat dong 200 va khong lo thuong hieu / domain Web Chinh
        let bridge_resp = client
            .get(format!("{}/assets/bridge.js", base))
            .header("x-dealx-cloak", "1")
            .send()
            .await
            .unwrap();
        assert_eq!(bridge_resp.status().as_u16(), 200);

        let preview_resp = client
            .get(format!("{}/{}", base, code))
            .header("x-dealx-cloak", "1")
            .header("user-agent", "facebookexternalhit/1.1")
            .send()
            .await
            .unwrap();
        assert_eq!(preview_resp.status().as_u16(), 200);
        let preview_body = preview_resp.text().await.unwrap();
        assert!(!preview_body.contains("DealX"));
        assert!(!preview_body.contains("Thế Giới Siêu Sale"));
        assert!(!preview_body.contains("onrender.com"));
        assert!(!preview_body.contains("/admin"));

        // 3. Truy cập trực tiếp bằng Host của domain shortlink riêng -> /, /admin đều trả 404
        let custom_root = client
            .get(format!("{}/", base))
            .header("host", "short.godeal-test.online")
            .send()
            .await
            .unwrap();
        assert_eq!(custom_root.status().as_u16(), 404);

        let custom_admin = client
            .get(format!("{}/admin", base))
            .header("host", "short.godeal-test.online")
            .send()
            .await
            .unwrap();
        assert_eq!(custom_admin.status().as_u16(), 404);

        server_task.abort();
        let _ = std::fs::remove_file(&test_db_path);
    }

    #[tokio::test]
    async fn test_public_home_cloak_toggle_hides_root_and_convert_keeps_admin_alive() {
        let test_db_path = format!("target/test_home_cloak_{}.db", uuid::Uuid::new_v4().simple());
        let state = make_test_state(&test_db_path, None).await;
        {
            let conn = state.db.conn().unwrap();
            Repository::set_setting(&conn, "public_home_cloak", "1")
                .await
                .unwrap();
        }

        let resp_root = handle_root(Query(PublicHomeQuery::default()), State(state.clone())).await;
        assert_eq!(resp_root.status(), StatusCode::NOT_FOUND);

        let resp_convert = handle_public_convert(
            HeaderMap::new(),
            State(state.clone()),
            Form(PublicConvertForm {
                url: "https://shopee.vn/test".to_string(),
            }),
        )
        .await;
        assert_eq!(resp_convert.status(), StatusCode::NOT_FOUND);

        // Trang đăng nhập /admin/login vẫn hoạt động bình thường để Admin tắt lại bất kỳ lúc nào
        let resp_login = handle_login_page(HeaderMap::new(), State(state)).await;
        assert_eq!(resp_login.status(), StatusCode::OK);

        let _ = std::fs::remove_file(&test_db_path);
    }

    #[test]
    fn test_cloudflare_worker_template_has_bot_landing_and_blocks_admin() {
        assert!(CLOUDFLARE_WORKER_TEMPLATE.contains(r#"path === "/""#));
        assert!(CLOUDFLARE_WORKER_TEMPLATE.contains("renderBotLandingHtml()"));
        assert!(CLOUDFLARE_WORKER_TEMPLATE.contains("https://t.me/"));
        assert!(CLOUDFLARE_WORKER_TEMPLATE.contains("Thế Giới Siêu Sale"));
        assert!(CLOUDFLARE_WORKER_TEMPLATE.contains("x-dealx-cloak"));

        let html_start = CLOUDFLARE_WORKER_TEMPLATE
            .find("<!DOCTYPE html>")
            .expect("Phải có HTML trang đích bot trong cloudflare_worker.js");
        let html_end = CLOUDFLARE_WORKER_TEMPLATE[html_start..]
            .find("</html>")
            .expect("Phải đóng thẻ </html>")
            + html_start;
        let landing_html = &CLOUDFLARE_WORKER_TEMPLATE[html_start..=html_end + 6];

        assert!(!landing_html.contains("/admin"));
        assert!(!landing_html.contains("onrender.com"));
        assert!(!landing_html.contains("DealX"));
    }
}
