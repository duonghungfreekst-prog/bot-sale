use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use axum::{
    extract::{ConnectInfo, Form, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    middleware::Next,
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

use crate::bot::handlers::load_owned_aff_settings;
use crate::config::Config;
use crate::db::models::{ClickRecord, TrackedOrder};
use crate::db::repository::{NewDealInput, Repository};
use crate::db::AppDb;
use crate::services::accesstrade::{
    mask_token, AccessTradeClient, ApprovedCampaigns, VoucherMatchQuery, VoucherOffer,
};
use crate::services::deal_parser::DealParser;
use crate::services::order_tracker;
use crate::services::shopee_affiliate::is_valid_shopee_affiliate_id;
use crate::utils::{escape_html, format_vnd, sanitize_crlf};


mod admin;
mod auth;
mod middleware;
mod public;
mod shortlink;

use admin::*;
use auth::*;
use middleware::*;
use public::*;
use shortlink::*;
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
        .layer(axum::middleware::from_fn(admin_csrf_middleware))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            shortlink_cloak_middleware,
        ))
        .layer(axum::middleware::from_fn(security_headers_middleware))
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


/// Health check cho Render Web Service
async fn handle_health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "The Gioi Sieu Sale Bot & Web Admin Healthy")
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
        let db = AppDb::init_local_for_test(test_db_path).await.unwrap();
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
        assert!(html.contains("--color-bg: #F4F6F9;"));
        assert!(html.contains("/assets/bot-logo.webp"));
        assert!(html.contains("/assets/bot-cart-3d.webp"));
        assert!(html.contains("/assets/landing.js"));
        assert!(html.contains(r#"id="public-theme-btn""#));
        assert!(html.contains("public-theme-ico-use"));
        assert!(html.contains("platforms-strip"));
        assert!(html.contains("plat-dot"));
        assert!(html.contains(r##"<use href="#ico-shield"/>"##));
        assert!(html.contains("<svg"));
        assert!(!html.contains("#0c0f17"));

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
        assert!(desktop_body.contains("<svg"));
        assert!(!desktop_body.contains("#0c0f17"));

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
        assert!(mobile_body.contains("--bg: #F4F6F9;"));
        assert!(mobile_body.contains("<svg"));
        assert!(!mobile_body.contains("#0c0f17"));

        // Trích xuất payload/key và xác nhận decode_bridge_payload giải mã khớp 100%
        let p_hex = mobile_body.split("data-p=\"").nth(1).unwrap().split('"').next().unwrap();
        let k_hex = mobile_body.split("data-k=\"").nth(1).unwrap().split('"').next().unwrap();
        assert_eq!(decode_bridge_payload(p_hex, k_hex).as_deref(), Some(secret_target));

        // 4. Tắt shortlink_mobile_only: Mobile vẫn nhận Cầu nối Smart Bridge (data-mobile-only="0"), còn PC nhận 302 Found trực tiếp
        {
            let conn = state.db.conn().unwrap();
            Repository::set_setting(&conn, "shortlink_mobile_only", "0").await.unwrap();
        }
        let resp_mobile_open = handle_shortlink_redirect(
            Path(code.clone()),
            mobile_hdrs,
            State(state.clone()),
        )
        .await;
        assert_eq!(resp_mobile_open.status(), StatusCode::OK);
        let mobile_open_body = String::from_utf8(
            axum::body::to_bytes(resp_mobile_open.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(mobile_open_body.contains("data-mobile-only=\"0\""));

        let mut pc_hdrs = HeaderMap::new();
        pc_hdrs.insert(
            header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/124.0".parse().unwrap(),
        );
        let resp_302 = handle_shortlink_redirect(
            Path(code),
            pc_hdrs,
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
        let locked_body = String::from_utf8(
            axum::body::to_bytes(resp_locked.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(locked_body.contains("--bg-base: #F4F6F9;"));
        assert!(locked_body.contains("<svg"));
        assert!(!locked_body.contains("cdn.jsdelivr.net"));

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
        assert!(landing_html.contains("--color-bg: #F4F6F9;"));
        assert!(landing_html.contains(r#"html[data-theme="dark"]"#));
        assert!(landing_html.contains(r#"id="cf-theme-btn""#));
        assert!(landing_html.contains("<svg"));
        assert!(!landing_html.contains("#060d18"));
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

    #[tokio::test]
    async fn test_admin_ui_light_first_sidebar_and_theme_toggle() {
        let test_db_path = format!("target/test_admin_ui_{}.db", uuid::Uuid::new_v4().simple());
        let mut state = make_test_state(&test_db_path, None).await;
        state.config.public_base_url = None;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = build_router(state).into_make_service_with_connect_info::<SocketAddr>();
        let srv = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = reqwest::Client::new();
        let base = format!("http://127.0.0.1:{}", addr.port());

        let dash_resp = client.get(format!("{}/admin", base)).send().await.unwrap();
        assert_eq!(dash_resp.status().as_u16(), 200);
        let dash_html = dash_resp.text().await.unwrap();

        assert!(dash_html.contains("--bg-base: #F4F6F9;"));
        assert!(dash_html.contains(r#"html[data-theme="dark"]"#));
        assert!(dash_html.contains(r#"<aside class="sidebar">"#));
        assert!(dash_html.contains(r#"id="theme-toggle-btn""#));
        assert!(dash_html.contains("toggleTheme()"));
        assert!(dash_html.contains("Be Vietnam Pro"));
        assert!(dash_html.contains("DOMParser"));
        assert!(dash_html.contains("tab-pane-overview"));
        assert!(dash_html.contains("tab-pane-platforms"));
        assert!(dash_html.contains("tab-pane-vouchers"));
        assert!(dash_html.contains("tab-pane-orders"));
        assert!(dash_html.contains("PC Local (Miễn đăng nhập)"));
        assert!(!dash_html.contains("#070b14"));
        assert!(!dash_html.contains("rgba(88, 28, 135"));
        assert!(dash_html.contains("charts-grid"));
        assert!(dash_html.contains("chart-donut"));
        assert!(dash_html.contains("signal-meter"));
        assert!(dash_html.contains(r##"<use href="#ico-flame"/>"##));
        assert!(dash_html.contains(r##"<use href="#ico-crown"/>"##));
        assert!(dash_html.contains(r##"<use href="#ico-platforms"/>"##));
        assert!(dash_html.contains(r##"<use href="#ico-orders"/>"##));
        assert!(!dash_html.contains("#0c0f17"));

        srv.abort();
        let _ = std::fs::remove_file(&test_db_path);

        // Kiểm tra giao diện trang Login (ở chế độ Cloud chưa đăng nhập để không redirect thẳng vào /admin)
        let login_db = format!("target/test_admin_login_ui_{}.db", uuid::Uuid::new_v4().simple());
        let login_state = make_test_state(&login_db, None).await;
        let login_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let login_addr = login_listener.local_addr().unwrap();
        let login_app = build_router(login_state).into_make_service_with_connect_info::<SocketAddr>();
        let login_srv = tokio::spawn(async move {
            let _ = axum::serve(login_listener, login_app).await;
        });

        let login_resp = client
            .get(format!("http://127.0.0.1:{}/admin/login", login_addr.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(login_resp.status().as_u16(), 200);
        let login_html = login_resp.text().await.unwrap();
        assert!(login_html.contains("--bg-base: #F4F6F9;"));
        assert!(login_html.contains(r#"id="login-theme-btn""#));
        assert!(login_html.contains("dealx-theme"));
        assert!(login_html.contains("Đăng Nhập Quản Trị"));
        assert!(!login_html.contains("#070b14"));

        login_srv.abort();
        let _ = std::fs::remove_file(&login_db);
    }

    #[tokio::test]
    async fn test_public_home_hides_synthetic_codes_filters_guides_and_enriches_links() {
        let test_db_path = format!("target/test_pub_honest_{}.db", uuid::Uuid::new_v4().simple());
        let state = make_test_state(&test_db_path, None).await;
        {
            let conn = state.db.conn().unwrap();
            Repository::set_setting(&conn, "shopee_affiliate_id", "17375520550")
                .await
                .unwrap();
            Repository::set_setting(
                &conn,
                "shopee_social_params",
                "channel_type=fb&sm_sig_id=SIGWEB1",
            )
            .await
            .unwrap();
            Repository::set_setting(&conn, "shortlink_base_url", "off")
                .await
                .unwrap();

            Repository::add_deal(
                &conn,
                &NewDealInput {
                    platform: "Shopee".to_string(),
                    title: "[Toàn sàn] Mã Facebook KOL".to_string(),
                    coupon_code: Some("KOL_FB_22".to_string()),
                    discount_desc: "📌 Hướng dẫn lấy mã độc quyền Shopee x Facebook 22%".to_string(),
                    min_spend: 0.0,
                    deal_url: "https://shopee.vn/m/kol".to_string(),
                    expire_at: None,
                    is_exclusive: true,
                },
            )
            .await
            .unwrap();

            Repository::add_deal(
                &conn,
                &NewDealInput {
                    platform: "Shopee".to_string(),
                    title: "[Toàn sàn] Voucher Shopee 23%".to_string(),
                    coupon_code: Some("SHP_AUTO_1981279232".to_string()),
                    discount_desc: "Mã 23% tối đa 500k đơn từ 150K".to_string(),
                    min_spend: 150000.0,
                    deal_url: "https://shopee.vn/search?evcode=V23&promotionId=1981279232".to_string(),
                    expire_at: None,
                    is_exclusive: true,
                },
            )
            .await
            .unwrap();

            Repository::add_deal(
                &conn,
                &NewDealInput {
                    platform: "Shopee".to_string(),
                    title: "[MCN] Deal Độc Quyền 30%".to_string(),
                    coupon_code: Some("ATVIP30".to_string()),
                    discount_desc: "Giảm 30% tối đa 100k".to_string(),
                    min_spend: 100000.0,
                    deal_url: "https://shopee.vn/product/111/222".to_string(),
                    expire_at: None,
                    is_exclusive: true,
                },
            )
            .await
            .unwrap();
        }

        let resp = handle_root(Query(PublicHomeQuery::default()), State(state)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let html = String::from_utf8(
            axum::body::to_bytes(resp.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();

        assert!(!html.contains("KOL_FB_22"));
        assert!(!html.contains("Hướng dẫn lấy mã độc quyền"));
        assert!(!html.contains("SHP_AUTO_1981279232"));
        assert!(html.contains("Áp tự động qua link (Không cần gõ mã)"));
        assert!(html.contains("Lưu &amp; Áp ưu đãi Shopee"));
        assert!(html.contains("<code>ATVIP30</code>"));
        assert!(html.contains(r#"data-copy-url="ATVIP30""#));
        assert!(html.contains("affiliate_id=17375520550"));
        assert!(html.contains("sm_sig_id=SIGWEB1"));

        let _ = std::fs::remove_file(&test_db_path);
    }

    #[tokio::test]
    async fn test_admin_csrf_blocks_cross_origin_and_bad_token_allows_same_origin() {
        let test_db_path = format!("target/test_csrf_{}.db", uuid::Uuid::new_v4().simple());
        let mut state = make_test_state(&test_db_path, None).await;
        state.config.public_base_url = None;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = build_router(state).into_make_service_with_connect_info::<SocketAddr>();
        let srv = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let base = format!("http://127.0.0.1:{}", addr.port());

        let dash_resp = client.get(format!("{}/admin", base)).send().await.unwrap();
        assert_eq!(dash_resp.status().as_u16(), 200);

        let csrf_cookie_val = dash_resp
            .headers()
            .get_all(axum::http::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|s| {
                s.split(';')
                    .next()
                    .and_then(|kv| kv.strip_prefix("dealx_csrf="))
                    .map(|v| v.to_string())
            })
            .expect("GET /admin must set dealx_csrf cookie");
        let dash_html = dash_resp.text().await.unwrap();
        assert!(dash_html.contains(&format!(
            r#"<meta name="csrf-token" content="{}">"#,
            csrf_cookie_val
        )));
        assert!(dash_html.contains(&format!(
            r#"<input type="hidden" name="_csrf" value="{}">"#,
            csrf_cookie_val
        )));

        let form_ok = format!(
            "platform=Shopee&title=Test+CSRF+Deal&coupon_code=CSRF10&discount_desc=Giam+10k&min_spend=0&deal_url=https%3A%2F%2Fshopee.vn%2F&is_exclusive=1&_csrf={}",
            csrf_cookie_val
        );

        // 1. Cross-origin Origin header bị chặn 403
        let evil_origin = client
            .post(format!("{}/admin/deals/add", base))
            .header("Origin", "https://evil.example")
            .header("Cookie", format!("dealx_csrf={}", csrf_cookie_val))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form_ok.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(evil_origin.status().as_u16(), 403);

        // 2. Sec-Fetch-Site: cross-site bị chặn 403
        let cross_site = client
            .post(format!("{}/admin/deals/add", base))
            .header("Sec-Fetch-Site", "cross-site")
            .header("Cookie", format!("dealx_csrf={}", csrf_cookie_val))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form_ok.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(cross_site.status().as_u16(), 403);

        // 3. Có dealx_csrf cookie nhưng thiếu/sai _csrf token bị chặn 403
        let bad_token = client
            .post(format!("{}/admin/deals/add", base))
            .header("Origin", base.clone())
            .header("Cookie", format!("dealx_csrf={}", csrf_cookie_val))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("platform=Shopee&title=Bad&discount_desc=Bad&min_spend=0&deal_url=https%3A%2F%2Fshopee.vn%2F&_csrf=wrong_token")
            .send()
            .await
            .unwrap();
        assert_eq!(bad_token.status().as_u16(), 403);

        // 4. Same-Origin + dealx_csrf cookie khớp với _csrf được phép thực thi (303)
        let allowed = client
            .post(format!("{}/admin/deals/add", base))
            .header("Origin", base.clone())
            .header("Sec-Fetch-Site", "same-origin")
            .header("Cookie", format!("dealx_csrf={}", csrf_cookie_val))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form_ok)
            .send()
            .await
            .unwrap();
        assert_eq!(allowed.status().as_u16(), 303);

        srv.abort();
        let _ = std::fs::remove_file(&test_db_path);
    }

    #[tokio::test]
    async fn test_security_headers_csp_clean_and_rate_limiter_prune() {
        let test_db_path = format!("target/test_csp_{}.db", uuid::Uuid::new_v4().simple());
        let state = make_test_state(&test_db_path, None).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = build_router(state).into_make_service_with_connect_info::<SocketAddr>();
        let srv = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{}/", addr.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let csp = resp
            .headers()
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok())
            .expect("CSP header must be present");
        assert!(!csp.contains("cdn.jsdelivr.net"));
        assert!(!csp.contains("cdn.tailwindcss.com"));
        assert_eq!(
            resp.headers()
                .get("x-content-type-options")
                .and_then(|v| v.to_str().ok()),
            Some("nosniff")
        );

        srv.abort();
        let _ = std::fs::remove_file(&test_db_path);

        // Kiểm tra prune_rate_limiter_map xóa mục hết hạn trước và giữ khóa đang hiệu lực
        let mut limiter: HashMap<String, (u32, i64)> = HashMap::new();
        let now_ts = 10_000;
        for i in 0..8 {
            limiter.insert(format!("expired_{}", i), (2, now_ts - 120 - i));
        }
        limiter.insert("active_old".to_string(), (5, now_ts - 20));
        limiter.insert("active_mid".to_string(), (5, now_ts - 10));
        limiter.insert("active_new".to_string(), (5, now_ts - 2));

        auth::prune_rate_limiter_map(&mut limiter, now_ts, 60, 4);

        assert!(limiter.len() <= 4);
        assert!(limiter.contains_key("active_old"));
        assert!(limiter.contains_key("active_mid"));
        assert!(limiter.contains_key("active_new"));
        assert!(!limiter.contains_key("expired_0"));
    }
}
