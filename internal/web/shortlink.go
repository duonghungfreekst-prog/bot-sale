package web

import (
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"net/http"
	"strings"

	"salehunter-go/internal/db"
	"salehunter-go/internal/utils"
)

func isPreviewBot(r *http.Request) bool {
	ua := strings.ToLower(r.Header.Get("User-Agent"))
	return strings.Contains(ua, "telegrambot") ||
		strings.Contains(ua, "twitterbot") ||
		strings.Contains(ua, "facebookexternalhit") ||
		strings.Contains(ua, "whatsapp") ||
		strings.Contains(ua, "discordbot") ||
		strings.Contains(ua, "slackbot") ||
		strings.Contains(ua, "zalo")
}

// IsDesktopBrowser kiểm tra client có phải trình duyệt máy tính hay không.
func IsDesktopBrowser(r *http.Request) bool {
	secMobile := strings.TrimSpace(r.Header.Get("Sec-Ch-Ua-Mobile"))
	if secMobile == "?0" {
		return true
	}
	secPlat := strings.ToLower(r.Header.Get("Sec-Ch-Ua-Platform"))
	if strings.Contains(secPlat, "windows") ||
		strings.Contains(secPlat, "macos") ||
		strings.Contains(secPlat, "linux") ||
		strings.Contains(secPlat, "chrome os") {
		return true
	}
	ua := strings.ToLower(r.Header.Get("User-Agent"))
	isMobileUA := strings.Contains(ua, "android") ||
		strings.Contains(ua, "iphone") ||
		strings.Contains(ua, "ipad") ||
		strings.Contains(ua, "ipod") ||
		strings.Contains(ua, "mobile")
	return !isMobileUA &&
		(strings.Contains(ua, "windows nt") || strings.Contains(ua, "macintosh") || strings.Contains(ua, "x11; linux"))
}

// IsMobileBrowser kiểm tra client có phải trình duyệt điện thoại hay không.
func IsMobileBrowser(r *http.Request) bool {
	if strings.TrimSpace(r.Header.Get("Sec-Ch-Ua-Mobile")) == "?1" {
		return true
	}
	ua := strings.ToLower(r.Header.Get("User-Agent"))
	return strings.Contains(ua, "android") ||
		strings.Contains(ua, "iphone") ||
		strings.Contains(ua, "ipad") ||
		strings.Contains(ua, "ipod") ||
		strings.Contains(ua, "mobile") ||
		strings.Contains(ua, "telegram") ||
		strings.Contains(ua, "zalo") ||
		strings.Contains(ua, "fban") ||
		strings.Contains(ua, "fbav")
}

// EncodeBridgePayload mã hóa XOR link đích bằng khóa ngẫu nhiên 8 byte để không lộ URL thô trong HTML.
func EncodeBridgePayload(targetURL string) (string, string) {
	key := make([]byte, 8)
	if _, err := rand.Read(key); err != nil {
		copy(key, []byte("dlx2026!"))
	}
	raw := []byte(targetURL)
	xored := make([]byte, len(raw))
	for i, b := range raw {
		xored[i] = b ^ key[i%len(key)]
	}
	return hex.EncodeToString(xored), hex.EncodeToString(key)
}

// DecodeBridgePayload giải mã payload XOR (chủ yếu phục vụ unit test & kiểm chứng).
func DecodeBridgePayload(payloadHex, keyHex string) (string, bool) {
	if payloadHex == "" || keyHex == "" || len(payloadHex)%2 != 0 || len(keyHex)%2 != 0 {
		return "", false
	}
	key, err := hex.DecodeString(keyHex)
	if err != nil || len(key) == 0 {
		return "", false
	}
	raw, err := hex.DecodeString(payloadHex)
	if err != nil {
		return "", false
	}
	out := make([]byte, len(raw))
	for i, b := range raw {
		out[i] = b ^ key[i%len(key)]
	}
	return string(out), true
}

