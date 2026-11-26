package web

import (
	"crypto/subtle"
	"fmt"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"github.com/google/uuid"

	"salehunter-go/internal/db"
	"salehunter-go/internal/utils"
)

// ExtractCookieValue đọc giá trị cookie theo tên từ HTTP Request.
func ExtractCookieValue(r *http.Request, name string) string {
	if c, err := r.Cookie(name); err == nil {
		return c.Value
	}
	raw := r.Header.Get("Cookie")
	if raw == "" {
		return ""
	}
	prefix := name + "="
	for _, part := range strings.Split(raw, ";") {
		part = strings.TrimSpace(part)
		if strings.HasPrefix(part, prefix) {
			return strings.TrimPrefix(part, prefix)
		}
	}
	return ""
}

// ExtractSessionToken trích xuất session token từ cookie `dealx_session`.
func ExtractSessionToken(r *http.Request) string {
	return ExtractCookieValue(r, "dealx_session")
}

// ExtractClientIP lấy địa chỉ IP gốc của client qua Cloudflare/Render proxy headers.
func ExtractClientIP(r *http.Request) string {
	if ip := strings.TrimSpace(r.Header.Get("Cf-Connecting-Ip")); ip != "" {
		return ip
	}
	if xff := r.Header.Get("X-Forwarded-For"); xff != "" {
		first := strings.TrimSpace(strings.Split(xff, ",")[0])
		if first != "" {
			return first
		}
	}
	if realIP := strings.TrimSpace(r.Header.Get("X-Real-Ip")); realIP != "" {
		return realIP
	}
	return "local"
}

// IsAuthenticated kiểm tra quyền Admin từ session hợp lệ hoặc truy cập localhost PC trusted.
func IsAuthenticated(state *AppState, r *http.Request) bool {
	if IsTrustedLocalAdminHeader(state, r) {
		return true
	}
	token := ExtractSessionToken(r)
	if token == "" {
		return false
	}

	state.SessionsMu.RLock()
	_, inRAM := state.Sessions[token]
	state.SessionsMu.RUnlock()
	if inRAM {
		return true
	}

	if state.DB != nil && state.DB.DB != nil {
		if ok, err := db.VerifyAdminSession(state.DB.DB, token); err == nil && ok {
			state.SessionsMu.Lock()
			if len(state.Sessions) >= 100 {
				state.Sessions = make(map[string]struct{})
			}
			state.Sessions[token] = struct{}{}
			state.SessionsMu.Unlock()
			return true
		}
	}
	return false
}

// HandleLoginPage hiển thị trang đăng nhập Admin (1 tài khoản duy nhất, không đăng ký).
func HandleLoginPage(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin", http.StatusSeeOther)
			return
		}
		html := `<!DOCTYPE html>
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
</html>`
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(html))
	}
}

func renderAuthNoticePage(title, heading, messageHTML, btnLabel string, isLock bool) string {
	accentLight := "#B91C1C"
	accentDark := "#F87171"
	softBgLight := "#FEE2E2"
	softBgDark := "rgba(248, 113, 113, 0.14)"
	iconSVG := `<svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></svg>`
	if isLock {
		accentLight = "#B45309"
		accentDark = "#FBBF24"
		softBgLight = "#FEF3C7"
		softBgDark = "rgba(251, 191, 36, 0.14)"
		iconSVG = `<svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>`
	}

	return fmt.Sprintf(`<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>%s</title>
    <script>
        (function () { try { if (localStorage.getItem('dealx-theme') === 'dark') document.documentElement.setAttribute('data-theme', 'dark'); } catch (e) {} })();
    </script>
    <style>
        :root {
            --bg-base: #F4F6F9; --bg-card: #FFFFFF; --border: #E2E8F0; --text: #0F172A; --muted: #475569;
            --accent: %s; --accent-soft: %s; --btn-bg: #C2410C; --btn-hover: #9A3412; --btn-fg: #FFFFFF;
        }
        html[data-theme="dark"] {
            --bg-base: #0B1120; --bg-card: #131C31; --border: rgba(148,163,184,0.22); --text: #F8FAFC; --muted: #94A3B8;
            --accent: %s; --accent-soft: %s; --btn-bg: #F97316; --btn-hover: #FB923C; --btn-fg: #090D16;
        }
        * { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            min-height: 100vh; display: flex; align-items: center; justify-content: center; padding: 20px;
            background: var(--bg-base); color: var(--text); font-family: system-ui, -apple-system, sans-serif;
        }
        .card {
            width: 100%%; max-width: 420px; background: var(--bg-card); border: 1px solid var(--border);
            border-radius: 20px; padding: 30px 24px; text-align: center;
            box-shadow: 0 14px 36px -10px rgba(15, 23, 42, 0.12);
        }
        .badge-ico {
            width: 56px; height: 56px; border-radius: 16px; margin: 0 auto 14px;
            display: flex; align-items: center; justify-content: center;
            background: var(--accent-soft); color: var(--accent);
        }
        h2 { font-size: 19px; font-weight: 800; color: var(--accent); margin-bottom: 10px; }
        p { font-size: 14px; color: var(--muted); line-height: 1.6; margin-bottom: 22px; }
        .btn {
            display: inline-flex; align-items: center; justify-content: center; min-height: 44px;
            padding: 10px 22px; border-radius: 10px; background: var(--btn-bg); color: var(--btn-fg);
            font-weight: 700; font-size: 14px; text-decoration: none;
        }
        .btn:hover { background: var(--btn-hover); }
    </style>
</head>
<body>
    <main class="card">
        <div class="badge-ico" aria-hidden="true">%s</div>
        <h2>%s</h2>
        <p>%s</p>
        <a href="/admin/login" class="btn">%s</a>
    </main>
</body>
</html>`,
		utils.EscapeHTML(title),
		accentLight,
		softBgLight,
		accentDark,
		softBgDark,
		iconSVG,
		utils.EscapeHTML(heading),
		messageHTML,
		utils.EscapeHTML(btnLabel),
	)
}

