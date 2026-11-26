package web

import (
	"encoding/json"
	"io"
	"log/slog"
	"net/http"

	"salehunter-go/internal/bot"
	"salehunter-go/internal/utils"
)

// BuildRouter xây dựng HTTP router đầy đủ cho Web Public, Web Admin, Shortlink & Telegram Webhook.
func BuildRouter(state *AppState) http.Handler {
	mux := http.NewServeMux()

	// Trang chủ công khai & chuyển đổi link trực tiếp
	mux.HandleFunc("GET /{$}", HandleRoot(state))
	mux.HandleFunc("POST /convert", HandlePublicConvert(state))
	mux.HandleFunc("GET /convert", func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "/", http.StatusSeeOther)
	})

	// Static JS / WebP Assets
	mux.HandleFunc("GET /assets/bridge.js", HandleBridgeJS)
	mux.HandleFunc("GET /assets/home.js", HandleHomeJS)
	mux.HandleFunc("GET /assets/theme.js", HandleThemeJS)
	mux.HandleFunc("GET /assets/admin.js", HandleAdminJS)
	mux.HandleFunc("GET /assets/bot-logo.webp", HandleBotLogoWebp)
	mux.HandleFunc("GET /assets/bot-cart-3d.webp", HandleBotCart3DWebp)
	mux.HandleFunc("GET /assets/landing.js", HandleLandingJS)

	// Health & Telegram Webhook
	mux.HandleFunc("GET /health", HandleHealth)
	mux.HandleFunc("POST /telegram/webhook", HandleTelegramWebhook(state))

	// Xác thực & Quản trị Web Admin (POST /admin/logout bắt buộc Double-Submit CSRF chống Logout CSRF)
	mux.HandleFunc("GET /admin/login", HandleLoginPage(state))
	mux.HandleFunc("POST /admin/login", HandleLoginSubmit(state))
	mux.HandleFunc("POST /admin/logout", HandleLogout(state))

	mux.HandleFunc("GET /admin", HandleDashboard(state))
	mux.HandleFunc("POST /admin/settings/accesstrade", HandleUpdateAccessTrade(state))
	mux.HandleFunc("POST /admin/settings/accesstrade/sync", HandleSyncAccessTrade(state))
	mux.HandleFunc("POST /admin/settings/shopee", HandleUpdateShopeeSettings(state))
	mux.HandleFunc("POST /admin/settings/affiliate", HandleUpdateAffiliateSettings(state))
	mux.HandleFunc("POST /admin/settings/notifications", HandleUpdateNotifications(state))
	mux.HandleFunc("POST /admin/orders/sync", HandleSyncOrders(state))
	mux.HandleFunc("POST /admin/vouchers/sync", HandleSyncVouchers(state))
	mux.HandleFunc("POST /admin/deals/add", HandleAddDeal(state))
	mux.HandleFunc("POST /admin/deals/import", HandleImportDeals(state))
	mux.HandleFunc("POST /admin/deals/toggle", HandleToggleDeal(state))
	mux.HandleFunc("POST /admin/deals/delete", HandleDeleteDeal(state))

	// Giải mã shortlink 8 ký tự chuẩn DealX
	mux.HandleFunc("GET /go/{code}", func(w http.ResponseWriter, r *http.Request) {
		HandleShortlinkRedirect(state, r.PathValue("code"))(w, r)
	})
	mux.HandleFunc("GET /{code}", func(w http.ResponseWriter, r *http.Request) {
		HandleShortlinkRedirect(state, r.PathValue("code"))(w, r)
	})

	// Chuỗi middleware chuẩn: Recovery -> SecurityHeaders -> ShortlinkCloak -> AdminCSRF -> Mux
	var handler http.Handler = AdminCSRFMiddleware(mux)
	handler = ShortlinkCloakMiddleware(state)(handler)
	handler = SecurityHeadersMiddleware(handler)
	handler = RecoveryMiddleware(handler)
	return handler
}

// HandleHealth phục vụ healthcheck cho Render / Docker / Fly.io.
func HandleHealth(w http.ResponseWriter, _ *http.Request) {
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("The Gioi Sieu Sale Bot & Web Admin Healthy"))
}

// HandleTelegramWebhook xác thực secret token hằng thời gian và đẩy Update vào Dispatcher.
func HandleTelegramWebhook(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if state.WebhookCh == nil {
			w.WriteHeader(http.StatusNotFound)
			return
		}

		providedSecret := r.Header.Get("X-Telegram-Bot-Api-Secret-Token")
		expectedSecret := state.Config.TelegramWebhookSecret

		if len(providedSecret) == 0 ||
			!utils.ConstantTimeEqualHash(providedSecret, expectedSecret) {
			slog.Warn("[WEB] Từ chối yêu cầu Telegram Webhook do sai secret token.")
			w.WriteHeader(http.StatusUnauthorized)
			return
		}

		body, err := io.ReadAll(io.LimitReader(r.Body, 2*1024*1024))
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		var upd bot.Update
		if jErr := json.Unmarshal(body, &upd); jErr != nil {
			slog.Warn("[WEB] Không thể giải mã JSON Telegram Webhook", "err", jErr)
			w.WriteHeader(http.StatusOK)
			return
		}

		select {
		case state.WebhookCh <- upd:
			w.WriteHeader(http.StatusOK)
		default:
			slog.Error("[WEB] Hàng đợi Telegram Webhook đầy, không thể đẩy Update.")
			w.WriteHeader(http.StatusServiceUnavailable)
		}
	}
}
