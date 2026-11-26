use super::*;

pub(super) fn extract_cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie_str = headers.get(header::COOKIE)?.to_str().ok()?;
    let prefix = format!("{}=", name);
    for part in cookie_str.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix(&prefix) {
            return Some(val.to_string());
        }
    }
    None
}

/// Dọn bộ đếm Rate Limiter an toàn: ưu tiên xóa các IP đã hết cửa sổ TTL, chỉ cắt 10% entry cũ nhất nếu vẫn vượt ngưỡng, không xóa trắng bộ đếm đang khóa.
pub(super) fn prune_rate_limiter_map(
    map: &mut HashMap<String, (u32, i64)>,
    now_ts: i64,
    window_secs: i64,
    max_entries: usize,
) {
    if map.len() < max_entries {
        return;
    }
    map.retain(|_, (_, ts)| now_ts - *ts < window_secs);
    if map.len() >= max_entries {
        let mut by_age: Vec<(String, i64)> = map
            .iter()
            .map(|(k, (_, ts))| (k.clone(), *ts))
            .collect();
        by_age.sort_by_key(|(_, ts)| *ts);
        let evict_count = (max_entries / 10).max(1);
        for (old_ip, _) in by_age.into_iter().take(evict_count) {
            map.remove(&old_ip);
        }
    }
}

/// Trích xuất session token từ HTTP Cookie header
pub(super) fn extract_session_token(headers: &HeaderMap) -> Option<String> {
    extract_cookie_value(headers, "dealx_session")
}

/// Lấy địa chỉ IP gốc của client qua Cloudflare/Render proxy headers
pub(super) fn extract_client_ip(headers: &HeaderMap) -> String {
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
    if let Some(real_ip) = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return real_ip.to_string();
    }
    "local".to_string()
}

