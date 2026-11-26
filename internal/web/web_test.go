package web

import (
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
	"time"

	"salehunter-go/internal/bot"
	"salehunter-go/internal/config"
	"salehunter-go/internal/db"
	"salehunter-go/internal/services/accesstrade"
)

func makeTestState(t *testing.T, publicBaseURL string, webhookCh chan<- bot.Update) *AppState {
	t.Helper()
	dbPath := filepath.Join(t.TempDir(), "web_test.db")
	appDB, err := db.InitLocalForTest(dbPath)
	if err != nil {
		t.Fatalf("InitLocalForTest: %v", err)
	}
	t.Cleanup(func() { _ = appDB.Close() })

	cfg := config.Config{
		TelegramToken:              "123:TEST",
		BotUsername:                "vvipass_vip_bot",
		DatabasePath:               dbPath,
		AccessTradeDeeplinkTemplate: "https://go.isclix.com/deep_link/1234/5678?url={{CLEAN_URL}}",
		AdminUsername:              "admin",
		AdminPassword:              "StrongTestPass!2026",
		Port:                       10000,
		PublicBaseURL:              publicBaseURL,
		TelegramWebhookSecret:      "whsec_test_1234567890",
	}
	atClient := accesstrade.NewClient()
	return NewAppState(cfg, appDB, atClient, webhookCh)
}

func doReq(handler http.Handler, req *http.Request) *httptest.ResponseRecorder {
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)
	return rec
}

func TestSecurityHeadersAndCSPClean(t *testing.T) {
	state := makeTestState(t, "", nil)
	handler := BuildRouter(state)

	req := httptest.NewRequest(http.MethodGet, "/health", nil)
	res := doReq(handler, req)
	if res.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", res.Code)
	}
	if res.Header().Get("X-Content-Type-Options") != "nosniff" {
		t.Fatalf("missing nosniff header")
	}
	if res.Header().Get("X-Frame-Options") != "DENY" {
		t.Fatalf("missing DENY frame header")
	}
	if !strings.Contains(res.Header().Get("Content-Security-Policy"), "default-src 'self'") {
		t.Fatalf("missing clean CSP")
	}
	if !strings.Contains(res.Header().Get("Permissions-Policy"), "camera=()") {
		t.Fatalf("missing Permissions-Policy header")
	}
	if res.Header().Get("Cross-Origin-Opener-Policy") != "same-origin" {
		t.Fatalf("missing Cross-Origin-Opener-Policy header")
	}
}

