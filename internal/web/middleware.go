package web

import (
	"bytes"
	"crypto/subtle"
	"io"
	"net"
	"net/http"
	"net/url"
	"sort"
	"strings"
	"sync"

	"salehunter-go/internal/bot"
	"salehunter-go/internal/config"
	"salehunter-go/internal/db"
	"salehunter-go/internal/services/accesstrade"
	"salehunter-go/internal/services/parser"
)

const LocalPCTrustedHeader = "X-Dealx-Local-Trusted"

// RateEntry lưu bộ đếm và mốc thời gian gần nhất của 1 IP trong Rate Limiter.
type RateEntry struct {
	Count  uint32
	LastTS int64
}

// AppState giữ toàn bộ dependencies chia sẻ cho các HTTP handlers Web Public, Admin & Shortlink.
type AppState struct {
	Config      config.Config
	DB          *db.AppDb
	AccessTrade *accesstrade.AccessTradeClient
	Parser      *parser.DealParser
	WebhookCh   chan<- bot.Update // nil khi chạy Long Polling

	SessionsMu sync.RWMutex
	Sessions   map[string]struct{}

	LoginLimiterMu sync.RWMutex
	LoginLimiter   map[string]RateEntry

	ConvertLimiterMu sync.RWMutex
	ConvertLimiter   map[string]RateEntry
}

// NewAppState khởi tạo AppState dùng chung giữa router và background tasks.
func NewAppState(
	cfg config.Config,
	appDB *db.AppDb,
	atClient *accesstrade.AccessTradeClient,
	webhookCh chan<- bot.Update,
) *AppState {
	return &AppState{
		Config:         cfg,
		DB:             appDB,
		AccessTrade:    atClient,
		Parser:         parser.WithAccessTrade(atClient),
		WebhookCh:      webhookCh,
		Sessions:       make(map[string]struct{}),
		LoginLimiter:   make(map[string]RateEntry),
		ConvertLimiter: make(map[string]RateEntry),
	}
}

// PruneRateLimiterMap dọn bộ đếm Rate Limiter an toàn theo TTL và giới hạn số entry.
func PruneRateLimiterMap(m map[string]RateEntry, nowTS, windowSecs int64, maxEntries int) {
	if len(m) < maxEntries {
		return
	}
	for k, v := range m {
		if nowTS-v.LastTS >= windowSecs {
			delete(m, k)
		}
	}
	if len(m) >= maxEntries {
		type agePair struct {
			ip string
			ts int64
		}
		byAge := make([]agePair, 0, len(m))
		for k, v := range m {
			byAge = append(byAge, agePair{ip: k, ts: v.LastTS})
		}
		sort.Slice(byAge, func(i, j int) bool {
			return byAge[i].ts < byAge[j].ts
		})
		evictCount := maxEntries / 10
		if evictCount < 1 {
			evictCount = 1
		}
		for i := 0; i < len(byAge) && i < evictCount; i++ {
			delete(m, byAge[i].ip)
		}
	}
}

// SecurityHeadersMiddleware bổ sung HTTP Security Headers bắt buộc cho mọi phản hồi.
func SecurityHeadersMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		h := w.Header()
		h.Set("X-Content-Type-Options", "nosniff")
		h.Set("X-Frame-Options", "DENY")
		h.Set("Strict-Transport-Security", "max-age=31536000")
		h.Set("X-XSS-Protection", "1; mode=block")
		h.Set("Permissions-Policy", "camera=(), microphone=(), geolocation=()")
		h.Set("Cross-Origin-Opener-Policy", "same-origin")
		h.Set(
			"Content-Security-Policy",
			"default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com data:; img-src 'self' data: https:; connect-src 'self' https:;",
		)
		next.ServeHTTP(w, r)
	})
}

func forbiddenCSRFResponse(w http.ResponseWriter) {
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.WriteHeader(http.StatusForbidden)
	_, _ = w.Write([]byte("Forbidden: Yêu cầu không hợp lệ hoặc thiếu xác thực CSRF/Same-Origin."))
}