// HandleBridgeJS phục vụ JS ngoài cho Cầu nối Mobile (`/assets/bridge.js`).
func HandleBridgeJS(w http.ResponseWriter, _ *http.Request) {
	js := `function isRealMobile() {
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
})();`
	w.Header().Set("Content-Type", "application/javascript; charset=utf-8")
	w.Header().Set("Cache-Control", "public, max-age=300")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte(js))
}

// HandleShortlinkRedirect giải mã link bọc `/{code}` & `/go/{code}` với 2 lớp bảo vệ + XOR Bridge.
func HandleShortlinkRedirect(state *AppState, code string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		c := strings.TrimSpace(code)
		if !db.IsValidShortCode(c) {
			http.NotFound(w, r)
			return
		}

		record, err := state.DB.ResolveShortLinkCached(c)
		if err != nil || record == nil {
			http.NotFound(w, r)
			return
		}

		if isPreviewBot(r) {
			platDesc := strings.TrimSpace(record.Platform)
			if platDesc == "" {
				platDesc = "ứng dụng mua sắm"
			}
			titleDisplay := strings.TrimSpace(record.Title)
			if titleDisplay == "" {
				titleDisplay = "Ưu đãi " + platDesc + " hôm nay"
			}
			previewHTML := fmt.Sprintf(`<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="utf-8">
    <meta name="robots" content="noindex, nofollow">
    <meta property="og:title" content="%s">
    <meta property="og:description" content="Bấm để mở ưu đãi trên %s">
    <title>%s</title>
</head>
<body><p>Đang mở liên kết ưu đãi.</p></body>
</html>`,
				utils.EscapeHTML(titleDisplay),
				utils.EscapeHTML(platDesc),
				utils.EscapeHTML(titleDisplay),
			)
			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(previewHTML))
			return
		}

		settingsMap, _ := state.DB.GetSettingsMapCached()
		mobileOnly := strings.TrimSpace(settingsMap["shortlink_mobile_only"]) == "1"

		if mobileOnly && IsDesktopBrowser(r) {
			blockedHTML := `<!DOCTYPE html>
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
</html>`
			w.Header().Set("Cache-Control", "no-store")
			w.Header().Set("X-Robots-Tag", "noindex, nofollow")
			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			w.WriteHeader(http.StatusForbidden)
			_, _ = w.Write([]byte(blockedHTML))
			return
		}

		if state.DB != nil {
			state.DB.RecordShortLinkClickBuffered(c)
		}

		if !mobileOnly && !IsMobileBrowser(r) {
			w.Header().Set("Location", record.TargetURL)
			w.Header().Set("Referrer-Policy", "unsafe-url")
			w.Header().Set("Cache-Control", "no-store, no-cache, must-revalidate, private")
			w.Header().Set("X-Robots-Tag", "noindex, nofollow")
			w.WriteHeader(http.StatusFound)
			return
		}

		payloadHex, keyHex := EncodeBridgePayload(record.TargetURL)
		platLabel := strings.TrimSpace(record.Platform)
		if platLabel == "" {
			platLabel = "ứng dụng mua sắm"
		}
		mobileOnlyFlag := "0"
		if mobileOnly {
			mobileOnlyFlag = "1"
		}

		bridgeHTML := fmt.Sprintf(`<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="robots" content="noindex, nofollow">
    <meta name="referrer" content="unsafe-url">
    <title>Đang chuyển sang %s...</title>
    <script>
    (function(){try{var t=localStorage.getItem('dealx-theme')||'light';if(t==='dark')document.documentElement.setAttribute('data-theme','dark');}catch(_){}})();
    </script>
    <style>
        :root {
            --bg: #F4F6F9; --card: #FFFFFF; --inset: #F8FAFC; --border: #E2E8F0;
            --text: #0F172A; --muted: #475569; --accent: #C2410C; --accent-hover: #9A3412;
            --accent-contrast: #FFFFFF; --accent-soft: rgba(194, 65, 12, 0.10);
            --tip-bg: rgba(234, 88, 12, 0.08); --tip-border: rgba(194, 65, 12, 0.28); --tip-text: #9A3412;
            --shadow: 0 18px 36px -12px rgba(15, 23, 42, 0.12), 0 2px 6px rgba(15, 23, 42, 0.04);
        }
        html[data-theme="dark"] {
            --bg: #0B1120; --card: #131C31; --inset: #0F172A; --border: #1E293B;
            --text: #F8FAFC; --muted: #94A3B8; --accent: #F97316; --accent-hover: #FB923C;
            --accent-contrast: #090D16; --accent-soft: rgba(249, 115, 22, 0.16);
            --tip-bg: rgba(249, 115, 22, 0.12); --tip-border: rgba(249, 115, 22, 0.35); --tip-text: #FDBA74;
            --shadow: 0 20px 40px -12px rgba(0, 0, 0, 0.55);
        }
        * { box-sizing: border-box; }
        body {
            margin: 0; min-height: 100vh; display: flex; align-items: center; justify-content: center;
            background: radial-gradient(circle at 50%% 0%%, var(--accent-soft), transparent 55%%), var(--bg);
            color: var(--text); font-family: system-ui, -apple-system, sans-serif; padding: 20px;
        }
        .card {
            width: 100%%; max-width: 420px; background: var(--card); border: 1px solid var(--border);
            border-radius: 20px; padding: 28px 24px; text-align: center; box-shadow: var(--shadow);
        }
        .spinner-ring {
            position: relative; width: 52px; height: 52px; margin: 0 auto 16px;
            display: flex; align-items: center; justify-content: center;
            border-radius: 16px; background: var(--accent-soft); color: var(--accent);
        }
        .spinner {
            width: 32px; height: 32px; border: 3px solid var(--border);
            border-top-color: var(--accent); border-radius: 50%%; animation: spin 0.8s linear infinite;
        }
        @keyframes spin { to { transform: rotate(360deg); } }
        @media (prefers-reduced-motion: reduce) { .spinner { animation: none; border-color: var(--accent); opacity: 0.7; } }
        h1 { margin: 0 0 10px; font-size: 18px; line-height: 1.35; font-weight: 800; }
        p { margin: 0; color: var(--muted); font-size: 14px; line-height: 1.6; }
        .hidden { display: none; }
        .rescue { margin-top: 18px; display: flex; flex-direction: column; gap: 10px; }
        .btn-open, .btn-copy {
            display: inline-flex; align-items: center; justify-content: center; gap: 8px;
            border-radius: 12px; font-weight: 700; cursor: pointer; text-decoration: none;
        }
        .btn-open {
            min-height: 48px; background: var(--accent); color: var(--accent-contrast); padding: 12px 16px;
        }
        .btn-open:hover { background: var(--accent-hover); }
        .btn-copy {
            min-height: 44px; background: var(--inset); color: var(--text); border: 1px solid var(--border);
            padding: 10px 14px; font-weight: 600;
        }
        .btn-open:focus-visible, .btn-copy:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
        .tip {
            display: none; font-size: 12.5px; color: var(--tip-text); background: var(--tip-bg);
            border: 1px solid var(--tip-border); border-radius: 10px; padding: 11px 12px; text-align: left; line-height: 1.5;
        }
    </style>
</head>
<body data-p="%s" data-k="%s" data-mobile-only="%s">
    <main class="card">
        <div class="spinner-ring" aria-hidden="true">
            <div id="bridge-spinner" class="spinner"></div>
        </div>
        <h1 id="bridge-title">Đang mở %s...</h1>
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
</html>`,
			utils.EscapeHTML(platLabel),
			utils.EscapeHTML(payloadHex),
			utils.EscapeHTML(keyHex),
			mobileOnlyFlag,
			utils.EscapeHTML(platLabel),
		)

		w.Header().Set("Referrer-Policy", "unsafe-url")
		w.Header().Set("Cache-Control", "no-store, no-cache, must-revalidate, private")
		w.Header().Set("X-Robots-Tag", "noindex, nofollow")
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(bridgeHTML))
	}
}
