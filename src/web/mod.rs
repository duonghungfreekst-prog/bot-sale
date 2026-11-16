use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use axum::{
    extract::{Form, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;
use tracing::{error, info};
use uuid::Uuid;

use crate::config::Config;
use crate::db::repository::Repository;
use crate::db::AppDb;
use crate::services::accesstrade::{mask_token, AccessTradeClient};
use crate::utils::escape_html;

#[derive(Clone)]
pub struct AppState {
    pub db: AppDb,
    pub config: Config,
    pub accesstrade: Arc<AccessTradeClient>,
    pub sessions: Arc<RwLock<HashSet<String>>>,
}

/// Khởi chạy Web Admin Dashboard Server
pub async fn start_web_server(
    port: u16,
    db: AppDb,
    config: Config,
    accesstrade: Arc<AccessTradeClient>,
) -> anyhow::Result<()> {
    let state = AppState {
        db,
        config,
        accesstrade,
        sessions: Arc::new(RwLock::new(HashSet::new())),
    };

    let app = build_router(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("🚀 Web Admin Dashboard DealX đang chạy tại: http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    Router::new()
        // Các route công khai
        .route("/", get(handle_root))
        .route("/health", get(handle_health))
        .route("/admin/login", get(handle_login_page).post(handle_login_submit))
        .route("/admin/logout", get(handle_logout))
        // Các route quản trị (chỉ 1 tài khoản Admin duy nhất)
        .route("/admin", get(handle_dashboard))
        .route("/admin/settings/accesstrade", post(handle_update_accesstrade))
        .route("/admin/settings/accesstrade/sync", post(handle_sync_accesstrade))
        .route("/admin/settings/affiliate", post(handle_update_affiliate_settings))
        .layer(middleware::from_fn(security_headers_middleware))
        .with_state(state)
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
        HeaderValue::from_static("default-src 'self'; style-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net; img-src 'self' data:"),
    );
    res
}

/// Root redirect về /admin
async fn handle_root() -> Redirect {
    Redirect::to("/admin")
}

/// Health check cho Render Web Service
async fn handle_health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "DealX Bot & Web Admin Healthy")
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

/// Kiểm tra xem yêu cầu hiện tại có quyền Admin hay không
async fn is_authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    if let Some(token) = extract_session_token(headers) {
        let sessions = state.sessions.read().await;
        sessions.contains(&token)
    } else {
        false
    }
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
    <title>Đăng Nhập Quản Trị - DealX Admin</title>
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
            <h1 class="text-2xl font-black tracking-wide text-white">DEALX ADMIN</h1>
            <p class="text-sm text-gray-400 mt-1">Cổng Quản Trị Hệ Thống Săn Sale & Ép Giá</p>
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

/// Xử lý xác thực đăng nhập (Chống Timing Attack bằng Constant-Time comparison)
async fn handle_login_submit(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Response {
    let user_match = form
        .username
        .as_bytes()
        .ct_eq(state.config.admin_username.as_bytes())
        .into();
    let pass_match = form
        .password
        .as_bytes()
        .ct_eq(state.config.admin_password.as_bytes())
        .into();

    if user_match && pass_match {
        let token = Uuid::new_v4().to_string();
        {
            let mut sessions = state.sessions.write().await;
            // Giới hạn số phiên đăng nhập lưu trên RAM tránh phình bộ nhớ
            if sessions.len() >= 100 {
                sessions.clear();
            }
            sessions.insert(token.clone());
        }

        info!("✅ Quản trị viên đăng nhập thành công vào Web Dashboard!");
        let cookie_header = format!(
            "dealx_session={}; HttpOnly; Path=/; SameSite=Lax; Max-Age=86400",
            token
        );

        let mut response = Redirect::to("/admin").into_response();
        if let Ok(val) = header::HeaderValue::from_str(&cookie_header) {
            response.headers_mut().insert(header::SET_COOKIE, val);
        }
        response
    } else {
        error!("❌ Thử đăng nhập trái phép vào Web Admin Dashboard");
        tokio::time::sleep(Duration::from_millis(500)).await;
        let error_html = r#"<!DOCTYPE html>
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
        <p class="text-sm text-gray-400 mb-6">Tài khoản hoặc mật khẩu quản trị viên không chính xác. Hệ thống chỉ cho phép duy nhất 1 tài khoản Admin.</p>
        <a href="/admin/login" class="inline-block px-6 py-2.5 bg-red-600 hover:bg-red-700 rounded-xl font-semibold text-white transition">Thử Lại</a>
    </div>
</body>
</html>"#;
        Html(error_html).into_response()
    }
}

/// Đăng xuất quản trị viên
async fn handle_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = extract_session_token(&headers) {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&token);
    }

    let mut response = Redirect::to("/admin/login").into_response();
    if let Ok(val) = header::HeaderValue::from_str("dealx_session=; HttpOnly; Path=/; Max-Age=0") {
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

/// Giao diện chính Dashboard Quản Trị
async fn handle_dashboard(headers: HeaderMap, State(state): State<AppState>) -> Response {
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

    let affiliate_template = Repository::get_setting(&conn, "affiliate_template")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let db_at_token = Repository::get_setting(&conn, "accesstrade_api_token")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| state.config.accesstrade_api_token.clone());

    let camp_shopee = Repository::get_setting(&conn, "campaign_shopee").await.ok().flatten().unwrap_or_default();
    let camp_lazada = Repository::get_setting(&conn, "campaign_lazada").await.ok().flatten().unwrap_or_default();
    let camp_tiki = Repository::get_setting(&conn, "campaign_tiki").await.ok().flatten().unwrap_or_default();
    let camp_tiktok = Repository::get_setting(&conn, "campaign_tiktok").await.ok().flatten().unwrap_or_default();

    let (at_status_badge, campaign_status_html) = if let Some(ref token) = db_at_token {
        let masked = mask_token(token);
        match state.accesstrade.get_approved_campaigns(token, false).await {
            Ok(approved) => {
                let badge = format!(
                    "<span class=\"px-2.5 py-1 bg-green-500/20 text-green-400 rounded-lg text-xs font-semibold\">🟢 Đã kết nối ({})</span>",
                    masked
                );
                let rows = format!(
                    "{}{}{}{}",
                    render_platform_status("🟠 Shopee", &approved.shopee, &camp_shopee),
                    render_platform_status("🔵 Lazada", &approved.lazada, &camp_lazada),
                    render_platform_status("🔹 Tiki", &approved.tiki, &camp_tiki),
                    render_platform_status("🎵 TikTok Shop", &approved.tiktok, &camp_tiktok),
                );
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

    let dashboard_html = format!(
        r#"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>DealX - Quản Trị Hệ Thống Săn Sale & Ép Giá</title>
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
                    <h1 class="font-extrabold text-lg text-white leading-tight">DEALX DASHBOARD</h1>
                    <p class="text-xs text-gray-400">Quản Trị Bot Săn Sale Đa Sàn</p>
                </div>
            </div>

            <div class="flex items-center space-x-4">
                <div class="hidden sm:flex items-center space-x-2 text-xs text-gray-400 bg-gray-800 px-3 py-1.5 rounded-xl border border-gray-700">
                    <span class="w-2 h-2 rounded-full bg-green-400 animate-pulse"></span>
                    <span>Admin: <strong class="text-white">{}</strong></span>
                    <span class="text-gray-600">|</span>
                    <span>Bot: <a href="https://t.me/{}" target="_blank" rel="noopener noreferrer" class="text-yellow-400 hover:underline">@{}</a></span>
                </div>
                <a href="/admin/logout" class="px-3.5 py-1.5 bg-red-600/80 hover:bg-red-600 text-white rounded-xl text-xs font-semibold transition">
                    Đăng Xuất
                </a>
            </div>
        </div>
    </header>

    <main class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 space-y-8">
        <!-- Thống kê tổng quan -->
        <section class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-5">
            <div class="bg-gray-900 border border-gray-800 p-5 rounded-2xl shadow-lg">
                <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Người Dùng Telegram</div>
                <div class="text-3xl font-black text-white">{}</div>
                <div class="text-xs text-green-400 mt-2">👥 Khách đã tương tác bot</div>
            </div>

            <div class="bg-gray-900 border border-gray-800 p-5 rounded-2xl shadow-lg">
                <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Tổng Lượt Dán Link / Click</div>
                <div class="text-3xl font-black text-yellow-400">{}</div>
                <div class="text-xs text-gray-400 mt-2">🔗 Tổng số link đã xử lý</div>
            </div>

            <div class="bg-gray-900 border border-gray-800 p-5 rounded-2xl shadow-lg">
                <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Phân Bổ Lượt Dán Sàn</div>
                <div class="text-xs space-y-1 mt-1 text-gray-300">
                    <div class="flex justify-between"><span>🟠 Shopee:</span> <strong class="text-white">{}</strong></div>
                    <div class="flex justify-between"><span>🔵 Lazada:</span> <strong class="text-white">{}</strong></div>
                    <div class="flex justify-between"><span>🎵 TikTok:</span> <strong class="text-white">{}</strong></div>
                    <div class="flex justify-between"><span>🔹 Tiki:</span> <strong class="text-white">{}</strong></div>
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
                        Bot tự động gọi API <code>v1/product_link/create</code> (kèm <code>utm_source=facebook</code>) để sinh link rút gọn và cộng 100% hoa hồng về ví AccessTrade của bạn.
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
                <div class="bg-gray-950/60 border border-gray-800 rounded-xl p-4">
                    <div class="text-xs font-bold uppercase tracking-wider text-gray-400 mb-2">Trạng Thái Duyệt Các Sàn</div>
                    <div class="text-sm space-y-1">
                        {}
                    </div>
                    <p class="text-xs text-gray-500 mt-3">
                        Sàn nào báo <b>Chưa duyệt</b>: Hãy vào <a href="https://pub2.accesstrade.vn" target="_blank" rel="noopener noreferrer" class="text-yellow-400 underline">pub2.accesstrade.vn</a> bấm Đăng ký chiến dịch sàn đó rồi nhấn <b>Đồng Bộ Lại</b>.
                    </p>
                </div>

                <form method="POST" action="/admin/settings/accesstrade" class="lg:col-span-2 space-y-4">
                    <div>
                        <label class="block text-xs font-semibold text-gray-400 mb-1">ACCESSTRADE API Access Key (Token)</label>
                        <input type="password" name="api_token" placeholder="Dán API Access Key mới (hoặc để trống để giữ Token hiện tại)..." class="w-full bg-gray-800 border border-gray-700 rounded-xl px-4 py-2.5 text-white font-mono text-sm focus:outline-none focus:ring-2 focus:ring-yellow-500">
                        <p class="text-xs text-gray-500 mt-1">💡 Khuyên dùng thêm biến <code>ACCESSTRADE_API_TOKEN</code> trong tab Environment trên Render để không bị mất khi Render khởi động lại.</p>
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
        DealX Affiliate System · Bản Quyền 2026 · Vận hành bảo mật trên Rust + LibSQL
    </footer>
</body>
</html>"#,
        escape_html(&state.config.admin_username),
        escape_html(&state.config.bot_username),
        escape_html(&state.config.bot_username),
        stats.total_users,
        stats.total_clicks,
        stats.shopee_clicks,
        stats.lazada_clicks,
        stats.tiktok_clicks,
        stats.tiki_clicks,
        at_status_badge,
        campaign_status_html,
        escape_html(&camp_shopee),
        escape_html(&camp_lazada),
        escape_html(&camp_tiki),
        escape_html(&camp_tiktok),
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

async fn handle_sync_accesstrade(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let token_opt = Repository::get_setting(&conn, "accesstrade_api_token")
            .await
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| state.config.accesstrade_api_token.clone());

        if let Some(token) = token_opt {
            let _ = state.accesstrade.get_approved_campaigns(&token, true).await;
        }
    }

    Redirect::to("/admin").into_response()
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
        let config = Config {
            telegram_token: "test".to_string(),
            bot_username: "test_bot".to_string(),
            database_path: test_db_path.clone(),
            turso_url: None,
            turso_auth_token: None,
            default_cashback_rate: 70.0,
            default_referral_bonus_rate: 10.0,
            min_withdrawal_vnd: 50000.0,
            accesstrade_deeplink_template: "template".to_string(),
            accesstrade_api_token: None,
            admin_username: "admin".to_string(),
            admin_password: "password".to_string(),
            port: 10000,
        };
        let db = AppDb::init(&config).await.unwrap();
        let state = AppState {
            db,
            config,
            accesstrade: Arc::new(AccessTradeClient::new()),
            sessions: Arc::new(RwLock::new(HashSet::new())),
        };

        let _app: Router = build_router(state);
        let _ = std::fs::remove_file(&test_db_path);
    }
}