// AdminCSRFMiddleware kiểm tra Same-Origin (Sec-Fetch-Site, Origin, Referer) và Double-Submit CSRF Token cho POST /admin/*.
func AdminCSRFMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || !strings.HasPrefix(r.URL.Path, "/admin") {
			next.ServeHTTP(w, r)
			return
		}

		reqHost := ExtractHostOnly(r.Host)

		if fetchSite := strings.ToLower(strings.TrimSpace(r.Header.Get("Sec-Fetch-Site"))); fetchSite != "" {
			if fetchSite != "same-origin" && fetchSite != "none" {
				forbiddenCSRFResponse(w)
				return
			}
		}

		if originRaw := r.Header.Get("Origin"); originRaw != "" {
			if strings.EqualFold(strings.TrimSpace(originRaw), "null") {
				forbiddenCSRFResponse(w)
				return
			}
			originHost := ExtractHostOnly(originRaw)
			if originHost == "" || (reqHost != "" && originHost != reqHost) {
				forbiddenCSRFResponse(w)
				return
			}
		} else if refRaw := r.Header.Get("Referer"); refRaw != "" {
			refHost := ExtractHostOnly(refRaw)
			if refHost == "" || (reqHost != "" && refHost != reqHost) {
				forbiddenCSRFResponse(w)
				return
			}
		}

		if r.URL.Path == "/admin/login" {
			next.ServeHTTP(w, r)
			return
		}

		cookieCSRF := ExtractCookieValue(r, "dealx_csrf")
		hasSessionCookie := ExtractSessionToken(r) != ""
		isTrustedLocal := r.Header.Get(LocalPCTrustedHeader) == "1"
		headerCSRF := strings.TrimSpace(r.Header.Get("X-CSRF-Token"))

		// Chỉ để lọt các request chưa đăng nhập từ môi trường Cloud/Remote (handler sẽ redirect /admin/login).
		// Nếu đã có session HOẶC đang dùng luồng PC Local miễn đăng nhập thì bắt buộc phải hợp lệ Double-Submit CSRF.
		if !hasSessionCookie && !isTrustedLocal {
			next.ServeHTTP(w, r)
			return
		}

		const maxBodyBytes = 262144
		bodyBytes, err := io.ReadAll(io.LimitReader(r.Body, maxBodyBytes+1))
		if err != nil || len(bodyBytes) > maxBodyBytes {
			forbiddenCSRFResponse(w)
			return
		}
		r.Body = io.NopCloser(bytes.NewReader(bodyBytes))

		submittedCSRF := headerCSRF
		if submittedCSRF == "" {
			if formVals, pErr := url.ParseQuery(string(bodyBytes)); pErr == nil {
				if v := strings.TrimSpace(formVals.Get("_csrf")); v != "" {
					submittedCSRF = v
				}
			}
		}

		validCSRF := cookieCSRF != "" && submittedCSRF != "" &&
			subtle.ConstantTimeCompare([]byte(cookieCSRF), []byte(submittedCSRF)) == 1
		if !validCSRF {
			forbiddenCSRFResponse(w)
			return
		}

		next.ServeHTTP(w, r)
	})
}

func notFoundCloakResponse(w http.ResponseWriter) {
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("X-Robots-Tag", "noindex, nofollow")
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.WriteHeader(http.StatusNotFound)
	_, _ = w.Write([]byte("Not Found"))
}

// ExtractHostOnly chuẩn hóa tên miền (lowercase, bỏ :port và www.).
func ExtractHostOnly(rawHostOrURL string) string {
	trimmed := strings.TrimSpace(rawHostOrURL)
	if trimmed == "" {
		return ""
	}
	var parsed *url.URL
	var err error
	if strings.Contains(trimmed, "://") {
		parsed, err = url.Parse(trimmed)
	} else {
		parsed, err = url.Parse("http://" + trimmed)
	}
	if err != nil || parsed == nil {
		return ""
	}
	host := strings.ToLower(parsed.Hostname())
	host = strings.TrimPrefix(host, "www.")
	return host
}

// IsAllowedShortlinkCloakPath kiểm tra path được phép mở qua tên miền shortlink bị che.
func IsAllowedShortlinkCloakPath(method, path string) bool {
	if method != http.MethodGet && method != http.MethodHead {
		return false
	}
	switch path {
	case "/assets/bridge.js", "/assets/bot-logo.webp", "/assets/bot-cart-3d.webp", "/assets/landing.js":
		return true
	}
	switch strings.ToLower(path) {
	case "/health", "/convert", "/status", "/metrics", "/robots":
		return false
	}
	code := ""
	if strings.HasPrefix(path, "/go/") {
		code = strings.TrimPrefix(path, "/go/")
	} else if strings.HasPrefix(path, "/") {
		code = strings.TrimPrefix(path, "/")
	}
	return !strings.Contains(code, "/") && db.IsValidShortCode(code)
}