func TestAdminCSRFAndSameOriginProtection(t *testing.T) {
	state := makeTestState(t, "https://app.onrender.com", nil)
	handler := BuildRouter(state)

	const sid = "sess_csrf_test"
	state.Sessions[sid] = struct{}{}

	// 1. GET /admin với session -> cấp cookie dealx_csrf và _csrf trong form
	reqGet := httptest.NewRequest(http.MethodGet, "/admin", nil)
	reqGet.Host = "app.onrender.com"
	reqGet.AddCookie(&http.Cookie{Name: "dealx_session", Value: sid})
	resGet := doReq(handler, reqGet)
	if resGet.Code != http.StatusOK {
		t.Fatalf("expected 200 GET /admin, got %d", resGet.Code)
	}
	var csrfVal string
	for _, c := range resGet.Result().Cookies() {
		if c.Name == "dealx_csrf" {
			csrfVal = c.Value
			break
		}
	}
	if csrfVal == "" {
		t.Fatal("expected dealx_csrf cookie on GET /admin")
	}
	if !strings.Contains(resGet.Body.String(), fmt.Sprintf(`name="_csrf" value="%s"`, csrfVal)) {
		t.Fatal("expected _csrf hidden input injected into POST forms")
	}

	// 2. POST cross-origin -> 403
	form1 := url.Values{"template": {"https://evil.example/?url={{CLEAN_URL}}"}, "_csrf": {csrfVal}}
	reqCross := httptest.NewRequest(http.MethodPost, "/admin/settings/affiliate", strings.NewReader(form1.Encode()))
	reqCross.Host = "app.onrender.com"
	reqCross.Header.Set("Origin", "https://evil.example")
	reqCross.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	reqCross.AddCookie(&http.Cookie{Name: "dealx_session", Value: sid})
	reqCross.AddCookie(&http.Cookie{Name: "dealx_csrf", Value: csrfVal})
	if resCross := doReq(handler, reqCross); resCross.Code != http.StatusForbidden {
		t.Fatalf("expected 403 cross-origin, got %d", resCross.Code)
	}

	// 3. POST thiếu _csrf -> 403
	formNoCSRF := url.Values{"template": {"https://go.isclix.com/?url={{CLEAN_URL}}"}}
	reqMissing := httptest.NewRequest(http.MethodPost, "/admin/settings/affiliate", strings.NewReader(formNoCSRF.Encode()))
	reqMissing.Host = "app.onrender.com"
	reqMissing.Header.Set("Origin", "https://app.onrender.com")
	reqMissing.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	reqMissing.AddCookie(&http.Cookie{Name: "dealx_session", Value: sid})
	reqMissing.AddCookie(&http.Cookie{Name: "dealx_csrf", Value: csrfVal})
	if resMiss := doReq(handler, reqMissing); resMiss.Code != http.StatusForbidden {
		t.Fatalf("expected 403 missing csrf, got %d", resMiss.Code)
	}

	// 4. POST hợp lệ (same-origin + double submit _csrf) -> 303
	reqValid := httptest.NewRequest(http.MethodPost, "/admin/settings/affiliate", strings.NewReader(form1.Encode()))
	reqValid.Host = "app.onrender.com"
	reqValid.Header.Set("Origin", "https://app.onrender.com")
	reqValid.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	reqValid.AddCookie(&http.Cookie{Name: "dealx_session", Value: sid})
	reqValid.AddCookie(&http.Cookie{Name: "dealx_csrf", Value: csrfVal})
	if resValid := doReq(handler, reqValid); resValid.Code != http.StatusSeeOther {
		t.Fatalf("expected 303 valid csrf, got %d", resValid.Code)
	}
}

func TestShortlinkCloakAndXORBridge(t *testing.T) {
	state := makeTestState(t, "", nil)
	_ = db.SetSetting(state.DB.DB, "shortlink_mobile_only", "1")
	_ = db.SetSetting(state.DB.DB, "shortlink_base_url", "https://thegioisieusale.click")
	state.DB.InvalidateSettingsCache()

	target := "https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fproduct%2F1%2F2"
	wrapped := state.DB.WrapURLCached("https://thegioisieusale.click", target, "Tai nghe Bluetooth", "Shopee")
	code := strings.TrimPrefix(wrapped, "https://thegioisieusale.click/")
	handler := BuildRouter(state)

	// 1. Cloak ẩn /admin & / với domain bọc link
	reqCloak := httptest.NewRequest(http.MethodGet, "/admin", nil)
	reqCloak.Host = "thegioisieusale.click"
	reqCloak.Header.Set("X-Dealx-Cloak", "1")
	if res := doReq(handler, reqCloak); res.Code != http.StatusNotFound {
		t.Fatalf("expected 404 cloaked /admin, got %d", res.Code)
	}

	// 2. Preview Bot nhận HTML OG tags thay vì link đích thật
	reqBot := httptest.NewRequest(http.MethodGet, "/"+code, nil)
	reqBot.Host = "thegioisieusale.click"
	reqBot.Header.Set("User-Agent", "TelegramBot (like TwitterBot)")
	resBot := doReq(handler, reqBot)
	if resBot.Code != http.StatusOK || !strings.Contains(resBot.Body.String(), "og:title") {
		t.Fatalf("expected 200 OG preview, got %d", resBot.Code)
	}

	// 3. Desktop bị chặn 403 khi mobile_only=1
	reqPC := httptest.NewRequest(http.MethodGet, "/"+code, nil)
	reqPC.Host = "thegioisieusale.click"
	reqPC.Header.Set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/120.0.0.0 Safari/537.36")
	if resPC := doReq(handler, reqPC); resPC.Code != http.StatusForbidden {
		t.Fatalf("expected 403 on desktop when mobile_only=1, got %d", resPC.Code)
	}

	// 4. Mobile nhận cầu nối XOR bridge -> giải mã khớp URL gốc
	reqMob := httptest.NewRequest(http.MethodGet, "/"+code, nil)
	reqMob.Host = "thegioisieusale.click"
	reqMob.Header.Set("User-Agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Mobile/15E148")
	resMob := doReq(handler, reqMob)
	if resMob.Code != http.StatusOK {
		t.Fatalf("expected 200 XOR bridge on mobile, got %d", resMob.Code)
	}
	bodyStr := resMob.Body.String()
	if strings.Contains(bodyStr, "an_redir") {
		t.Fatal("raw target URL leaked inside bridge HTML")
	}
	reP := regexp.MustCompile(`data-p="([0-9a-f]+)"`)
	reK := regexp.MustCompile(`data-k="([0-9a-f]+)"`)
	mP := reP.FindStringSubmatch(bodyStr)
	mK := reK.FindStringSubmatch(bodyStr)
	if len(mP) < 2 || len(mK) < 2 {
		t.Fatalf("missing XOR payload attributes in bridge HTML")
	}
	decoded, ok := DecodeBridgePayload(mP[1], mK[1])
	if !ok || decoded != target {
		t.Fatalf("decoded mismatch: ok=%v decoded=%s", ok, decoded)
	}
}

