use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use axum::{
    extract::{ConnectInfo, Form, Path, Query, Request, State},
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

static DESKTOP_WAKE_HOOK: OnceLock<Arc<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Đăng ký hàm đánh thức cửa sổ Desktop khi người dùng mở lại file `.exe` lúc app đang thu gọn dưới khay hệ thống.
pub fn set_desktop_wake_hook<F>(hook: F)
where
    F: Fn() + Send + Sync + 'static,
{
    let _ = DESKTOP_WAKE_HOOK.set(Arc::new(hook));
}

async fn handle_internal_show(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    let is_local_pc = state.config.public_base_url.is_none()
        && !headers.contains_key("cf-ray")
        && !headers.contains_key("cf-connecting-ip")
        && !headers.contains_key("x-dealx-cloak");
    let host_ok = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .and_then(extract_host_only)
        .map(|h| matches!(h.as_str(), "127.0.0.1" | "localhost" | "[::1]" | "::1"))
        .unwrap_or(false);
    if !is_local_pc || !host_ok {
        return not_found_cloak_response();
    }
    if let Some(hook) = DESKTOP_WAKE_HOOK.get() {
        hook();
    }
    (StatusCode::OK, "OK").into_response()
}

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

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    Router::new()
        // Các route công khai
        .route("/", get(handle_root))
        .route("/convert", post(handle_public_convert).get(|| async { Redirect::to("/") }))
        .route("/assets/bridge.js", get(handle_bridge_js))
        .route("/assets/home.js", get(handle_home_js))
        .route("/assets/bot-logo.webp", get(handle_bot_logo_webp))
        .route("/assets/bot-cart-3d.webp", get(handle_bot_cart_3d_webp))
        .route("/assets/landing.js", get(handle_landing_js))
        .route("/health", get(handle_health))
        .route("/internal/show", get(handle_internal_show))
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
        HeaderValue::from_static("default-src 'self'; script-src 'self' 'unsafe-inline' https://cdn.tailwindcss.com https://cdn.jsdelivr.net; style-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com data:; img-src 'self' data: https:; connect-src 'self' https:;"),
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
    if matches!(
        path,
        "/assets/bridge.js"
            | "/assets/bot-logo.webp"
            | "/assets/bot-cart-3d.webp"
            | "/assets/landing.js"
    ) {
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

const LOCAL_PC_TRUSTED_HEADER: &str = "x-dealx-local-trusted";

fn is_loopback_host(headers: &HeaderMap) -> bool {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .and_then(extract_host_only)
        .map(|h| matches!(h.as_str(), "127.0.0.1" | "localhost" | "[::1]" | "::1"))
        .unwrap_or(false)
}

fn has_proxy_or_tunnel_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("cf-ray")
        || headers.contains_key("cf-connecting-ip")
        || headers.contains_key("x-forwarded-for")
        || headers.contains_key("x-forwarded-host")
        || headers.contains_key("x-dealx-cloak")
}

/// Chỉ cho phép bỏ qua đăng nhập Admin khi chạy tại máy PC (`public_base_url` trống),
/// kết nối loopback trực tiếp và không đi qua Cloudflare Tunnel / Reverse Proxy.
fn is_local_pc_trusted_loopback_request(
    state: &AppState,
    headers: &HeaderMap,
    peer_ip: Option<IpAddr>,
) -> bool {
    if state.config.public_base_url.is_some() {
        return false;
    }
    if has_proxy_or_tunnel_headers(headers) {
        return false;
    }
    if let Some(ip) = peer_ip {
        if !ip.is_loopback() {
            return false;
        }
    }
    is_loopback_host(headers)
}

fn is_trusted_local_admin_header(state: &AppState, headers: &HeaderMap) -> bool {
    state.config.public_base_url.is_none()
        && !has_proxy_or_tunnel_headers(headers)
        && is_loopback_host(headers)
        && headers
            .get(LOCAL_PC_TRUSTED_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "1")
            .unwrap_or(false)
}

/// Middleware che hoàn toàn Web Chính & `/admin` khi request đi qua Cloudflare Worker (`X-DealX-Cloak: 1`),
/// đi qua Cloudflare Tunnel / Proxy (`cf-ray` / `cf-connecting-ip` trên PC), hoặc truy cập bằng tên miền bọc link riêng (`shortlink_base_url` / `origin.<shortlink>`).
async fn shortlink_cloak_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    // Luôn xóa header nội bộ trước để chống client bên ngoài giả mạo
    req.headers_mut().remove(LOCAL_PC_TRUSTED_HEADER);
    let peer_ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip());
    let local_pc_trusted = is_local_pc_trusted_loopback_request(&state, req.headers(), peer_ip);

    let headers = req.headers();
    let has_cloak_header = headers
        .get("x-dealx-cloak")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim() == "1")
        .unwrap_or(false);
    let has_cf_proxy_header = state.config.public_base_url.is_none()
        && (headers.contains_key("cf-ray") || headers.contains_key("cf-connecting-ip"));

    let is_cloaked_domain = if has_cloak_header || has_cf_proxy_header {
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
                let tunnel_snap = crate::services::tunnel::snapshot_tunnel_state();
                let short_host = crate::config::resolve_pc_runtime_shortlink_base_url(
                    db_short_raw.as_deref(),
                    state.config.shortlink_base_url.as_deref(),
                    None,
                    tunnel_snap.quick_tunnel_url.as_deref(),
                    tunnel_snap.has_named_token,
                )
                .as_deref()
                .and_then(extract_host_only);
                match short_host {
                    Some(sh) => {
                        let origin_sub = format!("origin.{}", sh);
                        (sh == rh || rh == origin_sub || rh.ends_with(".trycloudflare.com"))
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

    if local_pc_trusted && !is_cloaked_domain {
        req.headers_mut().insert(
            HeaderName::from_static(LOCAL_PC_TRUSTED_HEADER),
            HeaderValue::from_static("1"),
        );
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

            let aff_badge_text = if owned_aff.shortlink_base.is_some() {
                "Đã gắn link bọc ưu đãi"
            } else {
                "Đã gắn link ưu đãi trực tiếp"
            };

            let result_html = format!(
                r#"<div class="convert-result" aria-live="polite">
                    <div class="convert-result__head">
                        <span class="badge badge--platform">{}</span>
                        <span class="badge badge--accent">{}</span>
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
                aff_badge_text,
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

const BOT_LOGO_WEBP: &[u8] = include_bytes!("../../assets/bot-logo.webp");
const BOT_CART_3D_WEBP: &[u8] = include_bytes!("../../assets/bot-cart-3d.webp");

fn serve_static_asset(body: &'static [u8], content_type: &'static str, max_age: u32) -> Response {
    let mut resp = (StatusCode::OK, body).into_response();
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(cache_val) = HeaderValue::from_str(&format!("public, max-age={}", max_age)) {
        resp.headers_mut().insert(header::CACHE_CONTROL, cache_val);
    }
    resp
}

/// Logo chính thức của Bot Thế Giới Siêu Sale (WebP tối ưu dung lượng)
async fn handle_bot_logo_webp() -> Response {
    serve_static_asset(BOT_LOGO_WEBP, "image/webp", 86400)
}

/// Ảnh minh họa 3D Bot đẩy xe hàng chứa kho mã giảm giá (WebP)
async fn handle_bot_cart_3d_webp() -> Response {
    serve_static_asset(BOT_CART_3D_WEBP, "image/webp", 86400)
}

/// Script ngoài tạo hiệu ứng xoay 3D & ánh sáng phản chiếu theo chuột cho Trang Đích Bot (CSP `script-src 'self'`)
async fn handle_landing_js() -> Response {
    let js = r#"(function () {
    if (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    var stage = document.getElementById("stage3d");
    if (!stage) return;
    stage.addEventListener("pointermove", function (e) {
        if (e.pointerType === "touch") return;
        var r = stage.getBoundingClientRect();
        if (!r.width || !r.height) return;
        var px = (e.clientX - r.left) / r.width;
        var py = (e.clientY - r.top) / r.height;
        stage.classList.add("is-interacting");
        stage.style.setProperty("--rx", ((0.5 - py) * 12).toFixed(2) + "deg");
        stage.style.setProperty("--ry", ((px - 0.5) * 14).toFixed(2) + "deg");
        stage.style.setProperty("--gx", (px * 100).toFixed(1) + "%");
        stage.style.setProperty("--gy", (py * 100).toFixed(1) + "%");
    });
    stage.addEventListener("pointerleave", function () {
        stage.classList.remove("is-interacting");
        stage.style.setProperty("--rx", "0deg");
        stage.style.setProperty("--ry", "0deg");
        stage.style.setProperty("--gx", "50%");
        stage.style.setProperty("--gy", "45%");
    });
})();"#;
    serve_static_asset(js.as_bytes(), "application/javascript; charset=utf-8", 3600)
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
    let shortlink_db = match conn_opt.as_ref() {
        Some(conn) => Repository::get_setting(conn, "shortlink_base_url")
            .await
            .ok()
            .flatten(),
        None => None,
    };
    let tunnel_snap = crate::services::tunnel::snapshot_tunnel_state();
    let shortlink_base = crate::config::resolve_pc_runtime_shortlink_base_url(
        shortlink_db.as_deref(),
        state.config.shortlink_base_url.as_deref(),
        state.config.public_base_url.as_deref(),
        tunnel_snap.quick_tunnel_url.as_deref(),
        tunnel_snap.has_named_token,
    );
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
        .map(|v| v.trim() == "1")
        .unwrap_or(false);

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

/// Kiểm tra xem yêu cầu hiện tại có quyền Admin hay không (từ phiên hợp lệ hoặc truy cập trực tiếp localhost trên PC)
async fn is_authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    if is_trusted_local_admin_header(state, headers) {
        return true;
    }
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
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Plus+Jakarta+Sans:wght@400;500;600;700;800&display=swap" rel="stylesheet">
    <style>
        *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            background-color: #070b14;
            background-image:
                radial-gradient(circle at 50% 0%, rgba(249, 115, 22, 0.16), transparent 55%),
                radial-gradient(circle at 10% 90%, rgba(56, 189, 248, 0.08), transparent 45%);
            font-family: 'Plus Jakarta Sans', -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            color: #f8fafc;
            min-height: 100vh;
            display: flex;
            align-items: center;
            justify-content: center;
            padding: 1.25rem;
        }
        .login-card {
            background: #0f172a;
            border: 1px solid #1e293b;
            box-shadow: 0 24px 48px -12px rgba(0, 0, 0, 0.65);
            border-radius: 1.35rem;
            width: 100%;
            max-width: 27rem;
            padding: 2.25rem;
        }
        .login-head { text-align: center; margin-bottom: 1.75rem; }
        .login-logo {
            width: 80px; height: 80px; border-radius: 9999px; object-fit: cover; display: block;
            margin: 0 auto 1rem; border: 2px solid #f97316;
            box-shadow: 0 0 0 4px rgba(249, 115, 22, 0.18), 0 12px 24px rgba(0, 0, 0, 0.45);
        }
        .login-title {
            font-size: 1.5rem; font-weight: 800; letter-spacing: -0.02em;
            background: linear-gradient(90deg, #fb923c, #fcd34d);
            -webkit-background-clip: text; background-clip: text; color: transparent;
        }
        .login-sub { font-size: 0.8rem; color: #94a3b8; margin-top: 0.35rem; font-weight: 500; }
        .login-form { display: flex; flex-direction: column; gap: 1.15rem; }
        .field-label {
            display: block; font-size: 0.72rem; font-weight: 700; text-transform: uppercase;
            letter-spacing: 0.06em; color: #94a3b8; margin-bottom: 0.45rem;
        }
        .field-input {
            width: 100%; padding: 0.78rem 1rem; background: #162033; border: 1px solid #293548;
            border-radius: 0.75rem; color: #f8fafc; font-size: 0.9rem; font-family: inherit;
            outline: none; transition: border-color 0.15s, box-shadow 0.15s;
        }
        .field-input::placeholder { color: #64748b; }
        .field-input:focus { border-color: #f97316; box-shadow: 0 0 0 3px rgba(249, 115, 22, 0.2); }
        .login-btn {
            width: 100%; padding: 0.85rem 1rem; margin-top: 0.35rem;
            background: linear-gradient(135deg, #f59e0b 0%, #f97316 100%);
            border: none; border-radius: 0.75rem; color: #070b14; font-weight: 800;
            font-size: 0.9rem; font-family: inherit; cursor: pointer;
            box-shadow: 0 8px 20px rgba(249, 115, 22, 0.28); transition: transform 0.12s, filter 0.15s;
        }
        .login-btn:hover { filter: brightness(1.06); }
        .login-btn:active { transform: scale(0.98); }
        .login-foot {
            margin-top: 1.75rem; padding-top: 1.25rem; border-top: 1px solid #1e293b;
            text-align: center; font-size: 0.75rem; color: #64748b;
        }
    </style>
</head>
<body>
    <div class="login-card">
        <div class="login-head">
            <img src="/assets/bot-logo.webp" alt="Logo Thế Giới Siêu Sale" class="login-logo">
            <h1 class="login-title">THẾ GIỚI SIÊU SALE</h1>
            <p class="login-sub">Cổng Quản Trị Hệ Thống Săn Sale Đa Sàn</p>
        </div>

        <form method="POST" action="/admin/login" class="login-form">
            <div>
                <label class="field-label" for="admin-username">Tài khoản Quản Trị</label>
                <input id="admin-username" type="text" name="username" required autocomplete="username" class="field-input" placeholder="Nhập tên đăng nhập admin...">
            </div>

            <div>
                <label class="field-label" for="admin-password">Mật khẩu</label>
                <input id="admin-password" type="password" name="password" required autocomplete="current-password" class="field-input" placeholder="Nhập mật khẩu quản trị...">
            </div>

            <button type="submit" class="login-btn">Đăng Nhập Quản Trị</button>
        </form>

        <div class="login-foot">
            🛡️ Bảo mật 1 tài khoản Admin duy nhất · Đăng ký bị vô hiệu hoá
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

fn render_platform_status(label: &str, matched: &Option<(String, String)>, manual_id: &str, backup_note: Option<&str>) -> String {
    if !manual_id.trim().is_empty() {
        return format!(
            "<div class=\"plat-row\">\
                <div class=\"plat-row__left\">\
                    <div class=\"plat-row__name\">{}</div>\
                    <div class=\"plat-row__sub\">ID thủ công: <code>{}</code></div>\
                </div>\
                <span class=\"badge badge--ok\">🟢 Thủ công</span>\
             </div>",
            label,
            escape_html(manual_id.trim())
        );
    }
    match matched {
        Some((id, _)) => format!(
            "<div class=\"plat-row\">\
                <div class=\"plat-row__left\">\
                    <div class=\"plat-row__name\">{}</div>\
                    <div class=\"plat-row__sub\">Campaign: <code>{}</code></div>\
                </div>\
                <span class=\"badge badge--ok\">🟢 Đã duyệt</span>\
             </div>",
            label,
            escape_html(id)
        ),
        None => {
            if let Some(note) = backup_note {
                format!(
                    "<div class=\"plat-row\">\
                        <div class=\"plat-row__left\">\
                            <div class=\"plat-row__name\">{}</div>\
                            <div class=\"plat-row__sub\">{}</div>\
                        </div>\
                        <span class=\"badge badge--info\">🔵 Sẵn sàng</span>\
                     </div>",
                    label,
                    escape_html(note)
                )
            } else {
                format!(
                    "<div class=\"plat-row\">\
                        <div class=\"plat-row__left\">\
                            <div class=\"plat-row__name\">{}</div>\
                            <div class=\"plat-row__sub\">Chưa duyệt trên pub2.accesstrade.vn</div>\
                        </div>\
                        <span class=\"badge badge--warn\">🟡 Chưa duyệt AT</span>\
                     </div>",
                    label
                )
            }
        }
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
    let is_cloud_runtime = state.config.public_base_url.is_some();
    let auth_action_html = if is_trusted_local_admin_header(&state, &headers) {
        r#"<span class="badge badge--ok">🖥️ PC Local (Miễn đăng nhập)</span>"#.to_string()
    } else {
        r#"<a href="/admin/logout" class="btn btn--danger btn--sm">Đăng Xuất</a>"#.to_string()
    };
    let persistence_banner = if !is_cloud_runtime && !has_turso {
        format!(
            r#"<div class="notice-banner notice-banner--ok">
                <span>💻</span>
                <span><b>Đang chạy trực tiếp trên PC (Local):</b> Dữ liệu được lưu bền vững trên máy tính tại <code>{}</code> · Bot chạy chế độ <b>Long Polling</b> và gọi API trực tiếp từ IP của bạn.</span>
            </div>"#,
            escape_html(&state.config.database_path)
        )
    } else if !has_env_token || !has_turso {
        r#"<div class="notice-banner notice-banner--warn">
            <span>⚠️</span>
            <span><b>Lưu ý lưu trữ:</b> Đang dùng ổ đĩa tạm thời trên Cloud. Hãy thêm <code>TURSO_DATABASE_URL</code> &amp; <code>ACCESSTRADE_API_TOKEN</code> vào Environment để lưu dữ liệu vĩnh viễn.</span>
        </div>"#.to_string()
    } else {
        r#"<div class="notice-banner notice-banner--ok">
            <span>🟢</span>
            <span><b>Hệ thống lưu trữ bền vững:</b> Đã kết nối Turso Cloud Database &amp; cố định API Token an toàn.</span>
        </div>"#.to_string()
    };

    let notice_banner = match query.notice.as_deref() {
        Some(n) if !n.trim().is_empty() => format!(
            r#"<div class="notice-banner notice-banner--info"><span>ℹ️</span><span>{}</span></div>"#,
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
    let shortlink_db_raw = Repository::get_setting(&conn, "shortlink_base_url")
        .await
        .ok()
        .flatten();
    let db_cf_token = Repository::get_setting(&conn, "cloudflare_tunnel_token")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty());
    let env_cf_token = std::env::var("CLOUDFLARE_TUNNEL_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let has_cf_tunnel_token = db_cf_token.is_some() || env_cf_token.is_some();
    let tunnel_snap = crate::services::tunnel::snapshot_tunnel_state();
    let shortlink_base_url = crate::config::resolve_pc_runtime_shortlink_base_url(
        shortlink_db_raw.as_deref(),
        state.config.shortlink_base_url.as_deref(),
        None,
        tunnel_snap.quick_tunnel_url.as_deref(),
        tunnel_snap.has_named_token,
    )
    .unwrap_or_default();
    let shortlink_mobile_only = Repository::get_setting(&conn, "shortlink_mobile_only")
        .await
        .ok()
        .flatten()
        .map(|s| s.trim() == "1")
        .unwrap_or(false);
    let public_home_cloak = Repository::get_setting(&conn, "public_home_cloak")
        .await
        .ok()
        .flatten()
        .map(|s| s.trim() == "1")
        .unwrap_or(false);
    let derived_tunnel_origin = extract_host_only(&shortlink_base_url)
        .map(|h| format!("https://origin.{}", h))
        .unwrap_or_else(|| "https://origin.thegioisieusale.click".to_string());
    let origin_base_for_worker = state
        .config
        .public_base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&derived_tunnel_origin);

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
        has_cf_tunnel_token,
        tunnel_snap.quick_tunnel_url.as_deref(),
        tunnel_snap.is_running,
    );
    let order_alerts_card_html = render_order_alerts_card(&admin_telegram_id_val, &tracked_orders);

    let mut approved_opt: Option<ApprovedCampaigns> = None;
    let (at_status_badge, campaign_status_html) = if let Some(ref token) = db_at_token {
        let masked = mask_token(token);
        match state.accesstrade.get_approved_campaigns(token, false).await {
            Ok(approved) => {
                let badge = format!(
                    "<span class=\"badge badge--ok\">🟢 Đã kết nối ({})</span>",
                    masked
                );
                let dyn_list = approved.dynamic_campaigns();
                let dyn_line = if dyn_list.is_empty() {
                    r#"<div class="inset-note">✨ <b>Tự động nhận diện đa sàn:</b> Sẵn sàng — đăng ký thêm <i>Shein, CellphoneS, FPT Shop, Con Cưng, Fado, Klook...</i> trên <code>pub2.accesstrade.vn</code> sẽ tự kích hoạt không cần sửa code.</div>"#.to_string()
                } else {
                    let labels: Vec<String> = dyn_list
                        .iter()
                        .take(8)
                        .map(|c| format!("{} (<code>{}</code>)", escape_html(&c.platform_group), escape_html(&c.domain)))
                        .collect();
                    format!(
                        "<div class=\"inset-note inset-note--ok\">✨ <b>Đã tự mở thêm {} sàn/hãng mở rộng:</b> {}</div>",
                        dyn_list.len(),
                        labels.join(", ")
                    )
                };
                let has_shopee_direct = !shopee_affiliate_id.trim().is_empty() || state.config.shopee_affiliate_id.is_some();
                let shopee_backup_note = if has_shopee_direct {
                    Some("Shopee Chính Chủ (100% Hoạt Động)")
                } else {
                    None
                };
                let rows = format!(
                    "<div class=\"plat-list\">{}{}{}{}{}</div>{}",
                    render_platform_status("🟠 Shopee", &approved.shopee, &camp_shopee, shopee_backup_note),
                    render_platform_status("🔵 Lazada", &approved.lazada, &camp_lazada, Some("Deeplink Template Sẵn Sàng")),
                    render_platform_status("🔹 Tiki", &approved.tiki, &camp_tiki, None),
                    render_platform_status("🎵 TikTok Shop", &approved.tiktok, &camp_tiktok, None),
                    render_platform_status("🌐 Alibaba", &approved.alibaba, "", None),
                    dyn_line,
                );
                approved_opt = Some(approved);
                (badge, rows)
            }
            Err(err) => {
                let badge = format!(
                    "<span class=\"badge badge--err\">🔴 Lỗi kết nối ({})</span>",
                    masked
                );
                let err_box = format!("<p class=\"err-text\">{}</p>", escape_html(&err));
                (badge, err_box)
            }
        }
    } else {
        (
            "<span class=\"badge badge--muted\">⚪ Chưa nhập API Key</span>".to_string(),
            "<p class=\"muted-text\">Vui lòng nhập <b>ACCESSTRADE API Access Key</b> bên dưới hoặc thiết lập <code>ACCESSTRADE_API_TOKEN</code> trong file <code>.env</code>.</p>".to_string(),
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
            r#"<tr>
                <td><span class="badge badge--orange">{}</span></td>
                <td class="cell-strong">{}</td>
                <td><code class="code-pill">{}</code></td>
                <td><div class="cell-strong">{}</div><div class="cell-sub">{}</div></td>
                <td class="cell-muted">{}</td>
                <td><a href="{}" target="_blank" rel="noopener noreferrer" class="table-link">Mở Link Chuẩn ↗</a></td>
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
        let empty_hint = if db_at_token.is_none() {
            r#"Chưa cấu hình ACCESSTRADE API Access Key. Vào tab <b>"Kết Nối Sàn &amp; API"</b> để nhập Token hoặc nạp mã vào mục <b>"Kho Mã Độc Quyền Hợp Tác"</b> bên dưới."#.to_string()
        } else if approved_opt.is_none() {
            r#"⚠️ <b>ACCESSTRADE API đang bị lỗi kết nối (HTTP 403 Forbidden / Sai Key):</b> Bot chưa được cấp quyền kéo mã tự động từ sàn về.<br><span style="color:#fcd34d;">👉 Cách khắc phục:</span> Vào <a href="https://pub2.accesstrade.vn" target="_blank" rel="noopener noreferrer" class="table-link">pub2.accesstrade.vn</a> &gt; <b>Công cụ &gt; API</b> lấy lại Access Key mới rồi dán vào tab <b>"Kết Nối Sàn &amp; API"</b>."#.to_string()
        } else {
            r#"Không có mã nào khớp bộ lọc (hoặc đang nạp chu kỳ đầu tiên). Bấm nút <b>Quét Sâu Toàn Bộ Ngay</b> nếu cần làm mới ngay."#.to_string()
        };
        voucher_rows = format!(r#"<tr><td colspan="6" class="empty-cell">{}</td></tr>"#, empty_hint);
    }

    // Bảng danh sách Mã Độc Quyền Hợp Tác & Deals trong DB
    let mut exclusive_rows = String::new();
    for d in &saved_deals {
        let kind_badge = if d.is_exclusive {
            r#"<span class="badge badge--purple">👑 Độc Quyền</span>"#
        } else {
            r#"<span class="badge badge--muted">Thường</span>"#
        };
        let status_badge = if d.is_active {
            r#"<span class="badge badge--ok">🟢 Đang bật</span>"#
        } else {
            r#"<span class="badge badge--muted">⚫ Đang tắt</span>"#
        };
        let code_display = d.coupon_code.as_deref().unwrap_or("-");
        let min_note = if d.min_spend > 0.0 {
            format!(" · Đơn từ {}", escape_html(&format_vnd(d.min_spend)))
        } else {
            String::new()
        };
        exclusive_rows.push_str(&format!(
            r#"<tr>
                <td>{}</td>
                <td class="cell-strong">{}</td>
                <td><div class="cell-strong">{}</div><div class="cell-sub">{}{}</div></td>
                <td><code class="code-pill">{}</code></td>
                <td>{}</td>
                <td>
                    <div class="row-actions">
                        <form method="POST" action="/admin/deals/toggle">
                            <input type="hidden" name="deal_id" value="{}">
                            <button type="submit" class="btn btn--secondary btn--sm">Bật/Tắt</button>
                        </form>
                        <form method="POST" action="/admin/deals/delete">
                            <input type="hidden" name="deal_id" value="{}">
                            <button type="submit" class="btn btn--danger btn--sm">Xoá</button>
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
        exclusive_rows = r#"<tr><td colspan="6" class="empty-cell">Chưa có Mã Độc Quyền Hợp Tác nào. Hãy dán danh sách mã từ AM ACCESSTRADE / MCN / Nhãn hàng vào ô bên trên.</td></tr>"#.to_string();
    }

    // Render danh sách clicks gần đây (có escape chống XSS)
    let mut clicks_rows = String::new();
    for c in &clicks {
        let plat_badge = match c.platform.as_str() {
            "Shopee" => r#"<span class="badge badge--orange">Shopee</span>"#,
            "TikTok Shop" | "TikTok" => r#"<span class="badge badge--pink">TikTok Shop</span>"#,
            "Lazada" => r#"<span class="badge badge--info">Lazada</span>"#,
            "Tiki" => r#"<span class="badge badge--cyan">Tiki</span>"#,
            _ => r#"<span class="badge badge--muted">Khác</span>"#,
        };
        clicks_rows.push_str(&format!(
            r#"<tr>
                <td class="font-mono cell-muted">{}</td>
                <td>{}</td>
                <td><div class="truncate-url" title="{}">{}</div></td>
                <td><a href="{}" target="_blank" rel="noopener noreferrer" class="table-link table-link--amber">Mở link săn sale ↗</a></td>
                <td class="font-mono cell-muted">{}</td>
            </tr>"#,
            c.user_id,
            plat_badge,
            escape_html(&c.clean_url),
            escape_html(&c.clean_url),
            escape_html(&c.affiliate_url),
            escape_html(&c.created_at)
        ));
    }

    if clicks_rows.is_empty() {
        clicks_rows = r#"<tr><td colspan="5" class="empty-cell">Chưa có lượt dán link nào từ khách hàng. Dán link sản phẩm vào Bot Telegram @vvipass_vip_bot để kiểm tra.</td></tr>"#.to_string();
    }

    let bot_mode_badge = if state.webhook_tx.is_some() {
        r#"<span class="mode-tag mode-tag--ok">🟢 Webhook 24/7</span>"#
    } else {
        r#"<span class="mode-tag mode-tag--info">🔵 Polling PC</span>"#
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
    <title>Thế Giới Siêu Sale - Trung Tâm Quản Trị Hệ Thống</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Plus+Jakarta+Sans:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">
    <style>
        :root {{
            --bg-base: #070b14;
            --bg-surface: #0f172a;
            --bg-elevated: #111c33;
            --bg-inset: #0a101f;
            --bg-input: #16223a;
            --border-subtle: rgba(148, 163, 184, 0.14);
            --border-strong: rgba(148, 163, 184, 0.28);
            --text-primary: #f8fafc;
            --text-secondary: #cbd5e1;
            --text-muted: #94a3b8;
            --brand-primary: #f97316;
            --brand-accent: #f59e0b;
            --ok-text: #86efac;
            --ok-bg: rgba(34, 197, 94, 0.14);
            --ok-border: rgba(34, 197, 94, 0.34);
            --warn-text: #fde68a;
            --warn-bg: rgba(245, 158, 11, 0.14);
            --warn-border: rgba(245, 158, 11, 0.34);
            --err-text: #fca5a5;
            --err-bg: rgba(239, 68, 68, 0.14);
            --err-border: rgba(239, 68, 68, 0.34);
            --info-text: #7dd3fc;
            --info-bg: rgba(14, 165, 233, 0.14);
            --info-border: rgba(14, 165, 233, 0.34);
            --purple-text: #d8b4fe;
            --purple-bg: rgba(168, 85, 247, 0.16);
            --purple-border: rgba(168, 85, 247, 0.35);
            --pink-text: #f9a8d4;
            --pink-bg: rgba(236, 72, 153, 0.15);
            --pink-border: rgba(236, 72, 153, 0.34);
            --radius-sm: 0.5rem;
            --radius-md: 0.75rem;
            --radius-lg: 1rem;
            --shadow-card: 0 10px 30px -10px rgba(0, 0, 0, 0.55);
        }}
        *, *::before, *::after {{ box-sizing: border-box; }}
        body {{
            margin: 0;
            padding: 0;
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            background-color: var(--bg-base);
            background-image:
                radial-gradient(at 0% 0%, rgba(249, 115, 22, 0.11) 0px, transparent 52%),
                radial-gradient(at 100% 0%, rgba(14, 165, 233, 0.08) 0px, transparent 50%),
                radial-gradient(at 50% 100%, rgba(168, 85, 247, 0.06) 0px, transparent 52%);
            color: var(--text-primary);
            font-family: 'Plus Jakarta Sans', -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            -webkit-font-smoothing: antialiased;
        }}
        a {{ color: inherit; text-decoration: none; }}
        button, input, select, textarea {{ font-family: inherit; }}
        code {{
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.75rem;
            background: rgba(148, 163, 184, 0.12);
            color: #fcd34d;
            padding: 0.1rem 0.38rem;
            border-radius: 0.32rem;
        }}

        /* Header */
        .admin-header {{
            position: sticky;
            top: 0;
            z-index: 50;
            background: rgba(7, 11, 20, 0.9);
            backdrop-filter: blur(18px);
            -webkit-backdrop-filter: blur(18px);
            border-bottom: 1px solid var(--border-subtle);
        }}
        .admin-header__inner {{
            max-width: 1280px;
            margin: 0 auto;
            padding: 0.85rem 1.5rem;
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 1rem;
            flex-wrap: wrap;
        }}
        .brand {{
            display: flex;
            align-items: center;
            gap: 0.875rem;
            min-width: 0;
        }}
        .brand__logo-wrap {{
            position: relative;
            width: 44px;
            height: 44px;
            flex-shrink: 0;
        }}
        .brand__logo {{
            width: 44px;
            height: 44px;
            border-radius: 9999px;
            object-fit: cover;
            display: block;
            border: 2px solid var(--brand-primary);
            box-shadow: 0 0 0 3px rgba(249, 115, 22, 0.18);
        }}
        .brand__dot {{
            position: absolute;
            bottom: 0;
            right: 0;
            width: 11px;
            height: 11px;
            border-radius: 9999px;
            background: #22c55e;
            border: 2px solid var(--bg-base);
        }}
        .brand__title-row {{
            display: flex;
            align-items: center;
            gap: 0.5rem;
            flex-wrap: wrap;
        }}
        .brand__title {{
            margin: 0;
            font-size: 1.06rem;
            font-weight: 800;
            letter-spacing: -0.02em;
            background: linear-gradient(90deg, #fb923c, #fcd34d);
            -webkit-background-clip: text;
            background-clip: text;
            color: transparent;
        }}
        .brand__tag {{
            padding: 0.15rem 0.48rem;
            border-radius: 0.375rem;
            font-size: 0.65rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.06em;
            color: #fdba74;
            background: rgba(249, 115, 22, 0.15);
            border: 1px solid rgba(249, 115, 22, 0.35);
        }}
        .brand__sub {{
            margin: 0.14rem 0 0;
            font-size: 0.72rem;
            color: var(--text-muted);
        }}
        .header-actions {{
            display: flex;
            align-items: center;
            gap: 0.65rem;
            flex-wrap: wrap;
        }}
        .status-pill {{
            display: inline-flex;
            align-items: center;
            gap: 0.55rem;
            padding: 0.42rem 0.85rem;
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            font-size: 0.75rem;
            color: var(--text-muted);
        }}
        .status-pill strong {{ color: var(--text-primary); }}
        .status-pill a {{ color: #fbbf24; font-weight: 600; }}
        .status-pill a:hover {{ text-decoration: underline; }}
        .status-sep {{ color: rgba(148, 163, 184, 0.3); }}
        .pulse-dot {{
            width: 8px;
            height: 8px;
            border-radius: 9999px;
            background: #22c55e;
            display: inline-block;
            flex-shrink: 0;
            box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.25);
        }}
        .mode-tag {{ font-weight: 600; font-size: 0.74rem; }}
        .mode-tag--ok {{ color: var(--ok-text); }}
        .mode-tag--info {{ color: var(--info-text); }}

        /* Tabs */
        .tab-bar {{
            position: sticky;
            top: 71px;
            z-index: 40;
            background: rgba(9, 14, 26, 0.88);
            backdrop-filter: blur(14px);
            -webkit-backdrop-filter: blur(14px);
            border-bottom: 1px solid var(--border-subtle);
        }}
        .tab-nav {{
            max-width: 1280px;
            margin: 0 auto;
            padding: 0.65rem 1.5rem;
            display: flex;
            align-items: center;
            gap: 0.5rem;
            overflow-x: auto;
        }}
        .tab-btn {{
            display: inline-flex;
            align-items: center;
            gap: 0.45rem;
            padding: 0.55rem 1rem;
            border-radius: var(--radius-md);
            font-size: 0.84rem;
            font-weight: 600;
            color: var(--text-muted);
            background: transparent;
            border: 1px solid transparent;
            cursor: pointer;
            white-space: nowrap;
            transition: all 0.15s ease;
        }}
        .tab-btn:hover {{
            color: var(--text-primary);
            background: rgba(30, 41, 59, 0.65);
            border-color: var(--border-subtle);
        }}
        .tab-btn.active {{
            color: #070b14;
            background: linear-gradient(135deg, #f59e0b 0%, #f97316 100%);
            border-color: #fb923c;
            font-weight: 700;
            box-shadow: 0 4px 14px rgba(249, 115, 22, 0.32);
        }}
        .tab-pane {{
            display: flex;
            flex-direction: column;
            gap: 1.5rem;
        }}
        .tab-pane.is-hidden {{ display: none !important; }}

        /* Main & Footer */
        .admin-main {{
            width: 100%;
            max-width: 1280px;
            margin: 0 auto;
            padding: 1.75rem 1.5rem 2.5rem;
            flex: 1;
            display: flex;
            flex-direction: column;
            gap: 1.5rem;
        }}
        .admin-footer {{
            width: 100%;
            max-width: 1280px;
            margin: 0 auto;
            padding: 1.25rem 1.5rem 2rem;
            text-align: center;
            font-size: 0.75rem;
            color: var(--text-muted);
            border-top: 1px solid var(--border-subtle);
        }}

        /* Notice Banners */
        .notice-banner {{
            padding: 0.85rem 1.15rem;
            border-radius: var(--radius-md);
            font-size: 0.84rem;
            font-weight: 500;
            line-height: 1.5;
            border: 1px solid var(--border-subtle);
        }}
        .notice-banner--ok {{
            background: var(--ok-bg);
            border-color: var(--ok-border);
            color: var(--ok-text);
        }}
        .notice-banner--warn {{
            background: var(--warn-bg);
            border-color: var(--warn-border);
            color: var(--warn-text);
        }}
        .notice-banner--info {{
            background: var(--info-bg);
            border-color: var(--info-border);
            color: var(--info-text);
        }}

        /* Cards */
        .card {{
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-lg);
            padding: 1.5rem;
            box-shadow: var(--shadow-card);
            display: flex;
            flex-direction: column;
            gap: 1.25rem;
        }}
        .card--table {{
            padding: 0;
            gap: 0;
            overflow: hidden;
        }}
        .card--orange {{ border-color: rgba(249, 115, 22, 0.34); }}
        .card--green {{ border-color: rgba(34, 197, 94, 0.34); }}
        .card--purple {{ border-color: rgba(168, 85, 247, 0.35); }}
        .card--social {{
            background: linear-gradient(120deg, rgba(88, 28, 135, 0.3), rgba(131, 24, 67, 0.24), rgba(154, 52, 18, 0.22));
            border-color: rgba(168, 85, 247, 0.34);
            flex-direction: row;
            flex-wrap: wrap;
            align-items: center;
            justify-content: space-between;
            gap: 1rem;
        }}
        .social-left {{
            display: flex;
            align-items: center;
            gap: 0.9rem;
            flex: 1 1 380px;
        }}
        .social-icon {{
            width: 48px;
            height: 48px;
            border-radius: 0.9rem;
            background: linear-gradient(135deg, #9333ea, #ec4899, #f97316);
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 1.45rem;
            flex-shrink: 0;
            box-shadow: 0 8px 20px rgba(168, 85, 247, 0.3);
        }}
        .social-hint {{
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 0.55rem 0.85rem;
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.74rem;
            color: var(--text-secondary);
        }}
        .card-head {{
            display: flex;
            flex-wrap: wrap;
            align-items: flex-start;
            justify-content: space-between;
            gap: 0.75rem;
            padding-bottom: 1rem;
            border-bottom: 1px solid var(--border-subtle);
        }}
        .card-head--table {{
            padding: 1.25rem 1.5rem;
            margin: 0;
            background: rgba(10, 16, 31, 0.55);
        }}
        .card-head-actions {{
            display: flex;
            align-items: center;
            gap: 0.55rem;
            flex-wrap: wrap;
        }}
        .card-title {{
            margin: 0;
            font-size: 1.06rem;
            font-weight: 700;
            color: var(--text-primary);
            display: flex;
            flex-wrap: wrap;
            align-items: center;
            gap: 0.5rem;
        }}
        .card-desc {{
            margin: 0.35rem 0 0;
            font-size: 0.78rem;
            color: var(--text-muted);
            line-height: 1.55;
        }}

        /* KPI Grid */
        .kpi-grid {{
            display: grid;
            grid-template-columns: repeat(4, minmax(0, 1fr));
            gap: 1.15rem;
        }}
        .kpi-card {{
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-lg);
            padding: 1.25rem;
            display: flex;
            flex-direction: column;
            justify-content: space-between;
            gap: 0.8rem;
            box-shadow: var(--shadow-card);
            transition: border-color 0.15s ease;
        }}
        .kpi-card:hover {{ border-color: var(--border-strong); }}
        .kpi-card--orange {{ border-color: rgba(249, 115, 22, 0.32); }}
        .kpi-card--green {{ border-color: rgba(34, 197, 94, 0.32); }}
        .kpi-card__top {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            font-size: 0.72rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--text-muted);
        }}
        .kpi-card__icon {{
            width: 2rem;
            height: 2rem;
            border-radius: 0.6rem;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            font-size: 0.92rem;
            background: rgba(148, 163, 184, 0.1);
            border: 1px solid var(--border-subtle);
        }}
        .kpi-card__val {{
            margin-top: 0.35rem;
            font-size: 1.9rem;
            font-weight: 800;
            line-height: 1.15;
            letter-spacing: -0.02em;
            color: var(--text-primary);
        }}
        .kpi-card__val--amber {{ color: #fbbf24; }}
        .kpi-card__val--orange {{ color: #fb923c; }}
        .kpi-card__val--green {{ color: #4ade80; font-size: 0.95rem; font-weight: 700; margin-top: 0.45rem; }}
        .kpi-card__meta {{
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 0.65rem 0.75rem;
            font-size: 0.73rem;
            color: var(--text-secondary);
            display: flex;
            flex-direction: column;
            gap: 0.3rem;
        }}
        .kpi-card__pill {{
            background: rgba(6, 182, 212, 0.12);
            border: 1px solid rgba(6, 182, 212, 0.28);
            color: #67e8f9;
            border-radius: var(--radius-sm);
            padding: 0.45rem 0.7rem;
            font-size: 0.73rem;
            font-weight: 500;
        }}
        .kpi-row {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            gap: 0.5rem;
            flex-wrap: wrap;
        }}
        .kpi-row strong {{ font-family: 'JetBrains Mono', monospace; color: var(--text-primary); }}

        /* Responsive Layout Grids */
        .split-at {{
            display: grid;
            grid-template-columns: minmax(340px, 1.08fr) minmax(0, 1.42fr);
            gap: 1.5rem;
            align-items: start;
        }}
        .grid-2 {{
            display: grid;
            grid-template-columns: repeat(2, minmax(0, 1fr));
            gap: 1.25rem;
        }}
        .grid-3 {{
            display: grid;
            grid-template-columns: repeat(3, minmax(0, 1fr));
            gap: 1rem;
        }}
        .form-grid-2 {{
            display: grid;
            grid-template-columns: repeat(2, minmax(0, 1fr));
            gap: 0.85rem;
        }}

        /* ACCESSTRADE Platform Status List */
        .plat-panel {{
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 1rem;
            display: flex;
            flex-direction: column;
            gap: 0.85rem;
        }}
        .plat-panel__title {{
            font-size: 0.72rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.06em;
            color: var(--text-muted);
        }}
        .plat-list {{
            display: flex;
            flex-direction: column;
            gap: 0.55rem;
        }}
        .plat-row {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 0.75rem;
            padding: 0.65rem 0.8rem;
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
        }}
        .plat-row__left {{
            display: flex;
            flex-direction: column;
            gap: 0.18rem;
            min-width: 0;
        }}
        .plat-row__name {{
            font-size: 0.82rem;
            font-weight: 600;
            color: var(--text-primary);
        }}
        .plat-row__sub {{
            font-size: 0.7rem;
            color: var(--text-muted);
            font-family: 'JetBrains Mono', monospace;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }}
        .inset-note {{
            background: rgba(15, 23, 42, 0.85);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-sm);
            padding: 0.75rem 0.85rem;
            font-size: 0.74rem;
            color: var(--text-muted);
            line-height: 1.55;
            display: flex;
            flex-direction: column;
            gap: 0.3rem;
        }}
        .inset-note strong, .inset-note b {{ color: var(--text-secondary); }}
        .inset-note--ok {{
            border-color: var(--ok-border);
            background: rgba(20, 83, 45, 0.22);
            color: var(--ok-text);
        }}
        .err-text {{ color: var(--err-text); font-size: 0.8rem; font-weight: 500; }}
        .muted-text {{ color: var(--text-muted); font-size: 0.75rem; margin-top: 0.3rem; }}

        /* Badges & Pills */
        .badge {{
            display: inline-flex;
            align-items: center;
            gap: 0.3rem;
            padding: 0.28rem 0.65rem;
            border-radius: 9999px;
            font-size: 0.72rem;
            font-weight: 600;
            border: 1px solid transparent;
            white-space: nowrap;
            line-height: 1.25;
            flex-shrink: 0;
        }}
        .badge--ok {{ background: var(--ok-bg); border-color: var(--ok-border); color: var(--ok-text); }}
        .badge--err {{ background: var(--err-bg); border-color: var(--err-border); color: var(--err-text); }}
        .badge--warn {{ background: var(--warn-bg); border-color: var(--warn-border); color: var(--warn-text); }}
        .badge--info {{ background: var(--info-bg); border-color: var(--info-border); color: var(--info-text); }}
        .badge--muted {{
            background: rgba(148, 163, 184, 0.12);
            border-color: rgba(148, 163, 184, 0.24);
            color: var(--text-secondary);
        }}
        .badge--orange {{
            background: rgba(249, 115, 22, 0.16);
            border-color: rgba(249, 115, 22, 0.36);
            color: #fdba74;
        }}
        .badge--purple {{ background: var(--purple-bg); border-color: var(--purple-border); color: var(--purple-text); }}
        .badge--pink {{ background: var(--pink-bg); border-color: var(--pink-border); color: var(--pink-text); }}
        .badge--cyan {{
            background: rgba(6, 182, 212, 0.15);
            border-color: rgba(6, 182, 212, 0.34);
            color: #67e8f9;
        }}
        .code-pill {{
            display: inline-block;
            padding: 0.22rem 0.55rem;
            border-radius: 0.45rem;
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.74rem;
            font-weight: 600;
            background: var(--bg-inset);
            color: #fcd34d;
            border: 1px solid var(--border-strong);
        }}

        /* Forms & Controls */
        .form-stack {{ display: flex; flex-direction: column; gap: 1rem; }}
        .subpanel {{
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 1.1rem;
            display: flex;
            flex-direction: column;
            gap: 0.85rem;
        }}
        .subpanel__title {{
            font-size: 0.75rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--text-secondary);
        }}
        .divider-top {{ border-top: 1px solid var(--border-subtle); padding-top: 0.9rem; }}
        .field-label {{
            display: block;
            font-size: 0.75rem;
            font-weight: 600;
            color: var(--text-secondary);
            margin-bottom: 0.35rem;
        }}
        input[type="text"],
        input[type="password"],
        input[type="url"],
        input[type="number"],
        select,
        textarea,
        .field-input,
        .code-textarea {{
            width: 100%;
            background: var(--bg-input);
            border: 1px solid var(--border-strong);
            border-radius: var(--radius-md);
            color: var(--text-primary);
            padding: 0.62rem 0.85rem;
            font-size: 0.82rem;
            outline: none;
            transition: border-color 0.15s ease, box-shadow 0.15s ease;
        }}
        .field-input--mono, .code-textarea {{
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.79rem;
        }}
        input::placeholder, textarea::placeholder {{ color: #64748b; }}
        input:focus, select:focus, textarea:focus {{
            border-color: var(--brand-accent);
            box-shadow: 0 0 0 3px rgba(245, 158, 11, 0.22);
        }}
        .check-row {{
            display: inline-flex;
            align-items: center;
            gap: 0.55rem;
            font-size: 0.8rem;
            color: var(--text-secondary);
            cursor: pointer;
        }}
        .check-row input[type="checkbox"] {{
            width: 1rem;
            height: 1rem;
            accent-color: var(--brand-primary);
            cursor: pointer;
        }}
        .filter-bar {{
            display: flex;
            flex-wrap: wrap;
            align-items: center;
            gap: 0.65rem;
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
            padding: 0.75rem 0.9rem;
            border-radius: var(--radius-md);
        }}
        .filter-bar select {{
            width: auto;
            min-width: 190px;
        }}
        .inline-form-grow {{
            display: flex;
            flex: 1 1 280px;
            align-items: center;
            gap: 0.65rem;
            flex-wrap: wrap;
        }}
        .grow-field {{
            flex: 1 1 220px;
            min-width: 180px;
            width: auto !important;
        }}

        /* Buttons */
        .btn {{
            display: inline-flex;
            align-items: center;
            justify-content: center;
            gap: 0.4rem;
            padding: 0.62rem 1.05rem;
            border-radius: var(--radius-md);
            font-size: 0.8rem;
            font-weight: 700;
            border: 1px solid transparent;
            cursor: pointer;
            text-decoration: none;
            white-space: nowrap;
            transition: all 0.15s ease;
        }}
        .btn--primary {{
            background: linear-gradient(135deg, #f59e0b 0%, #f97316 100%);
            color: #070b14;
            border-color: #fb923c;
            box-shadow: 0 6px 18px rgba(249, 115, 22, 0.26);
        }}
        .btn--primary:hover {{ filter: brightness(1.07); transform: translateY(-1px); }}
        .btn--secondary {{
            background: #1a2640;
            color: var(--text-primary);
            border-color: var(--border-strong);
        }}
        .btn--secondary:hover {{
            background: #233354;
            border-color: rgba(148, 163, 184, 0.45);
        }}
        .btn--success {{
            background: linear-gradient(135deg, #22c55e, #16a34a);
            color: #052e16;
            border-color: #4ade80;
        }}
        .btn--success:hover {{ filter: brightness(1.07); }}
        .btn--purple {{
            background: linear-gradient(135deg, #9333ea, #6366f1);
            color: #ffffff;
            border-color: #a855f7;
        }}
        .btn--purple:hover {{ filter: brightness(1.08); }}
        .btn--danger {{
            background: var(--err-bg);
            color: var(--err-text);
            border-color: var(--err-border);
        }}
        .btn--danger:hover {{ background: rgba(239, 68, 68, 0.25); }}
        .btn--orange-soft {{
            background: rgba(249, 115, 22, 0.16);
            color: #fdba74;
            border-color: rgba(249, 115, 22, 0.4);
        }}
        .btn--orange-soft:hover {{ background: rgba(249, 115, 22, 0.28); }}
        .btn--sm {{ padding: 0.38rem 0.75rem; font-size: 0.74rem; border-radius: var(--radius-sm); }}
        .btn--block {{ width: 100%; }}
        .row-actions {{ display: flex; align-items: center; gap: 0.45rem; flex-wrap: nowrap; }}

        /* Tables */
        .table-wrap {{
            overflow-x: auto;
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            background: rgba(10, 16, 31, 0.45);
        }}
        .card--table .table-wrap {{
            border: none;
            border-radius: 0;
            background: transparent;
        }}
        .data-table {{
            width: 100%;
            border-collapse: collapse;
            text-align: left;
            font-size: 0.8rem;
        }}
        .data-table thead {{
            background: var(--bg-inset);
            border-bottom: 1px solid var(--border-subtle);
        }}
        .data-table th {{
            padding: 0.78rem 0.95rem;
            font-size: 0.7rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--text-muted);
            white-space: nowrap;
        }}
        .data-table td {{
            padding: 0.78rem 0.95rem;
            border-bottom: 1px solid rgba(148, 163, 184, 0.09);
            vertical-align: middle;
        }}
        .data-table tbody tr:hover {{ background: rgba(30, 41, 59, 0.42); }}
        .cell-strong {{ font-weight: 600; color: var(--text-primary); }}
        .cell-sub {{ font-size: 0.72rem; color: var(--text-muted); margin-top: 0.2rem; }}
        .cell-muted {{ color: var(--text-muted); font-size: 0.75rem; }}
        .table-link {{ color: #7dd3fc; font-weight: 600; }}
        .table-link:hover {{ text-decoration: underline; }}
        .table-link--amber {{ color: #fbbf24; }}
        .truncate-url {{
            max-width: 240px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }}
        .empty-cell {{
            padding: 2rem 1rem !important;
            text-align: center;
            color: var(--text-muted);
            font-size: 0.82rem;
        }}

        /* Cloudflare Details */
        .cf-details {{
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 0.75rem 1rem;
        }}
        .cf-summary {{
            cursor: pointer;
            font-size: 0.8rem;
            font-weight: 600;
            color: #fdba74;
        }}
        .cf-body {{
            margin-top: 0.75rem;
            font-size: 0.76rem;
            color: var(--text-secondary);
            line-height: 1.55;
            display: flex;
            flex-direction: column;
            gap: 0.45rem;
        }}
        .cf-steps {{
            margin: 0.25rem 0 0 1.25rem;
            padding: 0;
            display: flex;
            flex-direction: column;
            gap: 0.35rem;
        }}

        /* Scrollbar & Responsive */
        ::-webkit-scrollbar {{ width: 6px; height: 6px; }}
        ::-webkit-scrollbar-track {{ background: var(--bg-base); }}
        ::-webkit-scrollbar-thumb {{ background: #334155; border-radius: 9999px; }}
        ::-webkit-scrollbar-thumb:hover {{ background: var(--brand-primary); }}

        @media (max-width: 1080px) {{
            .kpi-grid {{ grid-template-columns: repeat(2, minmax(0, 1fr)); }}
            .split-at, .grid-2, .grid-3 {{ grid-template-columns: minmax(0, 1fr); }}
        }}
        @media (max-width: 820px) {{
            .hide-sm {{ display: none !important; }}
            .tab-bar {{ top: 69px; }}
        }}
        @media (max-width: 600px) {{
            .kpi-grid, .form-grid-2 {{ grid-template-columns: minmax(0, 1fr); }}
            .admin-main {{ padding: 1.15rem 1rem 2rem; }}
            .admin-header__inner, .tab-nav {{ padding-left: 1rem; padding-right: 1rem; }}
        }}
    </style>
</head>
<body>
    <!-- Navbar Header -->
    <header class="admin-header">
        <div class="admin-header__inner">
            <div class="brand">
                <div class="brand__logo-wrap">
                    <img src="/assets/bot-logo.webp" alt="Logo Thế Giới Siêu Sale" class="brand__logo">
                    <span class="brand__dot"></span>
                </div>
                <div>
                    <div class="brand__title-row">
                        <h1 class="brand__title">THẾ GIỚI SIÊU SALE</h1>
                        <span class="brand__tag">Console</span>
                    </div>
                    <p class="brand__sub">Trung Tâm Điều Hành &amp; Quản Trị Hệ Thống Săn Sale Đa Sàn</p>
                </div>
            </div>

            <div class="header-actions">
                <a href="/" target="_blank" rel="noopener noreferrer" class="btn btn--secondary btn--sm hide-sm">
                    <span>🌐 Xem Trang Đích Bot 3D ↗</span>
                </a>
                <div class="status-pill hide-sm">
                    <span class="pulse-dot"></span>
                    <span>Admin: <strong>{}</strong></span>
                    <span class="status-sep">|</span>
                    <span>Bot: <a href="https://t.me/{}" target="_blank" rel="noopener noreferrer">@{}</a></span>
                    <span class="status-sep">|</span>
                    {}
                </div>
                {}
            </div>
        </div>
    </header>

    <!-- Sub-Navbar / Tab Navigation -->
    <div class="tab-bar">
        <div class="tab-nav">
            <button type="button" onclick="switchTab('overview')" id="tab-btn-overview" class="tab-btn active">
                <span>📊</span>
                <span>Tổng Quan</span>
            </button>
            <button type="button" onclick="switchTab('platforms')" id="tab-btn-platforms" class="tab-btn">
                <span>🔗</span>
                <span>Kết Nối Sàn &amp; API</span>
            </button>
            <button type="button" onclick="switchTab('vouchers')" id="tab-btn-vouchers" class="tab-btn">
                <span>🎟️</span>
                <span>Kho Voucher Thị Trường</span>
            </button>
            <button type="button" onclick="switchTab('orders')" id="tab-btn-orders" class="tab-btn">
                <span>🔔</span>
                <span>Báo Đơn &amp; Doanh Thu</span>
            </button>
        </div>
    </div>

    <!-- Main Content Panes -->
    <main class="admin-main">
        {}
        {}

        <!-- TAB 1: TỔNG QUAN (OVERVIEW) -->
        <div id="tab-pane-overview" class="tab-pane">
            <!-- 4 Thẻ KPI Thống Kê Hiện Đại -->
            <section class="kpi-grid">
                <div class="kpi-card">
                    <div>
                        <div class="kpi-card__top">
                            <span>Người Dùng Telegram</span>
                            <span class="kpi-card__icon">👥</span>
                        </div>
                        <div class="kpi-card__val">{}</div>
                    </div>
                    <div class="kpi-card__pill">✨ Khách thực tế đã tương tác bot</div>
                </div>

                <div class="kpi-card">
                    <div>
                        <div class="kpi-card__top">
                            <span>Tổng Lượt Dán Link</span>
                            <span class="kpi-card__icon">⚡</span>
                        </div>
                        <div class="kpi-card__val kpi-card__val--amber">{}</div>
                    </div>
                    <div class="kpi-card__meta">
                        <div class="kpi-row"><span>🟠 Shopee: <strong>{}</strong></span><span>🔵 Lazada: <strong>{}</strong></span></div>
                        <div class="kpi-row"><span>🎵 TikTok: <strong>{}</strong></span><span>🔹 Tiki: <strong>{}</strong></span></div>
                    </div>
                </div>

                <div class="kpi-card kpi-card--orange">
                    <div>
                        <div class="kpi-card__top">
                            <span>Kho Mã Thị Trường</span>
                            <span class="kpi-card__icon">🔥</span>
                        </div>
                        <div class="kpi-card__val kpi-card__val--orange">{}</div>
                    </div>
                    <div class="kpi-card__meta">
                        <div class="kpi-row"><span>🔥 Giảm ≥20%: <strong>{}</strong></span><span>💸 Tiền/Khác: <strong>{}</strong></span></div>
                        <div class="kpi-row"><span>🟠 Shopee: <strong>{}</strong></span><span>🔹 Tiki: <strong>{}</strong></span><span>👑 Độc quyền: <strong>{}</strong></span></div>
                    </div>
                </div>

                <div class="kpi-card kpi-card--green">
                    <div>
                        <div class="kpi-card__top">
                            <span>Tự Động Quét Nền</span>
                            <span class="kpi-card__icon">⏱️</span>
                        </div>
                        <div class="kpi-card__val--green">Chu kỳ 60 giây / lần</div>
                        <div class="muted-text">Lần cuối: <strong style="color: var(--text-primary); font-family: 'JetBrains Mono', monospace;">{}</strong></div>
                    </div>
                    <a href="/admin" class="btn btn--secondary btn--sm btn--block">🔄 Làm Mới Số Liệu Web</a>
                </div>
            </section>

            <!-- Widget Shopee Social Harvester -->
            <section class="card card--social">
                <div class="social-left">
                    <div class="social-icon">🤖</div>
                    <div>
                        <div class="card-title">
                            <span>Shopee Social Harvester — Tự Động Bắt Chữ Ký FB &amp; IG Reels</span>
                            <span class="badge badge--ok">🟢 Quét Ngầm 30 Phút/Lần</span>
                        </div>
                        <p class="card-desc">
                            Hệ thống tự động quét các nguồn deal lớn để trích xuất chữ ký số mạng xã hội (<code>sm_sig_id</code>, <code>channel_type=ig</code>) giúp 100% link Shopee do bot cấp cho khách mở khóa <b>Mã Giảm 20% - 30% độc quyền</b>!
                        </p>
                    </div>
                </div>
                <div class="social-hint">💡 Hoặc dán thẳng link FB/Insta vào bot Telegram @vvipass_vip_bot để nạp ngay!</div>
            </section>

            <!-- Lịch sử dán link gần nhất -->
            <section class="card card--table">
                <div class="card-head card-head--table">
                    <div>
                        <h2 class="card-title"><span>📋</span><span>Lịch Sử Khách Dán Link Săn Sale Gần Đây (50 lượt mới nhất)</span></h2>
                        <p class="card-desc">Theo dõi thời gian thực các liên kết khách gửi vào bot Telegram</p>
                    </div>
                </div>
                <div class="table-wrap">
                    <table class="data-table">
                        <thead>
                            <tr>
                                <th>Telegram ID</th>
                                <th>Sàn</th>
                                <th>Link Khách Gửi</th>
                                <th>Link Đầu Ra</th>
                                <th>Thời Gian</th>
                            </tr>
                        </thead>
                        <tbody>
                            {}
                        </tbody>
                    </table>
                </div>
            </section>
        </div>

        <!-- TAB 2: KẾT NỐI SÀN & API (PLATFORMS) -->
        <div id="tab-pane-platforms" class="tab-pane is-hidden">
            <!-- Kết nối ACCESSTRADE API Full 4 Sàn -->
            <section class="card">
                <div class="card-head">
                    <div>
                        <h2 class="card-title"><span>🚀</span><span>Kết Nối ACCESSTRADE API (Tự Động Đa Sàn)</span></h2>
                        <p class="card-desc">
                            Bot tự động gọi API <code>v1/product_link/create</code> &amp; TikTok v2 Universal Deeplink, đồng thời kiểm tra HTTP 302/200 chống lỗi link 404/500.
                        </p>
                    </div>
                    <div class="card-head-actions">
                        {}
                        <form method="POST" action="/admin/settings/accesstrade/sync" style="margin: 0;">
                            <button type="submit" class="btn btn--secondary btn--sm">🔄 Đồng Bộ Lại Chiến Dịch</button>
                        </form>
                    </div>
                </div>

                <div class="split-at">
                    <div class="plat-panel">
                        <div class="plat-panel__title">Trạng Thái Duyệt &amp; Chuẩn Hóa Link</div>
                        <div>{}</div>
                        <div class="inset-note">
                            <div><b>🛡️ Cơ chế bảo vệ Link Đầu Ra 100% Sống:</b></div>
                            <div>• <b>TikTok Shop:</b> Link v2 <code>tiktok.com/t/...</code> mở thẳng sản phẩm trên App.</div>
                            <div>• <b>Tiki &amp; Alibaba:</b> Tự dùng gói <b>TIKI CREATOR</b> &amp; <b>ALIBABA</b> đã duyệt + kiểm tra gateway <code>HTTP 200</code> trước khi gửi.</div>
                            <div>• <b>Shopee / Lazada:</b> Hỗ trợ song song Shopee Chính Chủ &amp; Deeplink dự phòng.</div>
                        </div>
                    </div>

                    <form method="POST" action="/admin/settings/accesstrade" class="form-stack">
                        <div>
                            <label class="field-label">ACCESSTRADE API Access Key (Token)</label>
                            <input type="password" name="api_token" placeholder="Dán API Access Key mới (hoặc để trống để giữ Token hiện tại)..." class="field-input field-input--mono">
                            <p class="muted-text">💡 Đã tích hợp sẵn API Token từ cấu hình hệ thống — tự động giữ kết nối khi khởi động lại.</p>
                        </div>

                        <div class="form-grid-2">
                            <div>
                                <label class="field-label">Campaign ID Shopee (Tùy chọn)</label>
                                <input type="text" name="campaign_shopee" value="{}" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                            </div>
                            <div>
                                <label class="field-label">Campaign ID Lazada (Tùy chọn)</label>
                                <input type="text" name="campaign_lazada" value="{}" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                            </div>
                            <div>
                                <label class="field-label">Campaign ID Tiki (Tùy chọn)</label>
                                <input type="text" name="campaign_tiki" value="{}" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                            </div>
                            <div>
                                <label class="field-label">Campaign ID TikTok Shop (Tùy chọn)</label>
                                <input type="text" name="campaign_tiktok" value="{}" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                            </div>
                        </div>

                        <div>
                            <button type="submit" class="btn btn--primary">Lưu &amp; Kích Hoạt ACCESSTRADE API</button>
                        </div>
                    </form>
                </div>
            </section>

            <!-- Card Shopee Chính Chủ & Bọc Link -->
            {}

            <!-- Cấu hình Link Tiếp Thị Dự Phòng (Deeplink Template) -->
            <section class="card">
                <div>
                    <h2 class="card-title"><span>🔗</span><span>Cấu Hình Deeplink Dự Phòng (Nếu Chưa Dùng API Token)</span></h2>
                    <p class="card-desc">
                        Nếu chiến dịch trên API chưa duyệt, hệ thống sẽ kiểm tra mẫu link dự phòng này. Nếu để trống, bot tự động trả về link sản phẩm sạch để khách mở App mua hàng không bị lỗi 404.
                    </p>
                </div>
                <form method="POST" action="/admin/settings/affiliate" class="form-stack">
                    <div>
                        <label class="field-label">Mẫu Deeplink Template</label>
                        <input type="text" name="template" value="{}" placeholder="Ví dụ: https://go.isclix.com/deep_link/PUB_ID/CAMPAIGN_ID?url={{CLEAN_URL}}&amp;utm_source=facebook&amp;sub1={{USER_ID}}" class="field-input field-input--mono">
                    </div>
                    <div>
                        <button type="submit" class="btn btn--secondary">Lưu Deeplink Dự Phòng</button>
                    </div>
                </form>
            </section>
        </div>

        <!-- TAB 3: KHO VOUCHER THỊ TRƯỜNG & ĐỘC QUYỀN (VOUCHERS) -->
        <div id="tab-pane-vouchers" class="tab-pane is-hidden">
            <!-- Kho Mã Giảm Giá Toàn Thị Trường -->
            <section class="card">
                <div class="card-head">
                    <div>
                        <h2 class="card-title">
                            <span>🔥</span>
                            <span>Kho Mã Giảm Giá Toàn Thị Trường (Cập Nhật Mỗi 60 Giây)</span>
                            <span class="badge badge--orange">{} mã đang sẵn ({} mã &gt;=20%)</span>
                        </h2>
                        <p class="card-desc">
                            Quét từ hệ thống ACCESSTRADE (mã %, giảm tiền K/VNĐ, mã hot, coupon Tiki/Shopee/Lazada/TikTok). Lần quét gần nhất: <strong style="color: var(--ok-text);">{}</strong>.
                        </p>
                    </div>
                    <form method="POST" action="/admin/vouchers/sync" style="margin: 0;">
                        <button type="submit" class="btn btn--orange-soft">🔄 Quét Sâu Toàn Bộ Ngay</button>
                    </form>
                </div>

                <!-- Thanh tìm kiếm & lọc trên Web -->
                <form method="GET" action="/admin" class="filter-bar">
                    <input type="text" name="q" value="{}" placeholder="Tìm tên shop, mã code, mức giảm (VD: 35%, LANGXDKKP, Tiki)..." class="field-input grow-field">
                    <select name="plat" class="field-input">
                        <option value="all" {}>🌟 Tất Cả Sàn &amp; Loại</option>
                        <option value="pct20" {}>🔥 Giảm Sâu 20% - 35%+</option>
                        <option value="other" {}>💸 Giảm Tiền K/VNĐ &amp; Khác</option>
                        <option value="Shopee" {}>🟠 Chỉ Shopee</option>
                        <option value="Tiki" {}>🔹 Chỉ Tiki</option>
                    </select>
                    <button type="submit" class="btn btn--secondary btn--sm">🔍 Lọc Mã</button>
                    <span class="cell-muted">Hiển thị <strong style="color: var(--text-primary);">{}</strong> / <strong>{}</strong> mã khớp</span>
                </form>

                <div class="table-wrap">
                    <table class="data-table">
                        <thead>
                            <tr>
                                <th>Mức Giảm</th>
                                <th>Sàn</th>
                                <th>Mã Code</th>
                                <th>Gian Hàng &amp; Điều Kiện Áp Mã</th>
                                <th>HSD</th>
                                <th>Link Áp Mã</th>
                            </tr>
                        </thead>
                        <tbody>
                            {}
                        </tbody>
                    </table>
                </div>
            </section>

            <!-- Kho Mã Độc Quyền Hợp Tác (Cấp Riêng AM / MCN / Shop) -->
            <section class="card card--purple">
                <div class="card-head">
                    <div>
                        <h2 class="card-title">
                            <span>👑</span>
                            <span>Kho Mã Độc Quyền Hợp Tác (Cấp Riêng AM / MCN / Nhãn Hàng)</span>
                            <span class="badge badge--purple">{} mã đã lưu</span>
                        </h2>
                        <p class="card-desc">
                            Dán hàng loạt mã độc quyền nhận được từ Account Manager ACCESSTRADE (MCN / Super Hoa Hồng) hoặc hợp tác trực tiếp với Shop. Các mã này luôn được <b>ưu tiên hàng đầu</b> trên Telegram Bot!
                        </p>
                    </div>
                </div>

                <div class="grid-2">
                    <!-- Nhập hàng loạt -->
                    <form method="POST" action="/admin/deals/import" class="subpanel">
                        <div class="subpanel__title" style="color: var(--purple-text);">⚡ Nhập Nhanh Hàng Loạt (Hỗ trợ dấu | hoặc copy thẳng từ Excel/Google Sheets)</div>
                        <p class="card-desc" style="margin: 0;">
                            Mỗi dòng 1 mã theo thứ tự: <code>Sàn | Tên/Shop | Mã Code | Mức giảm | Link áp mã | Đơn tối thiểu</code>
                        </p>
                        <textarea name="bulk_lines" rows="5" required placeholder="Shopee | Deal Độc Quyền MCN 30% | ATVIP30 | Giảm 30% tối đa 100k | https://shopee.vn/search?... | 200000" class="code-textarea"></textarea>
                        <div>
                            <button type="submit" class="btn btn--purple">📥 Nhập Danh Sách Mã Độc Quyền</button>
                        </div>
                    </form>

                    <!-- Thêm lẻ 1 mã -->
                    <form method="POST" action="/admin/deals/add" class="subpanel">
                        <div class="subpanel__title">➕ Thêm Lẻ 1 Mã Độc Quyền</div>
                        <div class="form-grid-2">
                            <select name="platform" class="field-input">
                                <option value="Shopee">Shopee</option>
                                <option value="TikTok Shop">TikTok Shop</option>
                                <option value="Lazada">Lazada</option>
                                <option value="Tiki">Tiki</option>
                                <option value="Alibaba">Alibaba</option>
                            </select>
                            <input type="text" name="coupon_code" required placeholder="Mã Code (VD: ATVIP35)" class="field-input field-input--mono">
                            <input type="text" name="title" required placeholder="Tên Shop / Chương trình" class="field-input">
                            <input type="text" name="discount_desc" required placeholder="Mức giảm (VD: Giảm 35% tối đa 120k)" class="field-input">
                            <input type="url" name="deal_url" required placeholder="Link áp mã (https://...)" class="field-input field-input--mono">
                            <input type="number" name="min_spend" min="0" step="1000" value="0" placeholder="Đơn tối thiểu (VNĐ)" class="field-input field-input--mono">
                        </div>
                        <label class="check-row">
                            <input type="checkbox" name="is_exclusive" value="1" checked>
                            <span>Đánh dấu là 👑 Mã Độc Quyền Hợp Tác (Ưu tiên hiển thị đầu tiên)</span>
                        </label>
                        <div>
                            <button type="submit" class="btn btn--secondary">💾 Lưu Mã Độc Quyền</button>
                        </div>
                    </form>
                </div>

                <div class="table-wrap">
                    <table class="data-table">
                        <thead>
                            <tr>
                                <th>Loại</th>
                                <th>Sàn</th>
                                <th>Tên &amp; Mô Tả</th>
                                <th>Mã Code</th>
                                <th>Trạng Thái</th>
                                <th>Hành Động</th>
                            </tr>
                        </thead>
                        <tbody>
                            {}
                        </tbody>
                    </table>
                </div>
            </section>
        </div>

        <!-- TAB 4: BÁO ĐƠN & DOANH THU (ORDERS) -->
        <div id="tab-pane-orders" class="tab-pane is-hidden">
            {}
        </div>
    </main>

    <footer class="admin-footer">
        Thế Giới Siêu Sale Affiliate System · Bản Quyền 2026 · Vận hành bảo mật trên Rust + LibSQL
    </footer>

    <script>
        function switchTab(tabId) {{
            document.querySelectorAll('.tab-pane').forEach(el => el.classList.add('is-hidden'));
            document.querySelectorAll('.tab-btn').forEach(btn => btn.classList.remove('active'));
            const target = document.getElementById('tab-pane-' + tabId);
            const btn = document.getElementById('tab-btn-' + tabId);
            if (target) target.classList.remove('is-hidden');
            if (btn) btn.classList.add('active');
            try {{
                history.replaceState(null, '', '#' + tabId);
            }} catch (e) {{}}
        }}

        window.addEventListener('DOMContentLoaded', () => {{
            let hash = window.location.hash.replace('#', '');
            if (!['overview', 'platforms', 'vouchers', 'orders'].includes(hash)) {{
                const urlParams = new URLSearchParams(window.location.search);
                if (urlParams.has('q') || urlParams.has('plat')) {{
                    hash = 'vouchers';
                }} else {{
                    hash = 'overview';
                }}
            }}
            switchTab(hash);
        }});
    </script>
    <script src="/assets/home.js" defer></script>
</body>
</html>"#,
        escape_html(&state.config.admin_username),
        escape_html(&state.config.bot_username),
        escape_html(&state.config.bot_username),
        bot_mode_badge,
        auth_action_html,
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
        clicks_rows,
        at_status_badge,
        campaign_status_html,
        escape_html(&camp_shopee),
        escape_html(&camp_lazada),
        escape_html(&camp_tiki),
        escape_html(&camp_tiktok),
        shopee_card_html,
        escape_html(&affiliate_template),
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
        order_alerts_card_html
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
        let clean_tok = crate::services::accesstrade::clean_api_token(&form.api_token);
        if !clean_tok.is_empty() {
            let _ = Repository::set_setting(&conn, "accesstrade_api_token", clean_tok).await;
            info!("✅ Quản trị viên đã cập nhật ACCESSTRADE API Token mới (đã làm sạch tiền tố) qua Web Admin.");
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
    has_cf_tunnel_token: bool,
    quick_tunnel_url: Option<&str>,
    tunnel_is_running: bool,
) -> String {
    let status_badge = if !app_id.trim().is_empty() && !app_secret.trim().is_empty() {
        format!(
            r#"<span class="badge badge--ok">🟢 Đang bật Open API (Secret: {})</span>"#,
            escape_html(&mask_token(app_secret))
        )
    } else if is_valid_shopee_affiliate_id(aff_id) {
        format!(
            r#"<span class="badge badge--ok">🟢 Đang bật Direct Affiliate ID: {}</span>"#,
            escape_html(aff_id.trim())
        )
    } else {
        r#"<span class="badge badge--muted">⚪ Chưa cấu hình — đang dùng luồng ACCESSTRADE</span>"#.to_string()
    };

    let tunnel_badge_html = if has_cf_tunnel_token && tunnel_is_running {
        r#"<span class="badge badge--ok">🌐 Named Tunnel đang chạy</span>"#.to_string()
    } else if let Some(qt) = quick_tunnel_url {
        format!(
            r#"<span class="badge badge--info">⚡ Auto Tunnel: {}</span>"#,
            escape_html(qt)
        )
    } else {
        r#"<span class="badge badge--warn">⚠️ Tunnel chưa bật</span>"#.to_string()
    };

    let tunnel_token_badge = if has_cf_tunnel_token {
        "🟢 Đã lưu Token cố định"
    } else {
        "Chưa gắn — đang tự chạy Quick Tunnel"
    };
    let tunnel_token_placeholder = if has_cf_tunnel_token {
        "Để trống để giữ Token hiện tại (hoặc dán Token mới để đổi)..."
    } else {
        "Dán Cloudflare Tunnel Token (eyJ... hoặc cả lệnh service install) để giữ cố định thegioisieusale.click..."
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
        "https://origin.thegioisieusale.click",
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
        r##"<section class="card card--orange">
            <div class="card-head">
                <div>
                    <h2 class="card-title"><span>🟠</span><span>Kết Nối Shopee Affiliate Chính Chủ &amp; Bọc Link Riêng</span></h2>
                    <p class="card-desc">
                        Ưu tiên link Shopee chính chủ qua Open API hoặc <code>s.shopee.vn/an_redir</code>. Khi chạy trên PC, hệ thống <b>tự động bật Cloudflare Tunnel</b> ngay khi mở server — không cần thao tác tay!
                    </p>
                </div>
                <div class="card-head-actions">{}{}</div>
            </div>
            <form method="POST" action="/admin/settings/shopee" class="form-stack">
                <div class="grid-3">
                    <div>
                        <label class="field-label">Shopee Affiliate ID (Dùng ngay qua an_redir)</label>
                        <input type="text" name="shopee_affiliate_id" value="{}" placeholder="Ví dụ: 17375520550 (5-20 chữ số)" class="field-input field-input--mono">
                    </div>
                    <div>
                        <label class="field-label">Shopee Open API AppID (Tùy chọn)</label>
                        <input type="text" name="shopee_app_id" value="{}" placeholder="Nhập AppID từ affiliate.shopee.vn..." class="field-input field-input--mono">
                    </div>
                    <div>
                        <label class="field-label">Shopee Open API Secret Key (Tùy chọn)</label>
                        <input type="password" name="shopee_app_secret" placeholder="{}" class="field-input field-input--mono">
                    </div>
                </div>
                <div class="grid-2 divider-top">
                    <div class="form-stack">
                        <div>
                            <label class="field-label">Bộ Tham Số Phiên IG/FB Reels Passthrough (<span style="color:#fdba74;">{}</span>)</label>
                            <input type="password" name="shopee_social_params" placeholder="{}" class="field-input field-input--mono">
                            <label class="check-row" style="margin-top:0.45rem;">
                                <input type="checkbox" name="clear_social_params" value="1">
                                <span>Xóa bộ tham số phiên IG/FB đang lưu</span>
                            </label>
                        </div>
                    </div>
                    <div class="form-stack">
                        <div>
                            <label class="field-label">Tên Miền Bọc Link Riêng / Tunnel Đang Dùng</label>
                            <input type="text" name="shortlink_base_url" value="{}" placeholder="Mặc định: tự động Quick Tunnel hoặc https://thegioisieusale.click (nhập off để tắt)" class="field-input field-input--mono">
                        </div>
                        <div>
                            <label class="field-label">Cloudflare Named Tunnel Token (<span style="color:#7dd3fc;">{}</span>)</label>
                            <input type="password" name="cloudflare_tunnel_token" placeholder="{}" class="field-input field-input--mono">
                            <label class="check-row" style="margin-top:0.45rem;">
                                <input type="checkbox" name="clear_tunnel_token" value="1">
                                <span>Xóa Tunnel Token đã lưu (quay về Auto Quick Tunnel)</span>
                            </label>
                        </div>
                        <label class="check-row">
                            <input type="checkbox" name="shortlink_mobile_only" value="1" {}>
                            <span>🛡️ Chặn máy tính soi link bọc (Chỉ cho điện thoại mở giống godeal.online)</span>
                        </label>
                        <label class="check-row">
                            <input type="checkbox" name="public_home_cloak" value="1" {}>
                            <span>🙈 Ẩn luôn Trang chủ <code>/</code> thành <code>404 Not Found</code> (Chỉ bật nếu không chờ duyệt web Shopee/AT)</span>
                        </label>
                    </div>
                </div>
                <div>
                    <button type="submit" class="btn btn--primary">
                        💾 Lưu Cấu Hình Shopee &amp; Tự Động Áp Dụng Tunnel
                    </button>
                </div>
            </form>
            <details class="cf-details">
                <summary class="cf-summary">
                    ☁️ Hướng dẫn gắn tên miền cố định (thegioisieusale.click) cho Cloudflare Tunnel (Tùy chọn — không gắn vẫn tự chạy Quick Tunnel)
                </summary>
                <div class="cf-body">
                    <ol class="cf-steps">
                        <li>Vào <b>one.dash.cloudflare.com</b> (Zero Trust → Networks → Tunnels) tạo 1 Tunnel mới, trỏ Public Hostname <code>origin.thegioisieusale.click</code> về <code>HTTP</code> <code>127.0.0.1:10000</code>, rồi dán Tunnel Token vào ô <b>Cloudflare Named Tunnel Token</b> ở trên và bấm <b>Lưu</b> (hệ thống tự kích hoạt ngay).</li>
                        <li>Vào <b>dash.cloudflare.com</b> → <b>Workers &amp; Pages</b> → chọn Worker của <code>thegioisieusale.click</code> → <b>Edit code</b>, dán toàn bộ code bên dưới vào rồi bấm <b>Deploy</b>.</li>
                        <li>Mọi đường dẫn bot gửi sẽ cố định dạng <code>https://thegioisieusale.click/8_ky_tu</code>; trang chủ <code>/</code> phục vụ Trang Đích 3D tại Cloudflare Edge và mọi đường dẫn nội bộ (<code>/admin</code>, <code>/convert</code>, <code>/health</code>...) đều trả <code>404 Not Found</code> khi truy cập từ Internet!</li>
                    </ol>
                    <textarea id="cf-worker-code" readonly rows="10" class="field-input field-input--mono code-textarea">{}</textarea>
                    <div>
                        <button type="button" class="btn btn--secondary js-copy-link" data-copy-target="cf-worker-code">
                            📋 Sao chép Code Cloudflare Worker
                        </button>
                    </div>
                </div>
            </details>
        </section>"##,
        status_badge,
        tunnel_badge_html,
        escape_html(aff_id),
        escape_html(app_id),
        secret_placeholder,
        social_badge,
        social_placeholder,
        escape_html(shortlink_base_url),
        tunnel_token_badge,
        tunnel_token_placeholder,
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
    pub cloudflare_tunnel_token: String,
    #[serde(default)]
    pub clear_tunnel_token: Option<String>,
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
    if !short_base_trimmed.is_empty()
        && !crate::config::is_shortlink_disabled_value(short_base_trimmed)
        && !is_valid_http_url(short_base_trimmed)
    {
        return redirect_with_notice("Tên miền bọc link riêng phải bắt đầu bằng https:// hoặc http:// (hoặc nhập off để tắt).");
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

    let tunnel_tok_trimmed = form.cloudflare_tunnel_token.trim();
    let validated_tunnel_tok = if form.clear_tunnel_token.as_deref() == Some("1") {
        Some(String::new())
    } else if !tunnel_tok_trimmed.is_empty() {
        match crate::services::tunnel::extract_tunnel_token(tunnel_tok_trimmed) {
            Some(tok) => Some(tok),
            None => {
                return redirect_with_notice(
                    "Cloudflare Tunnel Token chưa hợp lệ (phải chứa chuỗi token bắt đầu bằng eyJ...).",
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
    if let Some(tok_val) = validated_tunnel_tok {
        let _ = Repository::set_setting(&conn, "cloudflare_tunnel_token", &tok_val).await;
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

    crate::services::tunnel::ensure_tunnel_running(&state.db, &state.config).await;

    info!("✅ Quản trị viên đã cập nhật cấu hình Shopee Chính Chủ & Bọc Link trên Web Admin.");
    redirect_with_notice("Đã lưu cấu hình Shopee Chính Chủ & đồng bộ Cloudflare Tunnel ngay lập tức!")
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
        r#"<span class="badge badge--warn">🟡 Chưa gắn Telegram ID nhận báo đơn</span>"#.to_string()
    } else {
        format!(
            r#"<span class="badge badge--ok">🟢 Đang báo đơn tới ID: {}</span>"#,
            escape_html(admin_telegram_id.trim())
        )
    };

    let mut order_rows = String::new();
    for ord in orders {
        let status_cls = match ord.status.as_str() {
            "APPROVED" => "badge badge--ok",
            "REJECTED" => "badge badge--err",
            _ => "badge badge--warn",
        };
        order_rows.push_str(&format!(
            r#"<tr>
                <td class="cell-strong">{}</td>
                <td class="font-mono cell-muted">{}</td>
                <td><span class="{}">{}</span></td>
                <td class="cell-strong">{}</td>
                <td class="font-mono" style="color:#4ade80;font-weight:700;">{}</td>
                <td class="font-mono cell-muted">{}</td>
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
        order_rows = r#"<tr><td colspan="6" class="empty-cell">Chưa có đơn hàng phát sinh được ghi nhận trong 7 ngày gần nhất. Khi có đơn thật từ Shopee / TikTok Shop / ACCESSTRADE, hệ thống sẽ tự lưu và báo ngay về Telegram!</td></tr>"#.to_string();
    }

    format!(
        r#"<section class="card card--green">
            <div class="card-head">
                <div>
                    <h2 class="card-title"><span>🔔</span><span>Báo Đơn Tự Động Về Telegram &amp; Đối Soát Hoa Hồng (Mỗi 10 Phút)</span></h2>
                    <p class="card-desc">
                        Gõ lệnh <code>/stats</code> trong bot Telegram để lấy ID số của bạn rồi dán vào ô bên dưới. Mỗi khi có đơn mới trên ACCESSTRADE / Shopee Open API, bot sẽ nhắn tin báo ngay!
                    </p>
                </div>
                <div>{}</div>
            </div>
            <div class="filter-bar">
                <form method="POST" action="/admin/settings/notifications" class="inline-form-grow">
                    <div class="grow-field">
                        <label class="field-label">Telegram ID Admin Nhận Thông Báo Đơn</label>
                        <input type="text" name="admin_telegram_id" value="{}" placeholder="Ví dụ: 5123456789 (hoặc để trống để tắt)" class="field-input font-mono">
                    </div>
                    <button type="submit" class="btn btn--success">
                        💾 Lưu Telegram ID Báo Đơn
                    </button>
                </form>
                <form method="POST" action="/admin/orders/sync">
                    <button type="submit" class="btn btn--secondary">
                        🔄 Quét Đơn Mới Ngay
                    </button>
                </form>
            </div>
            <div class="table-wrap">
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>Sàn</th>
                            <th>Mã Đơn Hàng</th>
                            <th>Trạng Thái</th>
                            <th>Giá Trị Đơn</th>
                            <th>Hoa Hồng</th>
                            <th>Thời Gian Đặt</th>
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

        // 2. Mặc định cho phép mở cả Điện thoại & Máy tính (shortlink_mobile_only tắt) -> 302 Found
        let mut desktop_hdrs = HeaderMap::new();
        desktop_hdrs.insert(
            header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/122.0.0.0 Safari/537.36"
                .parse()
                .unwrap(),
        );
        let resp_desktop_default = handle_shortlink_redirect(
            Path(code.clone()),
            desktop_hdrs.clone(),
            State(state.clone()),
        )
        .await;
        assert_eq!(resp_desktop_default.status(), StatusCode::FOUND);

        // 2b. Khi Admin chủ động bật shortlink_mobile_only = "1" -> Desktop bị chặn 403 Forbidden
        {
            let conn = state.db.conn().unwrap();
            Repository::set_setting(&conn, "shortlink_mobile_only", "1").await.unwrap();
        }
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
        assert!(is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/assets/bot-logo.webp"
        ));
        assert!(is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/assets/bot-cart-3d.webp"
        ));
        assert!(is_allowed_shortlink_cloak_path(
            &axum::http::Method::GET,
            "/assets/landing.js"
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

    #[tokio::test]
    async fn test_pc_tunnel_cloak_blocks_admin_while_keeping_localhost_alive() {
        let test_db_path = format!("target/test_pc_tunnel_{}.db", uuid::Uuid::new_v4().simple());
        let mut state = make_test_state(&test_db_path, None).await;
        state.config.public_base_url = None;

        let code = {
            let conn = state.db.conn().unwrap();
            Repository::set_setting(&conn, "shortlink_base_url", "https://thegioisieusale.click")
                .await
                .unwrap();
            Repository::create_or_get_short_link(
                &conn,
                "https://s.shopee.vn/pc-tunnel-test",
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

        // Localhost trên PC truy cập /admin/login bình thường (200 OK)
        let local_login = client.get(format!("{}/admin/login", base)).send().await.unwrap();
        assert_eq!(local_login.status().as_u16(), 200);

        // Request đi qua Cloudflare Tunnel / origin.thegioisieusale.click phải bị khóa /admin và / -> 404
        for (hdr_name, hdr_val) in [
            ("host", "origin.thegioisieusale.click"),
            ("cf-ray", "test-ray-123"),
        ] {
            let admin_blocked = client
                .get(format!("{}/admin", base))
                .header(hdr_name, hdr_val)
                .send()
                .await
                .unwrap();
            assert_eq!(admin_blocked.status().as_u16(), 404);

            let root_blocked = client
                .get(format!("{}/", base))
                .header(hdr_name, hdr_val)
                .send()
                .await
                .unwrap();
            assert_eq!(root_blocked.status().as_u16(), 404);

            let bridge_ok = client
                .get(format!("{}/assets/bridge.js", base))
                .header(hdr_name, hdr_val)
                .send()
                .await
                .unwrap();
            assert_eq!(bridge_ok.status().as_u16(), 200);

            let link_ok = client
                .get(format!("{}/{}", base, code))
                .header(hdr_name, hdr_val)
                .header("user-agent", "facebookexternalhit/1.1")
                .send()
                .await
                .unwrap();
            assert_eq!(link_ok.status().as_u16(), 200);
        }

        server_task.abort();
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
        assert!(!landing_html.contains("Bắt mã tự động khung 0H"));
        assert!(!landing_html.contains("raw.githubusercontent.com"));
        assert!(landing_html.contains("/assets/bot-logo.webp"));
        assert!(landing_html.contains("/assets/bot-cart-3d.webp"));
        assert!(landing_html.contains("/assets/landing.js"));
        assert!(landing_html.contains("perspective"));
        assert!(landing_html.contains("preserve-3d"));
        assert!(landing_html.contains("@keyframes"));
        assert!(landing_html.contains("prefers-reduced-motion"));
    }

    #[tokio::test]
    async fn test_local_pc_skips_admin_login_while_blocking_spoof_and_cloud() {
        let pc_db = format!("target/test_local_nologin_{}.db", uuid::Uuid::new_v4().simple());
        let mut pc_state = make_test_state(&pc_db, None).await;
        pc_state.config.public_base_url = None;

        let pc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let pc_addr = pc_listener.local_addr().unwrap();
        let pc_app = build_router(pc_state).into_make_service_with_connect_info::<SocketAddr>();
        let pc_srv = tokio::spawn(async move {
            let _ = axum::serve(pc_listener, pc_app).await;
        });

        let no_redir = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let pc_base = format!("http://127.0.0.1:{}", pc_addr.port());

        // 1. Chạy tại máy PC -> vào thẳng /admin (200 OK), không bị bắt đăng nhập
        let direct_admin = no_redir.get(format!("{}/admin", pc_base)).send().await.unwrap();
        assert_eq!(direct_admin.status().as_u16(), 200);
        let body = direct_admin.text().await.unwrap();
        assert!(body.contains("Trung Tâm Điều Hành"));
        assert!(body.contains("PC Local (Miễn đăng nhập)"));

        // 2. Giả mạo header nội bộ kèm proxy header hoặc Host ngoài phải bị từ chối
        let spoof_proxy = no_redir
            .get(format!("{}/admin", pc_base))
            .header(LOCAL_PC_TRUSTED_HEADER, "1")
            .header("x-forwarded-for", "203.0.113.55")
            .send()
            .await
            .unwrap();
        assert_eq!(spoof_proxy.status().as_u16(), 303);

        let spoof_host = no_redir
            .get(format!("{}/admin", pc_base))
            .header(LOCAL_PC_TRUSTED_HEADER, "1")
            .header("host", "192.168.1.99:10000")
            .send()
            .await
            .unwrap();
        assert_eq!(spoof_host.status().as_u16(), 303);

        pc_srv.abort();
        let _ = std::fs::remove_file(&pc_db);

        // 3. Chế độ Cloud (có public_base_url) -> vẫn bắt buộc đăng nhập bình thường
        let cloud_db = format!("target/test_cloud_login_{}.db", uuid::Uuid::new_v4().simple());
        let cloud_state = make_test_state(&cloud_db, None).await;
        let cloud_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let cloud_addr = cloud_listener.local_addr().unwrap();
        let cloud_app = build_router(cloud_state).into_make_service_with_connect_info::<SocketAddr>();
        let cloud_srv = tokio::spawn(async move {
            let _ = axum::serve(cloud_listener, cloud_app).await;
        });

        let cloud_admin = no_redir
            .get(format!("http://127.0.0.1:{}/admin", cloud_addr.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(cloud_admin.status().as_u16(), 303);

        cloud_srv.abort();
        let _ = std::fs::remove_file(&cloud_db);
    }
}
