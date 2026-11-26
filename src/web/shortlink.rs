use super::*;

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

pub(super) fn is_desktop_browser(headers: &HeaderMap) -> bool {
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

pub(super) fn is_mobile_browser(headers: &HeaderMap) -> bool {
    let sec_mobile = headers
        .get("sec-ch-ua-mobile")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if sec_mobile == "?1" {
        return true;
    }
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    ua.contains("android")
        || ua.contains("iphone")
        || ua.contains("ipad")
        || ua.contains("ipod")
        || ua.contains("mobile")
        || ua.contains("telegram")
        || ua.contains("zalo")
        || ua.contains("fban")
        || ua.contains("fbav")
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
pub(super) fn decode_bridge_payload(payload_hex: &str, key_hex: &str) -> Option<String> {
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
pub(super) async fn handle_bridge_js() -> Response {
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
pub(super) async fn handle_shortlink_redirect(
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
    <script>
    (function(){try{var t=localStorage.getItem('dealx-theme')||'light';if(t==='dark')document.documentElement.setAttribute('data-theme','dark');}catch(_){}})();
    </script>
    <style>
        :root { --bg:#F4F6F9; --card:#FFFFFF; --border:#E2E8F0; --text:#0F172A; --muted:#475569; --accent:#C2410C; --accent-soft:rgba(194,65,12,0.1); }
        html[data-theme="dark"] { --bg:#0B1120; --card:#131C31; --border:#1E293B; --text:#F1F5F9; --muted:#94A3B8; --accent:#F97316; --accent-soft:rgba(249,115,22,0.14); }
        * { box-sizing:border-box; }
        body { margin:0; min-height:100vh; display:flex; align-items:center; justify-content:center; background:var(--bg); color:var(--text); font-family:system-ui,-apple-system,sans-serif; padding:24px; text-align:center; }
        .card { max-width:420px; width:100%; background:var(--card); border:1px solid var(--border); border-radius:20px; padding:28px 24px; box-shadow:0 12px 28px rgba(15,23,42,0.08); }
        .icon-badge { width:56px; height:56px; margin:0 auto 14px; border-radius:16px; display:flex; align-items:center; justify-content:center; background:var(--accent-soft); color:var(--accent); }
        h1 { margin:0 0 10px; font-size:18px; line-height:1.35; }
        p { margin:0; color:var(--muted); font-size:14px; line-height:1.6; }
    </style>
</head>
<body>
    <main class="card">
        <div class="icon-badge" aria-hidden="true">
            <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="2" width="14" height="20" rx="2" ry="2"/><line x1="12" y1="18" x2="12.01" y2="18"/></svg>
        </div>
        <h1>Link ưu đãi chỉ mở được trên Điện Thoại</h1>
        <p>Link ưu đãi này chỉ mở được trên Điện Thoại (iOS / Android) để tự động chuyển vào App sàn &amp; kích hoạt ưu đãi. Vui lòng bấm mở từ điện thoại của bạn!</p>
    </main>
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

    // Trên máy tính PC khi Admin tắt mobile_only, cho 302 trực tiếp để test nhanh; còn trên điện thoại luôn qua Cầu nối Mobile thông minh!
    if !mobile_only && !is_mobile_browser(&headers) {
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
    let mobile_only_flag = if mobile_only { "1" } else { "0" };
    let bridge_html = format!(
        r##"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="robots" content="noindex, nofollow">
    <meta name="referrer" content="unsafe-url">
    <title>Đang chuyển sang {}...</title>
    <script>
    (function(){{try{{var t=localStorage.getItem('dealx-theme')||'light';if(t==='dark')document.documentElement.setAttribute('data-theme','dark');}}catch(_){{}}}})();
    </script>
    <style>
        :root {{
            --bg: #F4F6F9; --card: #FFFFFF; --inset: #F8FAFC; --border: #E2E8F0;
            --text: #0F172A; --muted: #475569; --accent: #C2410C; --accent-hover: #9A3412;
            --accent-contrast: #FFFFFF; --accent-soft: rgba(194, 65, 12, 0.10);
            --tip-bg: rgba(234, 88, 12, 0.08); --tip-border: rgba(194, 65, 12, 0.28); --tip-text: #9A3412;
            --shadow: 0 18px 36px -12px rgba(15, 23, 42, 0.12), 0 2px 6px rgba(15, 23, 42, 0.04);
        }}
        html[data-theme="dark"] {{
            --bg: #0B1120; --card: #131C31; --inset: #0F172A; --border: #1E293B;
            --text: #F8FAFC; --muted: #94A3B8; --accent: #F97316; --accent-hover: #FB923C;
            --accent-contrast: #090D16; --accent-soft: rgba(249, 115, 22, 0.16);
            --tip-bg: rgba(249, 115, 22, 0.12); --tip-border: rgba(249, 115, 22, 0.35); --tip-text: #FDBA74;
            --shadow: 0 20px 40px -12px rgba(0, 0, 0, 0.55);
        }}
        * {{ box-sizing: border-box; }}
        body {{
            margin: 0; min-height: 100vh; display: flex; align-items: center; justify-content: center;
            background: radial-gradient(circle at 50% 0%, var(--accent-soft), transparent 55%), var(--bg);
            color: var(--text); font-family: system-ui, -apple-system, sans-serif; padding: 20px;
        }}
        .card {{
            width: 100%; max-width: 420px; background: var(--card); border: 1px solid var(--border);
            border-radius: 20px; padding: 28px 24px; text-align: center; box-shadow: var(--shadow);
        }}
        .spinner-ring {{
            position: relative; width: 52px; height: 52px; margin: 0 auto 16px;
            display: flex; align-items: center; justify-content: center;
            border-radius: 16px; background: var(--accent-soft); color: var(--accent);
        }}
        .spinner {{
            width: 32px; height: 32px; border: 3px solid var(--border);
            border-top-color: var(--accent); border-radius: 50%; animation: spin 0.8s linear infinite;
        }}
        @keyframes spin {{ to {{ transform: rotate(360deg); }} }}
        @media (prefers-reduced-motion: reduce) {{ .spinner {{ animation: none; border-color: var(--accent); opacity: 0.7; }} }}
        h1 {{ margin: 0 0 10px; font-size: 18px; line-height: 1.35; font-weight: 800; }}
        p {{ margin: 0; color: var(--muted); font-size: 14px; line-height: 1.6; }}
        .hidden {{ display: none; }}
        .rescue {{ margin-top: 18px; display: flex; flex-direction: column; gap: 10px; }}
        .btn-open, .btn-copy {{
            display: inline-flex; align-items: center; justify-content: center; gap: 8px;
            border-radius: 12px; font-weight: 700; cursor: pointer; text-decoration: none;
        }}
        .btn-open {{
            min-height: 48px; background: var(--accent); color: var(--accent-contrast); padding: 12px 16px;
        }}
        .btn-open:hover {{ background: var(--accent-hover); }}
        .btn-copy {{
            min-height: 44px; background: var(--inset); color: var(--text); border: 1px solid var(--border);
            padding: 10px 14px; font-weight: 600;
        }}
        .btn-open:focus-visible, .btn-copy:focus-visible {{ outline: 2px solid var(--accent); outline-offset: 2px; }}
        .tip {{
            display: none; font-size: 12.5px; color: var(--tip-text); background: var(--tip-bg);
            border: 1px solid var(--tip-border); border-radius: 10px; padding: 11px 12px; text-align: left; line-height: 1.5;
        }}
    </style>
</head>
<body data-p="{}" data-k="{}" data-mobile-only="{}">
    <main class="card">
        <div class="spinner-ring" aria-hidden="true">
            <div id="bridge-spinner" class="spinner"></div>
        </div>
        <h1 id="bridge-title">Đang mở {}...</h1>
        <p id="bridge-desc">Hệ thống đang chuyển bạn sang ứng dụng để kích hoạt mã giảm giá.</p>
        <div id="rescue-box" class="rescue hidden">
            <a id="rescue-open-btn" class="btn-open" href="#" rel="noopener">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 2 3 6v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6l-3-4z"/><line x1="3" y1="6" x2="21" y2="6"/><path d="M16 10a4 4 0 0 1-8 0"/></svg>
                <span>Bấm Để Mở Ứng Dụng Ngay</span>
            </a>
            <button id="rescue-copy-btn" class="btn-copy" type="button">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                <span>Sao chép link mở bằng Safari/Chrome</span>
            </button>
            <div id="webview-tip" class="tip">
                <strong>Mẹo nếu mở từ Zalo / Messenger:</strong> Bấm dấu <b>⋮</b> (hoặc <b>⋯</b>) ở góc màn hình rồi chọn <b>“Mở bằng trình duyệt mặc định”</b> để tự động bật App sàn.
            </div>
        </div>
    </main>
    <script src="/assets/bridge.js" defer></script>
</body>
</html>"##,
        escape_html(&plat_label),
        escape_html(&payload_hex),
        escape_html(&key_hex),
        mobile_only_flag,
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