/// Kiểm tra xem yêu cầu hiện tại có quyền Admin hay không (từ phiên hợp lệ hoặc truy cập trực tiếp localhost trên PC)
pub(super) async fn is_authenticated(state: &AppState, headers: &HeaderMap) -> bool {
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
    if let Ok(conn) = state.db.conn()
        && let Ok(true) = Repository::verify_admin_session(&conn, &token).await
    {
        let mut sessions = state.sessions.write().await;
        if sessions.len() >= 100 {
            sessions.clear();
        }
        sessions.insert(token);
        return true;
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
pub(super) async fn handle_login_page(headers: HeaderMap, State(state): State<AppState>) -> Response {
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
    <link href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600;700;800&display=swap" rel="stylesheet">
    <script>
        (function () {
            try {
                if (localStorage.getItem('dealx-theme') === 'dark') {
                    document.documentElement.setAttribute('data-theme', 'dark');
                }
            } catch (e) {}
        })();
    </script>
    <style>
        :root {
            --bg-base: #F4F6F9;
            --bg-card: #FFFFFF;
            --bg-input: #FFFFFF;
            --border-card: #E2E8F0;
            --border-input: #CBD5E1;
            --text-primary: #0F172A;
            --text-secondary: #475569;
            --text-placeholder: #94A3B8;
            --brand-primary: #C2410C;
            --brand-hover: #9A3412;
            --brand-ring: rgba(194, 65, 12, 0.22);
            --brand-contrast: #FFFFFF;
            --card-shadow: 0 12px 36px -8px rgba(15, 23, 42, 0.1), 0 2px 6px rgba(15, 23, 42, 0.04);
        }
        html[data-theme="dark"] {
            --bg-base: #0B1120;
            --bg-card: #131C31;
            --bg-input: #18233C;
            --border-card: rgba(148, 163, 184, 0.18);
            --border-input: rgba(148, 163, 184, 0.28);
            --text-primary: #F8FAFC;
            --text-secondary: #94A3B8;
            --text-placeholder: #64748B;
            --brand-primary: #F97316;
            --brand-hover: #FB923C;
            --brand-ring: rgba(249, 115, 22, 0.26);
            --brand-contrast: #090D16;
            --card-shadow: 0 20px 44px -12px rgba(0, 0, 0, 0.55);
        }
        *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            background-color: var(--bg-base);
            background-image:
                radial-gradient(circle at 50% 0%, rgba(234, 88, 12, 0.07), transparent 55%),
                radial-gradient(circle at 85% 90%, rgba(14, 165, 233, 0.05), transparent 45%);
            font-family: 'Be Vietnam Pro', "Segoe UI", system-ui, -apple-system, sans-serif;
            color: var(--text-primary);
            min-height: 100vh;
            display: flex;
            align-items: center;
            justify-content: center;
            padding: 1.25rem;
        }
        .login-card {
            position: relative;
            background: var(--bg-card);
            border: 1px solid var(--border-card);
            box-shadow: var(--card-shadow);
            border-radius: 1.25rem;
            width: 100%;
            max-width: 27rem;
            padding: 2.25rem 2rem;
        }
        .theme-switch {
            position: absolute;
            top: 1rem;
            right: 1rem;
            min-height: 36px;
            padding: 0.35rem 0.7rem;
            border-radius: 9999px;
            border: 1px solid var(--border-card);
            background: var(--bg-base);
            color: var(--text-secondary);
            font-size: 0.75rem;
            font-weight: 700;
            font-family: inherit;
            cursor: pointer;
            transition: border-color 0.15s, color 0.15s;
        }
        .theme-switch:hover { border-color: var(--brand-primary); color: var(--text-primary); }
        .theme-switch:focus-visible { outline: 2px solid var(--brand-primary); outline-offset: 2px; }
        .login-head { text-align: center; margin-bottom: 1.75rem; }
        .login-logo {
            width: 76px; height: 76px; border-radius: 9999px; object-fit: cover; display: block;
            margin: 0 auto 1rem; border: 2px solid var(--brand-primary);
            box-shadow: 0 6px 18px rgba(194, 65, 12, 0.16);
        }
        .login-title {
            font-size: 1.375rem; font-weight: 800; letter-spacing: -0.02em;
            color: var(--text-primary);
        }
        .login-sub { font-size: 0.85rem; color: var(--text-secondary); margin-top: 0.35rem; font-weight: 500; }
        .login-form { display: flex; flex-direction: column; gap: 1.15rem; }
        .field-label {
            display: block; font-size: 0.75rem; font-weight: 700; text-transform: uppercase;
            letter-spacing: 0.05em; color: var(--text-secondary); margin-bottom: 0.45rem;
        }
        .field-input {
            width: 100%; min-height: 44px; padding: 0.7rem 0.95rem;
            background: var(--bg-input); border: 1px solid var(--border-input);
            border-radius: 0.65rem; color: var(--text-primary); font-size: 0.92rem; font-family: inherit;
            transition: border-color 0.15s, box-shadow 0.15s;
        }
        .field-input::placeholder { color: var(--text-placeholder); }
        .field-input:focus {
            outline: none; border-color: var(--brand-primary); box-shadow: 0 0 0 3px var(--brand-ring);
        }
        .login-btn {
            width: 100%; min-height: 46px; padding: 0.8rem 1rem; margin-top: 0.25rem;
            background: var(--brand-primary);
            border: 1px solid var(--brand-primary); border-radius: 0.65rem;
            color: var(--brand-contrast); font-weight: 700;
            font-size: 0.94rem; font-family: inherit; cursor: pointer;
            transition: background-color 0.15s, transform 0.12s;
        }
        .login-btn:hover { background: var(--brand-hover); border-color: var(--brand-hover); }
        .login-btn:active { transform: scale(0.99); }
        .login-btn:focus-visible { outline: 2px solid var(--brand-primary); outline-offset: 2px; }
        .login-foot {
            margin-top: 1.65rem; padding-top: 1.15rem; border-top: 1px solid var(--border-card);
            text-align: center; font-size: 0.78rem; color: var(--text-secondary); line-height: 1.5;
        }
        @media (prefers-reduced-motion: reduce) {
            *, *::before, *::after { transition: none !important; animation: none !important; }
        }
    </style>
</head>
<body>
    <main class="login-card">
        <button type="button" id="login-theme-btn" class="theme-switch" aria-label="Chuyển đổi giao diện Sáng/Tối">🌙 Tối</button>
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
    </main>
    <script>
        (function () {
            const btn = document.getElementById('login-theme-btn');
            function syncLabel() {
                const isDark = document.documentElement.getAttribute('data-theme') === 'dark';
                if (btn) btn.textContent = isDark ? '☀️ Sáng' : '🌙 Tối';
            }
            syncLabel();
            if (btn) {
                btn.addEventListener('click', function () {
                    const isDark = document.documentElement.getAttribute('data-theme') === 'dark';
                    const next = isDark ? 'light' : 'dark';
                    if (next === 'dark') {
                        document.documentElement.setAttribute('data-theme', 'dark');
                    } else {
                        document.documentElement.removeAttribute('data-theme');
                    }
                    try { localStorage.setItem('dealx-theme', next); } catch (e) {}
                    syncLabel();
                });
            }
        })();
    </script>
</body>
</html>"#;

    Html(html).into_response()
}

fn render_auth_notice_page(
    title: &str,
    heading: &str,
    message_html: &str,
    btn_label: &str,
    is_lock: bool,
) -> String {
    let accent_light = if is_lock { "#B45309" } else { "#B91C1C" };
    let accent_dark = if is_lock { "#FBBF24" } else { "#F87171" };
    let soft_bg_light = if is_lock { "#FEF3C7" } else { "#FEE2E2" };
    let soft_bg_dark = if is_lock { "rgba(251, 191, 36, 0.14)" } else { "rgba(248, 113, 113, 0.14)" };
    let icon_svg = if is_lock {
        r#"<svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>"#
    } else {
        r#"<svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></svg>"#
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}</title>
    <script>
        (function () {{ try {{ if (localStorage.getItem('dealx-theme') === 'dark') document.documentElement.setAttribute('data-theme', 'dark'); }} catch (e) {{}} }})();
    </script>
    <style>
        :root {{
            --bg-base: #F4F6F9; --bg-card: #FFFFFF; --border: #E2E8F0; --text: #0F172A; --muted: #475569;
            --accent: {}; --accent-soft: {}; --btn-bg: #C2410C; --btn-hover: #9A3412; --btn-fg: #FFFFFF;
        }}
        html[data-theme="dark"] {{
            --bg-base: #0B1120; --bg-card: #131C31; --border: rgba(148,163,184,0.22); --text: #F8FAFC; --muted: #94A3B8;
            --accent: {}; --accent-soft: {}; --btn-bg: #F97316; --btn-hover: #FB923C; --btn-fg: #090D16;
        }}
        * {{ box-sizing: border-box; margin: 0; padding: 0; }}
        body {{
            min-height: 100vh; display: flex; align-items: center; justify-content: center; padding: 20px;
            background: var(--bg-base); color: var(--text); font-family: system-ui, -apple-system, sans-serif;
        }}
        .card {{
            width: 100%; max-width: 420px; background: var(--bg-card); border: 1px solid var(--border);
            border-radius: 20px; padding: 30px 24px; text-align: center;
            box-shadow: 0 14px 36px -10px rgba(15, 23, 42, 0.12);
        }}
        .badge-ico {{
            width: 56px; height: 56px; border-radius: 16px; margin: 0 auto 14px;
            display: flex; align-items: center; justify-content: center;
            background: var(--accent-soft); color: var(--accent);
        }}
        h2 {{ font-size: 19px; font-weight: 800; color: var(--accent); margin-bottom: 10px; }}
        p {{ font-size: 14px; color: var(--muted); line-height: 1.6; margin-bottom: 22px; }}
        .btn {{
            display: inline-flex; align-items: center; justify-content: center; min-height: 44px;
            padding: 10px 22px; border-radius: 10px; background: var(--btn-bg); color: var(--btn-fg);
            font-weight: 700; font-size: 14px; text-decoration: none;
        }}
        .btn:hover {{ background: var(--btn-hover); }}
    </style>
</head>
<body>
    <main class="card">
        <div class="badge-ico" aria-hidden="true">{}</div>
        <h2>{}</h2>
        <p>{}</p>
        <a href="/admin/login" class="btn">{}</a>
    </main>
</body>
</html>"#,
        escape_html(title),
        accent_light,
        soft_bg_light,
        accent_dark,
        soft_bg_dark,
        icon_svg,
        escape_html(heading),
        message_html,
        escape_html(btn_label)
    )
}