func TestCloudDefaultPasswordWarningAndBruteForceLimit(t *testing.T) {
	state := makeTestState(t, "https://bot.onrender.com", nil)
	state.Config.AdminPassword = "DealX@Admin2026"
	handler := BuildRouter(state)

	const sid = "cloud_warn_sess"
	state.Sessions[sid] = struct{}{}

	reqDash := httptest.NewRequest(http.MethodGet, "/admin", nil)
	reqDash.Host = "bot.onrender.com"
	reqDash.AddCookie(&http.Cookie{Name: "dealx_session", Value: sid})
	resDash := doReq(handler, reqDash)
	if !strings.Contains(resDash.Body.String(), "CẢNH BÁO BẢO MẬT CLOUD") {
		t.Fatal("expected Cloud default password warning banner")
	}

	// Brute-force login protection: cài sẵn 5 lần sai gần đây để không phải đợi 5*500ms sleep trong test
	state.LoginLimiter["1.2.3.4"] = RateEntry{Count: 5, LastTS: time.Now().Unix()}
	form := url.Values{"username": {"admin"}, "password": {"wrong"}}
	reqLocked := httptest.NewRequest(http.MethodPost, "/admin/login", strings.NewReader(form.Encode()))
	reqLocked.Host = "bot.onrender.com"
	reqLocked.Header.Set("Origin", "https://bot.onrender.com")
	reqLocked.Header.Set("Cf-Ray", "8abc1234-SIN")
	reqLocked.Header.Set("Cf-Connecting-Ip", "1.2.3.4")
	reqLocked.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	if resLocked := doReq(handler, reqLocked); resLocked.Code != http.StatusTooManyRequests {
		t.Fatalf("expected 429 after 5 failed attempts with Cf-Ray, got %d", resLocked.Code)
	}

	// Chống bypass Rate Limit: attacker gửi Cf-Connecting-Ip giả không có Cf-Ray và nhét IP giả ở đầu X-Forwarded-For
	state.LoginLimiter["5.6.7.8"] = RateEntry{Count: 5, LastTS: time.Now().Unix()}
	reqSpoof := httptest.NewRequest(http.MethodPost, "/admin/login", strings.NewReader(form.Encode()))
	reqSpoof.Host = "bot.onrender.com"
	reqSpoof.Header.Set("Origin", "https://bot.onrender.com")
	reqSpoof.Header.Set("Cf-Connecting-Ip", "9.9.9.9")
	reqSpoof.Header.Set("X-Forwarded-For", "8.8.4.4, 5.6.7.8")
	reqSpoof.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	if resSpoof := doReq(handler, reqSpoof); resSpoof.Code != http.StatusTooManyRequests {
		t.Fatalf("expected 429 via right-most XFF 5.6.7.8 (resisting Cf-Connecting-Ip & left XFF spoof), got %d", resSpoof.Code)
	}
}