// HandleLoginSubmit xử lý xác thực đăng nhập Admin (Chống Brute-force 5 lần / 15 phút & Constant-Time).
func HandleLoginSubmit(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if err := r.ParseForm(); err != nil {
			http.Error(w, "Invalid form data", http.StatusBadRequest)
			return
		}
		username := r.PostFormValue("username")
		password := r.PostFormValue("password")

		clientIP := ExtractClientIP(r)
		nowTS := time.Now().Unix()

		state.LoginLimiterMu.RLock()
		entry, exists := state.LoginLimiter[clientIP]
		state.LoginLimiterMu.RUnlock()
		if exists && entry.Count >= 5 && (nowTS-entry.LastTS) < 900 {
			lockedHTML := renderAuthNoticePage(
				"Tạm Khóa Đăng Nhập",
				"Tạm Khóa Do Nhập Sai Nhiều Lần",
				"Bạn đã nhập sai quá 5 lần. Hệ thống tạm khóa đăng nhập trong 15 phút để bảo vệ tài khoản Quản Trị.",
				"Quay Lại",
				true,
			)
			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(lockedHTML))
			return
		}

		userMatch := subtle.ConstantTimeCompare([]byte(username), []byte(state.Config.AdminUsername)) == 1
		passMatch := subtle.ConstantTimeCompare([]byte(password), []byte(state.Config.AdminPassword)) == 1

		if userMatch && passMatch {
			state.LoginLimiterMu.Lock()
			delete(state.LoginLimiter, clientIP)
			state.LoginLimiterMu.Unlock()

			token := uuid.NewString()
			state.SessionsMu.Lock()
			if len(state.Sessions) >= 100 {
				state.Sessions = make(map[string]struct{})
			}
			state.Sessions[token] = struct{}{}
			state.SessionsMu.Unlock()

			if state.DB != nil && state.DB.DB != nil {
				if err := db.CreateAdminSession(state.DB.DB, token, 86400); err != nil {
					slog.Warn("Không thể lưu admin session vào DB", "err", err)
				}
			}

			slog.Info("✅ Quản trị viên đăng nhập thành công vào Web Dashboard!")
			secureFlag := ""
			if state.Config.PublicBaseURL != "" {
				secureFlag = "; Secure"
			}
			cookieHeader := fmt.Sprintf("dealx_session=%s; HttpOnly; Path=/; SameSite=Lax; Max-Age=86400%s", token, secureFlag)
			w.Header().Add("Set-Cookie", cookieHeader)
			http.Redirect(w, r, "/admin", http.StatusSeeOther)
			return
		}

		state.LoginLimiterMu.Lock()
		PruneRateLimiterMap(state.LoginLimiter, nowTS, 900, 2000)
		cur := state.LoginLimiter[clientIP]
		if cur.LastTS == 0 || (nowTS-cur.LastTS) >= 900 {
			cur = RateEntry{Count: 1, LastTS: nowTS}
		} else {
			cur.Count++
			cur.LastTS = nowTS
		}
		state.LoginLimiter[clientIP] = cur
		state.LoginLimiterMu.Unlock()

		var remaining uint32
		if cur.Count < 5 {
			remaining = 5 - cur.Count
		}

		slog.Error("❌ Thử đăng nhập trái phép vào Web Admin Dashboard",
			"ip", utils.SanitizeCRLF(clientIP),
			"attempts", cur.Count,
		)
		time.Sleep(500 * time.Millisecond)

		errHTML := renderAuthNoticePage(
			"Đăng Nhập Thất Bại",
			"Đăng Nhập Thất Bại",
			fmt.Sprintf("Tài khoản hoặc mật khẩu quản trị viên không chính xác (còn <b>%d</b> lần thử trước khi tạm khóa 15 phút).", remaining),
			"Thử Lại",
			false,
		)
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.WriteHeader(http.StatusUnauthorized)
		_, _ = w.Write([]byte(errHTML))
	}
}

// HandleLogout đăng xuất quản trị viên (xóa cả RAM cache và DB).
func HandleLogout(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if token := ExtractSessionToken(r); token != "" {
			state.SessionsMu.Lock()
			delete(state.Sessions, token)
			state.SessionsMu.Unlock()
			if state.DB != nil && state.DB.DB != nil {
				_ = db.DeleteAdminSession(state.DB.DB, token)
			}
		}
		secureFlag := ""
		if state.Config.PublicBaseURL != "" {
			secureFlag = "; Secure"
		}
		w.Header().Add("Set-Cookie", "dealx_session=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0"+secureFlag)
		http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
	}
}
