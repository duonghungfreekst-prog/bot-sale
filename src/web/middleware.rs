use super::*;

/// Bổ sung các HTTP Security Headers bắt buộc cho mọi phản hồi Web Admin
pub(super) async fn security_headers_middleware(req: Request, next: Next) -> Response {
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
        HeaderValue::from_static("default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com data:; img-src 'self' data: https:; connect-src 'self' https:;"),
    );
    res
}

pub(super) fn forbidden_csrf_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        "Forbidden: Yêu cầu không hợp lệ hoặc thiếu xác thực CSRF/Same-Origin.",
    )
        .into_response()
}

/// Kiểm tra Same-Origin (`Sec-Fetch-Site`, `Origin`, `Referer`) và Double-Submit CSRF Token cho các request `POST /admin/*`.
pub(super) async fn admin_csrf_middleware(req: Request, next: Next) -> Response {
    let is_admin_post = req.method() == axum::http::Method::POST && req.uri().path().starts_with("/admin");
    if !is_admin_post {
        return next.run(req).await;
    }

    let req_host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .and_then(extract_host_only);

    if let Some(fetch_site) = req
        .headers()
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_ascii_lowercase())
        && !matches!(fetch_site.as_str(), "same-origin" | "none")
    {
        return forbidden_csrf_response();
    }

    if let Some(origin_raw) = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        if origin_raw.trim().eq_ignore_ascii_case("null") {
            return forbidden_csrf_response();
        }
        let origin_host = extract_host_only(origin_raw);
        if origin_host.is_none() || (req_host.is_some() && origin_host != req_host) {
            return forbidden_csrf_response();
        }
    } else if let Some(ref_raw) = req.headers().get(header::REFERER).and_then(|v| v.to_str().ok()) {
        let ref_host = extract_host_only(ref_raw);
        if ref_host.is_none() || (req_host.is_some() && ref_host != req_host) {
            return forbidden_csrf_response();
        }
    }

    if req.uri().path() == "/admin/login" {
        return next.run(req).await;
    }

    let cookie_csrf = extract_cookie_value(req.headers(), "dealx_csrf");
    let has_session_cookie = extract_session_token(req.headers()).is_some();
    let header_csrf = req
        .headers()
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if cookie_csrf.is_none() && !has_session_cookie && header_csrf.is_none() {
        return next.run(req).await;
    }

    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 262_144).await else {
        return forbidden_csrf_response();
    };

    let submitted_csrf = header_csrf.or_else(|| {
        url::form_urlencoded::parse(&bytes).find_map(|(k, v)| {
            if k == "_csrf" && !v.trim().is_empty() {
                Some(v.into_owned())
            } else {
                None
            }
        })
    });

    let valid_csrf = match (cookie_csrf.as_deref(), submitted_csrf.as_deref()) {
        (Some(c), Some(s)) if !c.is_empty() => bool::from(c.as_bytes().ct_eq(s.as_bytes())),
        _ => false,
    };

    if !valid_csrf {
        return forbidden_csrf_response();
    }

    let rebuilt = Request::from_parts(parts, axum::body::Body::from(bytes));
    next.run(rebuilt).await
}


/// Phản hồi chuẩn 404 Not Found cho tên miền bọc link hoặc chế độ ẩn web chính
pub(super) fn not_found_cloak_response() -> Response {
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
pub(super) fn extract_host_only(raw_host_or_url: &str) -> Option<String> {
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
pub(super) fn is_allowed_shortlink_cloak_path(method: &axum::http::Method, path: &str) -> bool {
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
pub(super) async fn is_public_home_cloaked(state: &AppState) -> bool {
    state
        .db
        .get_settings_map_cached()
        .await
        .ok()
        .and_then(|m| m.get("public_home_cloak").cloned())
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

pub(super) const LOCAL_PC_TRUSTED_HEADER: &str = "x-dealx-local-trusted";

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
    if let Some(ip) = peer_ip
        && !ip.is_loopback() {
            return false;
        }
    is_loopback_host(headers)
}

pub(super) fn is_trusted_local_admin_header(state: &AppState, headers: &HeaderMap) -> bool {
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
pub(super) async fn shortlink_cloak_middleware(
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
                let db_short_raw = state
                    .db
                    .get_settings_map_cached()
                    .await
                    .ok()
                    .and_then(|m| m.get("shortlink_base_url").cloned())
                    .filter(|s| !s.trim().is_empty());
                let short_host = crate::config::resolve_effective_shortlink_base_url(
                    db_short_raw.as_deref(),
                    state.config.shortlink_base_url.as_deref(),
                    None,
                )
                .as_deref()
                .and_then(extract_host_only);
                if rh.ends_with(".trycloudflare.com") && public_host.is_none() {
                    true
                } else {
                    match short_host {
                        Some(sh) => {
                            let origin_sub = format!("origin.{}", sh);
                            (sh == rh || rh == origin_sub)
                                && public_host.as_deref() != Some(sh.as_str())
                                && !(public_host.is_none() && rh.ends_with(".onrender.com"))
                        }
                        None => false,
                    }
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