// IsPublicHomeCloaked kiểm tra Admin có bật ẩn trang chủ `/` thành 404 hay không.
func IsPublicHomeCloaked(state *AppState) bool {
	m, err := state.DB.GetSettingsMapCached()
	if err != nil {
		return false
	}
	return strings.TrimSpace(m["public_home_cloak"]) == "1"
}

func isLoopbackHost(r *http.Request) bool {
	h := ExtractHostOnly(r.Host)
	return h == "127.0.0.1" || h == "localhost" || h == "::1" || h == "[::1]"
}

func hasProxyOrTunnelHeaders(r *http.Request) bool {
	return r.Header.Get("Cf-Ray") != "" ||
		r.Header.Get("Cf-Connecting-Ip") != "" ||
		r.Header.Get("X-Forwarded-For") != "" ||
		r.Header.Get("X-Forwarded-Host") != "" ||
		r.Header.Get("X-Dealx-Cloak") != ""
}

func isLoopbackRemoteAddr(remoteAddr string) bool {
	trimmed := strings.TrimSpace(remoteAddr)
	if trimmed == "" {
		return true // httptest default or empty peer IP
	}
	host := trimmed
	if h, _, err := net.SplitHostPort(trimmed); err == nil {
		host = h
	}
	ip := net.ParseIP(strings.Trim(host, "[]"))
	if ip == nil {
		return true
	}
	return ip.IsLoopback()
}

func isLocalPCTrustedLoopbackRequest(state *AppState, r *http.Request) bool {
	if state.Config.PublicBaseURL != "" {
		return false
	}
	if hasProxyOrTunnelHeaders(r) {
		return false
	}
	if !isLoopbackRemoteAddr(r.RemoteAddr) {
		return false
	}
	return isLoopbackHost(r)
}

// IsTrustedLocalAdminHeader kiểm tra header nội bộ xác thực truy cập Admin từ PC localhost.
func IsTrustedLocalAdminHeader(state *AppState, r *http.Request) bool {
	return state.Config.PublicBaseURL == "" &&
		!hasProxyOrTunnelHeaders(r) &&
		isLoopbackHost(r) &&
		r.Header.Get(LocalPCTrustedHeader) == "1"
}

// ShortlinkCloakMiddleware bảo vệ Web Chính & `/admin` khỏi domain shortlink / Cloudflare Worker / Tunnel.
func ShortlinkCloakMiddleware(state *AppState) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			r.Header.Del(LocalPCTrustedHeader)
			localPCTrusted := isLocalPCTrustedLoopbackRequest(state, r)

			hasCloakHeader := strings.TrimSpace(r.Header.Get("X-Dealx-Cloak")) == "1"
			hasCFProxyHeader := state.Config.PublicBaseURL == "" &&
				(r.Header.Get("Cf-Ray") != "" || r.Header.Get("Cf-Connecting-Ip") != "")

			isCloakedDomain := false
			if hasCloakHeader || hasCFProxyHeader {
				isCloakedDomain = true
			} else {
				reqHost := ExtractHostOnly(r.Host)
				publicHost := ExtractHostOnly(state.Config.PublicBaseURL)
				if reqHost != "" && publicHost != reqHost {
					m, _ := state.DB.GetSettingsMapCached()
					dbShortRaw := strings.TrimSpace(m["shortlink_base_url"])
					shortBase := config.ResolveEffectiveShortlinkBaseURL(dbShortRaw, state.Config.ShortlinkBaseURL, "")
					shortHost := ExtractHostOnly(shortBase)
					if strings.HasSuffix(reqHost, ".trycloudflare.com") && publicHost == "" {
						isCloakedDomain = true
					} else if shortHost != "" && shortHost != "127.0.0.1" && shortHost != "localhost" && shortHost != "::1" {
						originSub := "origin." + shortHost
						if (shortHost == reqHost || reqHost == originSub) &&
							publicHost != shortHost &&
							!(publicHost == "" && strings.HasSuffix(reqHost, ".onrender.com")) {
							isCloakedDomain = true
						}
					}
				}
			}

			if isCloakedDomain && !IsAllowedShortlinkCloakPath(r.Method, r.URL.Path) {
				notFoundCloakResponse(w)
				return
			}

			if localPCTrusted && !isCloakedDomain {
				r.Header.Set(LocalPCTrustedHeader, "1")
			}

			next.ServeHTTP(w, r)
		})
	}
}
