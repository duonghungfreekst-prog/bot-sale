use std::collections::HashSet;
use std::sync::Arc;
use axum::{
    extract::{Form, Path, State},
    http::{header, HeaderMap, StatusCode},
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
use crate::utils::format_vnd;

#[derive(Clone)]
pub struct AppState {
    pub db: AppDb,
    pub config: Config,
    pub sessions: Arc<RwLock<HashSet<String>>>,
}

/// Khởi chạy Web Admin Dashboard Server
pub async fn start_web_server(port: u16, db: AppDb, config: Config) -> anyhow::Result<()> {
    let state = AppState {
        db,
        config,
        sessions: Arc::new(RwLock::new(HashSet::new())),
    };

    let app = Router::new()
        // Các route công khai
        .route("/", get(handle_root))
        .route("/health", get(handle_health))
        .route("/admin/login", get(handle_login_page).post(handle_login_submit))
        .route("/admin/logout", get(handle_logout))
        
        // Các route quản trị (được bảo vệ bằng phiên đăng nhập Admin)
        .route("/admin", get(handle_dashboard))
        .route("/admin/deals/new", post(handle_add_deal))
        .route("/admin/deals/{id}/toggle", post(handle_toggle_deal))
        .route("/admin/deals/{id}/delete", post(handle_delete_deal))
        
        // Gắn state ứng dụng
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("🚀 Web Admin Dashboard DealX đang chạy tại: http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
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
    let user_match = form.username.as_bytes().ct_eq(state.config.admin_username.as_bytes()).into();
    let pass_match = form.password.as_bytes().ct_eq(state.config.admin_password.as_bytes()).into();

    if user_match && pass_match {
        let token = Uuid::new_v4().to_string();
        {
            let mut sessions = state.sessions.write().await;
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
        error!("❌ Thử đăng nhập trái phép vào Web Admin Dashboard với username: {}", form.username);
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

/// Giao diện chính Dashboard Quản Trị
async fn handle_dashboard(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let conn = match state.db.conn() {
        Ok(c) => c,
        Err(e) => {
            return Html(format!("<h1>Lỗi kết nối cơ sở dữ liệu: {}</h1>", e)).into_response();
        }
    };

    let stats = Repository::get_admin_dashboard_stats(&conn).await.unwrap_or_default();
    let deals = Repository::get_all_deals_for_admin(&conn).await.unwrap_or_default();
    let clicks = Repository::get_recent_clicks_for_admin(&conn, 50).await.unwrap_or_default();

    // Render danh sách deals HTML
    let mut deals_rows = String::new();
    for d in &deals {
        let status_badge = if d.is_active {
            "<span class=\"px-2.5 py-1 bg-green-500/20 text-green-400 rounded-lg text-xs font-semibold\">Đang chạy</span>"
        } else {
            "<span class=\"px-2.5 py-1 bg-gray-500/20 text-gray-400 rounded-lg text-xs font-semibold\">Tạm tắt</span>"
        };

        let coupon = d.coupon_code.as_deref().unwrap_or("Tự động");
        let toggle_text = if d.is_active { "Tắt" } else { "Bật" };

        deals_rows.push_str(&format!(
            r#"<tr class="border-b border-gray-800 hover:bg-gray-800/40 transition">
                <td class="py-3 px-4 text-xs text-gray-400">#{}</td>
                <td class="py-3 px-4 font-bold text-white">{}</td>
                <td class="py-3 px-4 text-sm text-yellow-400 font-mono">{}</td>
                <td class="py-3 px-4 text-sm text-gray-300">{}</td>
                <td class="py-3 px-4 text-xs text-gray-400">{}</td>
                <td class="py-3 px-4">{}</td>
                <td class="py-3 px-4 text-right space-x-2">
                    <form method="POST" action="/admin/deals/{}/toggle" class="inline">
                        <button class="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded-lg text-xs text-white">{}</button>
                    </form>
                    <form method="POST" action="/admin/deals/{}/delete" class="inline" onsubmit="return confirm('Bạn có chắc chắn muốn xoá mã này không?');">
                        <button class="px-3 py-1 bg-red-600/80 hover:bg-red-600 rounded-lg text-xs text-white">Xoá</button>
                    </form>
                </td>
            </tr>"#,
            d.id, d.title, coupon, d.discount_desc, format_vnd(d.min_spend), status_badge, d.id, toggle_text, d.id
        ));
    }

    if deals_rows.is_empty() {
        deals_rows = r#"<tr><td colspan="7" class="py-8 text-center text-gray-500">Chưa có mã giảm giá nào trong kho. Hãy thêm mã mới bên dưới!</td></tr>"#.to_string();
    }

    // Render danh sách clicks gần đây
    let mut clicks_rows = String::new();
    for c in &clicks {
        clicks_rows.push_str(&format!(
            r#"<tr class="border-b border-gray-800 hover:bg-gray-800/40 transition">
                <td class="py-3 px-4 text-xs font-mono text-gray-400">{}</td>
                <td class="py-3 px-4 font-semibold text-white">{}</td>
                <td class="py-3 px-4 text-xs text-gray-400 max-w-xs truncate" title="{}">{}</td>
                <td class="py-3 px-4 text-xs text-blue-400 max-w-xs truncate"><a href="{}" target="_blank" class="hover:underline">Mở Link Affiliate ↗</a></td>
                <td class="py-3 px-4 text-xs text-gray-500 font-mono">{}</td>
            </tr>"#,
            c.user_id, c.platform, c.clean_url, c.clean_url, c.affiliate_url, c.created_at
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
                    <span>Bot: <a href="https://t.me/{}" target="_blank" class="text-yellow-400 hover:underline">@{}</a></span>
                </div>
                <a href="/admin/logout" class="px-3.5 py-1.5 bg-red-600/80 hover:bg-red-600 text-white rounded-xl text-xs font-semibold transition">
                    Đăng Xuất
                </a>
            </div>
        </div>
    </header>

    <main class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 space-y-8">
        <!-- Thống kê tổng quan -->
        <section class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-5">
            <div class="bg-gray-900 border border-gray-800 p-5 rounded-2xl shadow-lg">
                <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Người Dùng Telegram</div>
                <div class="text-3xl font-black text-white">{}</div>
                <div class="text-xs text-green-400 mt-2 flex items-center">
                    <span>👥 Khách đã tương tác bot</span>
                </div>
            </div>

            <div class="bg-gray-900 border border-gray-800 p-5 rounded-2xl shadow-lg">
                <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Tổng Lượt Dán Link / Click</div>
                <div class="text-3xl font-black text-yellow-400">{}</div>
                <div class="text-xs text-gray-400 mt-2 flex items-center">
                    <span>🔗 Đã sinh link Affiliate thành công</span>
                </div>
            </div>

            <div class="bg-gray-900 border border-gray-800 p-5 rounded-2xl shadow-lg">
                <div class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">Kho Mã Giảm Giá Đang Có</div>
                <div class="text-3xl font-black text-orange-400">{}</div>
                <div class="text-xs text-gray-400 mt-2 flex items-center">
                    <span>🎟️ Voucher hiển thị trong lệnh /deals</span>
                </div>
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

        <!-- Thêm mã giảm giá mới -->
        <section class="bg-gray-900 border border-gray-800 rounded-2xl p-6 shadow-xl">
            <h2 class="text-lg font-bold text-white mb-4 flex items-center space-x-2">
                <span>➕</span>
                <span>Thêm Mã Giảm Giá Mới Vào Kho (/deals)</span>
            </h2>
            <form method="POST" action="/admin/deals/new" class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
                <div>
                    <label class="block text-xs font-semibold text-gray-400 mb-1">Sàn Thương Mại</label>
                    <select name="platform" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white focus:outline-none focus:ring-2 focus:ring-yellow-500">
                        <option value="Shopee">Shopee</option>
                        <option value="Lazada">Lazada</option>
                        <option value="TikTok Shop">TikTok Shop</option>
                        <option value="Tiki">Tiki</option>
                    </select>
                </div>

                <div>
                    <label class="block text-xs font-semibold text-gray-400 mb-1">Tiêu Đề Deal</label>
                    <input type="text" name="title" required placeholder="Ví dụ: Giảm 50K Toàn Sàn..." class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white focus:outline-none focus:ring-2 focus:ring-yellow-500">
                </div>

                <div>
                    <label class="block text-xs font-semibold text-gray-400 mb-1">Mã Coupon (Nếu có)</label>
                    <input type="text" name="coupon_code" placeholder="Để trống nếu tự động áp" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white focus:outline-none focus:ring-2 focus:ring-yellow-500">
                </div>

                <div>
                    <label class="block text-xs font-semibold text-gray-400 mb-1">Mô Tả Ưu Đãi</label>
                    <input type="text" name="discount_desc" required placeholder="Ví dụ: Giảm 20% tối đa 100K..." class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white focus:outline-none focus:ring-2 focus:ring-yellow-500">
                </div>

                <div>
                    <label class="block text-xs font-semibold text-gray-400 mb-1">Đơn Tối Thiểu (VNĐ)</label>
                    <input type="number" name="min_spend" required value="0" step="1000" class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white focus:outline-none focus:ring-2 focus:ring-yellow-500">
                </div>

                <div>
                    <label class="block text-xs font-semibold text-gray-400 mb-1">Link Lấy Mã / Sản Phẩm</label>
                    <input type="url" name="deal_url" required placeholder="https://..." class="w-full bg-gray-800 border border-gray-700 rounded-xl px-3 py-2 text-white focus:outline-none focus:ring-2 focus:ring-yellow-500">
                </div>

                <div class="sm:col-span-2 lg:col-span-3 pt-2">
                    <button type="submit" class="px-6 py-2.5 bg-gradient-to-r from-yellow-500 to-orange-500 hover:from-yellow-600 hover:to-orange-600 text-white font-bold rounded-xl shadow-lg transition">
                        Lưu Mã Giảm Giá
                    </button>
                </div>
            </form>
        </section>

        <!-- Bảng quản lý kho deals -->
        <section class="bg-gray-900 border border-gray-800 rounded-2xl overflow-hidden shadow-xl">
            <div class="p-5 border-b border-gray-800 flex justify-between items-center">
                <h2 class="text-lg font-bold text-white flex items-center space-x-2">
                    <span>🎟️</span>
                    <span>Danh Sách Mã Giảm Giá Hiện Có</span>
                </h2>
                <span class="text-xs text-gray-500 font-mono">Tự động đồng bộ với lệnh /deals của Telegram</span>
            </div>
            <div class="overflow-x-auto">
                <table class="w-full text-left">
                    <thead class="bg-gray-800/60 text-xs uppercase text-gray-400 border-b border-gray-800">
                        <tr>
                            <th class="py-3 px-4">ID</th>
                            <th class="py-3 px-4">Tiêu Đề</th>
                            <th class="py-3 px-4">Mã Coupon</th>
                            <th class="py-3 px-4">Giảm Giá</th>
                            <th class="py-3 px-4">Đơn Tối Thiểu</th>
                            <th class="py-3 px-4">Trạng Thái</th>
                            <th class="py-3 px-4 text-right">Thao Tác</th>
                        </tr>
                    </thead>
                    <tbody>
                        {}
                    </tbody>
                </table>
            </div>
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
                            <th class="py-3 px-4">Link Affiliate Sinh Ra</th>
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
        state.config.admin_username,
        state.config.bot_username,
        state.config.bot_username,
        stats.total_users,
        stats.total_clicks,
        stats.total_deals,
        stats.shopee_clicks,
        stats.lazada_clicks,
        stats.tiktok_clicks,
        stats.tiki_clicks,
        deals_rows,
        clicks_rows
    );

    Html(dashboard_html).into_response()
}

/// Form thêm deal mới
#[derive(serde::Deserialize)]
pub struct NewDealForm {
    pub platform: String,
    pub title: String,
    pub coupon_code: Option<String>,
    pub discount_desc: String,
    pub min_spend: f64,
    pub deal_url: String,
}

/// Xử lý thêm Deal mới
async fn handle_add_deal(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<NewDealForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let coupon = form.coupon_code.as_deref().filter(|s| !s.trim().is_empty());
        let _ = Repository::add_deal(
            &conn,
            &form.platform,
            &form.title,
            coupon,
            &form.discount_desc,
            form.min_spend,
            &form.deal_url,
        )
        .await;
    }

    Redirect::to("/admin").into_response()
}

/// Xử lý Bật/Tắt Deal
async fn handle_toggle_deal(
    headers: HeaderMap,
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let _ = Repository::toggle_deal(&conn, id).await;
    }

    Redirect::to("/admin").into_response()
}

/// Xử lý Xoá Deal
async fn handle_delete_deal(
    headers: HeaderMap,
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let _ = Repository::delete_deal(&conn, id).await;
    }

    Redirect::to("/admin").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_session_token() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "dealx_session=test-123-abc; other=xyz".parse().unwrap());
        assert_eq!(extract_session_token(&headers), Some("test-123-abc".to_string()));

        let empty_headers = HeaderMap::new();
        assert_eq!(extract_session_token(&empty_headers), None);
    }

    #[tokio::test]
    async fn test_router_build() {
        let config = Config {
            telegram_token: "test".to_string(),
            bot_username: "test_bot".to_string(),
            database_path: "salehunter.db".to_string(),
            turso_url: None,
            turso_auth_token: None,
            default_cashback_rate: 70.0,
            default_referral_bonus_rate: 10.0,
            min_withdrawal_vnd: 50000.0,
            accesstrade_deeplink_template: "template".to_string(),
            admin_username: "admin".to_string(),
            admin_password: "password".to_string(),
            port: 10000,
        };
        let db = AppDb::init(&config).await.unwrap();
        let state = AppState {
            db,
            config,
            sessions: Arc::new(RwLock::new(HashSet::new())),
        };

        // Kiểm tra xem router có dựng thành công và không panic cú pháp route
        let _app: Router = Router::new()
            .route("/", get(handle_root))
            .route("/health", get(handle_health))
            .route("/admin/login", get(handle_login_page).post(handle_login_submit))
            .route("/admin/logout", get(handle_logout))
            .route("/admin", get(handle_dashboard))
            .route("/admin/deals/new", post(handle_add_deal))
            .route("/admin/deals/{id}/toggle", post(handle_toggle_deal))
            .route("/admin/deals/{id}/delete", post(handle_delete_deal))
            .with_state(state);
    }
}