/// Xử lý xác thực đăng nhập (Chống Brute-force 5 lần / 15 phút & chống Timing Attack)
pub(super) async fn handle_login_submit(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Response {
    let client_ip = extract_client_ip(&headers);
    let now_ts = Utc::now().timestamp();

    {
        let limiter = state.login_limiter.read().await;
        if let Some(&(attempts, last_ts)) = limiter.get(&client_ip)
            && attempts >= 5 && (now_ts - last_ts) < 900 {
                let locked_html = render_auth_notice_page(
                    "Tạm Khóa Đăng Nhập",
                    "Tạm Khóa Do Nhập Sai Nhiều Lần",
                    "Bạn đã nhập sai quá 5 lần. Hệ thống tạm khóa đăng nhập trong 15 phút để bảo vệ tài khoản Quản Trị.",
                    "Quay Lại",
                    true,
                );
                return (StatusCode::TOO_MANY_REQUESTS, Html(locked_html)).into_response();
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
            prune_rate_limiter_map(&mut limiter, now_ts, 900, 2000);
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
        let error_html = render_auth_notice_page(
            "Đăng Nhập Thất Bại",
            "Đăng Nhập Thất Bại",
            &format!(
                "Tài khoản hoặc mật khẩu quản trị viên không chính xác (còn <b>{}</b> lần thử trước khi tạm khóa 15 phút).",
                remaining
            ),
            "Thử Lại",
            false,
        );
        (StatusCode::UNAUTHORIZED, Html(error_html)).into_response()
    }
}

/// Đăng xuất quản trị viên (xoá cả RAM cache và bảng admin_sessions trong DB)
pub(super) async fn handle_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
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