func TestLocalPCSkipLoginAndSpoofProtection(t *testing.T) {
	state := makeTestState(t, "", nil)
	handler := BuildRouter(state)

	// Direct localhost PC -> skip login
	reqLocal := httptest.NewRequest(http.MethodGet, "/admin", nil)
	reqLocal.Host = "127.0.0.1:10000"
	reqLocal.RemoteAddr = "127.0.0.1:54321"
	if res := doReq(handler, reqLocal); res.Code != http.StatusOK {
		t.Fatalf("expected 200 on local PC loopback, got %d", res.Code)
	}

	// Spoof X-Dealx-Local-Trusted qua proxy -> reject redirect /admin/login
	reqSpoof := httptest.NewRequest(http.MethodGet, "/admin", nil)
	reqSpoof.Host = "127.0.0.1:10000"
	reqSpoof.RemoteAddr = "8.8.8.8:12345"
	reqSpoof.Header.Set(LocalPCTrustedHeader, "1")
	reqSpoof.Header.Set("X-Forwarded-For", "8.8.8.8")
	if res := doReq(handler, reqSpoof); res.Code != http.StatusSeeOther {
		t.Fatalf("expected 303 redirect on spoofed local header, got %d", res.Code)
	}
}

func TestTelegramWebhookAuthAndQueue(t *testing.T) {
	ch := make(chan bot.Update, 2)
	state := makeTestState(t, "https://bot.onrender.com", ch)
	handler := BuildRouter(state)

	reqBad := httptest.NewRequest(http.MethodPost, "/telegram/webhook", strings.NewReader(`{"update_id":1}`))
	reqBad.Header.Set("X-Telegram-Bot-Api-Secret-Token", "wrong_secret")
	if res := doReq(handler, reqBad); res.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401 for wrong secret, got %d", res.Code)
	}

	reqOK := httptest.NewRequest(http.MethodPost, "/telegram/webhook", strings.NewReader(`{"update_id":42}`))
	reqOK.Header.Set("X-Telegram-Bot-Api-Secret-Token", state.Config.TelegramWebhookSecret)
	if res := doReq(handler, reqOK); res.Code != http.StatusOK {
		t.Fatalf("expected 200 for valid webhook, got %d", res.Code)
	}
	select {
	case u := <-ch:
		if u.UpdateID != 42 {
			t.Fatalf("expected UpdateID 42, got %d", u.UpdateID)
		}
	default:
		t.Fatal("expected update pushed into webhook channel")
	}
}

func TestPruneRateLimiterMapAndAssets(t *testing.T) {
	now := int64(10000)
	m := map[string]RateEntry{
		"expired": {Count: 5, LastTS: now - 1000},
		"old":     {Count: 4, LastTS: now - 50},
		"new":     {Count: 1, LastTS: now - 10},
	}
	PruneRateLimiterMap(m, now, 900, 2)
	if _, ok := m["expired"]; ok {
		t.Fatal("expected expired entry pruned")
	}
	if len(m) > 2 {
		t.Fatalf("expected len <= 2, got %d", len(m))
	}

	state := makeTestState(t, "", nil)
	handler := BuildRouter(state)
	for _, asset := range []string{"/assets/bot-logo.webp", "/assets/bot-cart-3d.webp", "/assets/bridge.js", "/assets/home.js", "/assets/landing.js"} {
		res := doReq(handler, httptest.NewRequest(http.MethodGet, asset, nil))
		if res.Code != http.StatusOK {
			t.Fatalf("asset %s failed: %d", asset, res.Code)
		}
		b, _ := io.ReadAll(res.Body)
		if len(b) == 0 {
			t.Fatalf("asset %s empty", asset)
		}
	}
}

func TestLoopbackShortlinkDoesNotSelfCloak(t *testing.T) {
	state := makeTestState(t, "", nil)
	state.Config.ShortlinkBaseURL = "http://127.0.0.1:18080"
	handler := BuildRouter(state)

	for _, path := range []string{"/health", "/", "/admin"} {
		req := httptest.NewRequest(http.MethodGet, path, nil)
		req.Host = "127.0.0.1:18080"
		req.RemoteAddr = "127.0.0.1:50001"
		res := doReq(handler, req)
		if res.Code != http.StatusOK {
			t.Fatalf("loopback path %s should not self-cloak to 404, got %d", path, res.Code)
		}
	}

	// Nhưng nếu có X-Dealx-Cloak: 1 thì vẫn phải ẩn /admin thành 404
	reqCloak := httptest.NewRequest(http.MethodGet, "/admin", nil)
	reqCloak.Host = "127.0.0.1:18080"
	reqCloak.Header.Set("X-Dealx-Cloak", "1")
	if res := doReq(handler, reqCloak); res.Code != http.StatusNotFound {
		t.Fatalf("expected 404 when X-Dealx-Cloak: 1 is set, got %d", res.Code)
	}
}

func TestDeepSecurityHardening(t *testing.T) {
	// 1. Chặn POST body quá lớn trên /convert (>64KB) và /admin/login (>32KB)
	stateCloud := makeTestState(t, "http://proxy-backend.internal", nil)
	handlerCloud := BuildRouter(stateCloud)

	bigConvertBody := "url=https%3A%2F%2Fshopee.vn%2Fa&pad=" + strings.Repeat("A", 70*1024)
	reqBigConvert := httptest.NewRequest(http.MethodPost, "/convert", strings.NewReader(bigConvertBody))
	reqBigConvert.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	if res := doReq(handlerCloud, reqBigConvert); res.Code != http.StatusRequestEntityTooLarge {
		t.Fatalf("expected 413 on oversized /convert body, got %d", res.Code)
	}

	bigLoginBody := "username=admin&password=" + strings.Repeat("B", 36*1024)
	reqBigLogin := httptest.NewRequest(http.MethodPost, "/admin/login", strings.NewReader(bigLoginBody))
	reqBigLogin.Host = "proxy-backend.internal"
	reqBigLogin.Header.Set("Origin", "http://proxy-backend.internal")
	reqBigLogin.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	if res := doReq(handlerCloud, reqBigLogin); res.Code != http.StatusRequestEntityTooLarge {
		t.Fatalf("expected 413 on oversized /admin/login body, got %d", res.Code)
	}

	// 2. X-Forwarded-Proto: https -> ép Secure cookie & Cache-Control: no-store
	reqLoginPage := httptest.NewRequest(http.MethodGet, "/admin/login", nil)
	reqLoginPage.Host = "proxy-backend.internal"
	resLoginPage := doReq(handlerCloud, reqLoginPage)
	if !strings.Contains(resLoginPage.Header().Get("Cache-Control"), "no-store") ||
		resLoginPage.Header().Get("X-Robots-Tag") != "noindex, nofollow" {
		t.Fatalf("expected no-store + noindex on /admin/login, got Cache-Control=%q X-Robots-Tag=%q",
			resLoginPage.Header().Get("Cache-Control"), resLoginPage.Header().Get("X-Robots-Tag"))
	}

	loginForm := url.Values{"username": {stateCloud.Config.AdminUsername}, "password": {stateCloud.Config.AdminPassword}}
	reqLoginOK := httptest.NewRequest(http.MethodPost, "/admin/login", strings.NewReader(loginForm.Encode()))
	reqLoginOK.Host = "proxy-backend.internal"
	reqLoginOK.Header.Set("Origin", "http://proxy-backend.internal")
	reqLoginOK.Header.Set("X-Forwarded-Proto", "https")
	reqLoginOK.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	resLoginOK := doReq(handlerCloud, reqLoginOK)
	if resLoginOK.Code != http.StatusSeeOther {
		t.Fatalf("expected 303 on valid login, got %d", resLoginOK.Code)
	}
	var sessCookie *http.Cookie
	for _, c := range resLoginOK.Result().Cookies() {
		if c.Name == "dealx_session" {
			sessCookie = c
			break
		}
	}
	if sessCookie == nil || !sessCookie.Secure {
		t.Fatalf("expected dealx_session cookie with Secure flag under X-Forwarded-Proto: https, got %#v", sessCookie)
	}

	reqDash := httptest.NewRequest(http.MethodGet, "/admin", nil)
	reqDash.Host = "proxy-backend.internal"
	reqDash.Header.Set("X-Forwarded-Proto", "https")
	reqDash.AddCookie(sessCookie)
	resDash := doReq(handlerCloud, reqDash)
	if resDash.Code != http.StatusOK || !strings.Contains(resDash.Header().Get("Cache-Control"), "no-store") {
		t.Fatalf("expected 200 and no-store on /admin, got code=%d cc=%q", resDash.Code, resDash.Header().Get("Cache-Control"))
	}
	var csrfCookie *http.Cookie
	for _, c := range resDash.Result().Cookies() {
		if c.Name == "dealx_csrf" {
			csrfCookie = c
			break
		}
	}
	if csrfCookie == nil || !csrfCookie.Secure {
		t.Fatalf("expected dealx_csrf cookie with Secure flag under X-Forwarded-Proto: https, got %#v", csrfCookie)
	}

	// 3. Chặn redirect nguy hiểm (javascript:) trong bảng short_links
	_, err := stateCloud.DB.DB.Exec(
		`INSERT INTO short_links (code, target_url, title, platform, clicks_count, created_at) VALUES (?, ?, ?, ?, 0, ?)`,
		"badxss1", "javascript:alert(1)", "Evil", "Shopee", "2026-11-26T00:00:00Z",
	)
	if err != nil {
		t.Fatalf("insert unsafe shortlink: %v", err)
	}
	reqBadShort := httptest.NewRequest(http.MethodGet, "/badxss1", nil)
	reqBadShort.Host = "proxy-backend.internal"
	if resBadShort := doReq(handlerCloud, reqBadShort); resBadShort.Code != http.StatusNotFound {
		t.Fatalf("expected 404 blocking javascript: redirect, got %d", resBadShort.Code)
	}

	// 4. PC Local POST vẫn bắt buộc Double-Submit CSRF
	stateLocal := makeTestState(t, "", nil)
	handlerLocal := BuildRouter(stateLocal)

	reqLocalDash := httptest.NewRequest(http.MethodGet, "/admin", nil)
	reqLocalDash.Host = "127.0.0.1:10000"
	reqLocalDash.RemoteAddr = "127.0.0.1:50001"
	resLocalDash := doReq(handlerLocal, reqLocalDash)
	var localCSRF string
	for _, c := range resLocalDash.Result().Cookies() {
		if c.Name == "dealx_csrf" {
			localCSRF = c.Value
			break
		}
	}
	if localCSRF == "" {
		t.Fatal("expected dealx_csrf cookie in PC local mode")
	}

	reqLocalNoCSRF := httptest.NewRequest(http.MethodPost, "/admin/settings/affiliate",
		strings.NewReader(url.Values{"template": {"https://go.isclix.com/?url={{CLEAN_URL}}"}}.Encode()))
	reqLocalNoCSRF.Host = "127.0.0.1:10000"
	reqLocalNoCSRF.RemoteAddr = "127.0.0.1:50001"
	reqLocalNoCSRF.Header.Set("Origin", "http://127.0.0.1:10000")
	reqLocalNoCSRF.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	if res := doReq(handlerLocal, reqLocalNoCSRF); res.Code != http.StatusForbidden {
		t.Fatalf("expected 403 on PC local POST without CSRF token, got %d", res.Code)
	}

	reqLocalWithCSRF := httptest.NewRequest(http.MethodPost, "/admin/settings/affiliate",
		strings.NewReader(url.Values{"template": {"https://go.isclix.com/?url={{CLEAN_URL}}"}, "_csrf": {localCSRF}}.Encode()))
	reqLocalWithCSRF.Host = "127.0.0.1:10000"
	reqLocalWithCSRF.RemoteAddr = "127.0.0.1:50001"
	reqLocalWithCSRF.Header.Set("Origin", "http://127.0.0.1:10000")
	reqLocalWithCSRF.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	reqLocalWithCSRF.AddCookie(&http.Cookie{Name: "dealx_csrf", Value: localCSRF})
	if res := doReq(handlerLocal, reqLocalWithCSRF); res.Code != http.StatusSeeOther {
		t.Fatalf("expected 303 on PC local POST with valid CSRF, got %d", res.Code)
	}
}
