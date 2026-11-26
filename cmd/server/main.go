package main

import (
	"context"
	"errors"
	"fmt"
	"hash/fnv"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"sync"
	"syscall"
	"time"

	"salehunter-go/internal/bot"
	"salehunter-go/internal/config"
	"salehunter-go/internal/db"
	"salehunter-go/internal/services/accesstrade"
	"salehunter-go/internal/services/orders"
	"salehunter-go/internal/services/social"
	"salehunter-go/internal/web"
)

func main() {
	logger := slog.New(slog.NewTextHandler(os.Stdout, &slog.HandlerOptions{
		Level: slog.LevelInfo,
	}))
	slog.SetDefault(logger)

	slog.Info("🚀 Khởi chạy hệ thống Bot Thế Giới Siêu Sale (Go Web Hosting Edition)...")

	cfg := config.FromEnv()
	if cfg.PublicBaseURL != "" && cfg.AdminPassword == "DealX@Admin2026" {
		slog.Warn("⚠️ CẢNH BÁO BẢO MẬT: Đang chạy chế độ Cloud nhưng dùng ADMIN_PASSWORD mặc định — hãy đặt biến môi trường ADMIN_PASSWORD mạnh ngay!")
	}

	appDB, err := db.Init(cfg)
	if err != nil {
		slog.Error("❌ Không thể khởi tạo Database", "err", err)
		os.Exit(1)
	}
	defer func() {
		if closeErr := appDB.Close(); closeErr != nil {
			slog.Warn("Lỗi khi đóng Database", "err", closeErr)
		}
	}()

	if cfg.TursoURL != "" && cfg.TursoAuthToken != "" {
		slog.Info("☁️ Đã kích hoạt Turso Cloud Database làm kho lưu trữ bền vững (không dùng file SQLite cục bộ).")
	} else {
		slog.Info("💾 Đang chạy chế độ PC Local — lưu dữ liệu bền vững trên SQLite", "path", cfg.DatabasePath)
	}

	atClient := accesstrade.NewClient()
	seedVoucherCacheFromDB(appDB, cfg, atClient)

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	var bgWG sync.WaitGroup

	// Warmup định kỳ mỗi 60s cho ACCESSTRADE campaigns + market vouchers + cleanup sessions mỗi 6h
	bgWG.Add(1)
	go func() {
		defer bgWG.Done()
		runWarmupLoop(ctx, appDB, cfg, atClient)
	}()

	var webhookCh chan bot.Update
	if cfg.PublicBaseURL != "" {
		webhookCh = make(chan bot.Update, 1024)
	}

	state := web.NewAppState(cfg, appDB, atClient, webhookCh)
	router := web.BuildRouter(state)

	listenAddr := fmt.Sprintf("0.0.0.0:%d", cfg.Port)
	ln, err := net.Listen("tcp", listenAddr)
	if err != nil {
		slog.Error("❌ Không thể mở cổng HTTP Web Server", "addr", listenAddr, "err", err)
		os.Exit(1)
	}

	httpServer := &http.Server{
		Addr:              listenAddr,
		Handler:           router,
		ReadHeaderTimeout: 10 * time.Second,
		ReadTimeout:       30 * time.Second,
		WriteTimeout:      60 * time.Second,
		IdleTimeout:       120 * time.Second,
	}

	webReadyCh := make(chan struct{})
	close(webReadyCh)

	webErrCh := make(chan error, 1)
	bgWG.Add(1)
	go func() {
		defer bgWG.Done()
		if sErr := httpServer.Serve(ln); sErr != nil && !errors.Is(sErr, http.ErrServerClosed) {
			slog.Error("Lỗi khi chạy Web Admin Server", "err", sErr)
			webErrCh <- sErr
		}
		close(webErrCh)
	}()

	slog.Info("🌐 Web Public, Admin Dashboard & Health Check đang lắng nghe", "url", fmt.Sprintf("http://127.0.0.1:%d/admin", cfg.Port))

	disableKeepalive := isTruthyEnv(os.Getenv("DISABLE_KEEPALIVE"))
	if !disableKeepalive && cfg.PublicBaseURL != "" {
		bgWG.Add(1)
		go func() {
			defer bgWG.Done()
			runKeepaliveLoop(ctx, cfg.PublicBaseURL)
		}()
	}

	// Chạy ngầm đối soát đơn hàng (mỗi 10 phút) & Social Harvester FB/IG Reels (mỗi 5 phút)
	orders.SpawnOrderTrackerTask(ctx.Done(), appDB, cfg, atClient)
	social.SpawnSocialHarvesterTask(ctx.Done(), appDB, cfg)

	botErrCh := make(chan error, 1)
	if strings.TrimSpace(cfg.TelegramToken) == "" || strings.Contains(cfg.TelegramToken, "DEMO_TOKEN") {
		slog.Info("-------------------------------------------------------")
		slog.Info("⚠️ CHÚ Ý: Chưa tìm thấy TELEGRAM_BOT_TOKEN trong biến môi trường!")
		slog.Info("   Web Admin Dashboard vẫn đang chạy phục vụ", "port", cfg.Port)
		slog.Info("-------------------------------------------------------")
	} else {
		bgWG.Add(1)
		go func() {
			defer bgWG.Done()
			if bErr := bot.RunBot(ctx, cfg, appDB, atClient, webhookCh, webReadyCh); bErr != nil {
				slog.Error("Lỗi khi chạy Telegram Bot", "err", bErr)
				botErrCh <- bErr
			}
			close(botErrCh)
		}()
	}

	select {
	case <-ctx.Done():
		slog.Info("🛑 Nhận tín hiệu thoát — đang thực hiện Graceful Shutdown Web Server & Bot...")
	case wErr := <-webErrCh:
		if wErr != nil {
			slog.Error("Web Server dừng bất ngờ, đang tắt hệ thống...", "err", wErr)
		}
		stop()
	case bErr := <-botErrCh:
		if bErr != nil {
			slog.Error("Telegram Bot dừng bất ngờ, đang tắt hệ thống...", "err", bErr)
		}
		stop()
	}

	shutCtx, shutCancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer shutCancel()
	if err := httpServer.Shutdown(shutCtx); err != nil {
		slog.Warn("Cưỡng chế đóng HTTP connections còn sót sau 5s", "err", err)
	}
	bgWG.Wait()
	slog.Info("✅ Hệ thống đã dừng sạch sẽ.")
}

func seedVoucherCacheFromDB(appDB *db.AppDb, cfg config.Config, atClient *accesstrade.AccessTradeClient) {
	if appDB == nil || appDB.DB == nil {
		return
	}
	tok := cfg.AccessTradeAPIToken
	if dbTok, err := db.GetSetting(appDB.DB, "accesstrade_api_token"); err == nil && dbTok != nil && strings.TrimSpace(*dbTok) != "" {
		tok = *dbTok
	}
	if tok == "" {
		return
	}
	cachedJSON, err := db.GetSetting(appDB.DB, "market_vouchers_cache_json")
	if err != nil || cachedJSON == nil || strings.TrimSpace(*cachedJSON) == "" {
		return
	}
	if seeded := atClient.SeedVoucherCacheFromJSON(tok, *cachedJSON); seeded > 0 {
		slog.Info("⚡ Đã nạp tức thì mã thị trường từ bộ nhớ đệm Database!", "count", seeded)
	}
}

func runWarmupLoop(ctx context.Context, appDB *db.AppDb, cfg config.Config, atClient *accesstrade.AccessTradeClient) {
	var cycle uint64
	var lastJSONHash uint64
	var hasLastHash bool

	runTick := func() {
		settingsMap, _ := appDB.GetSettingsMapCached()
		tok := strings.TrimSpace(settingsMap["accesstrade_api_token"])
		if tok == "" {
			tok = cfg.AccessTradeAPIToken
		}
		if tok != "" {
			isFull := cycle%5 == 0
			if isFull {
				if _, err := atClient.GetApprovedCampaigns(tok, true); err != nil {
					slog.Warn("Không thể đồng bộ sẵn ACCESSTRADE khi khởi động/định kỳ", "err", err)
				}
			}
			if _, err := atClient.SyncMarketVouchersTick(tok, isFull); err != nil {
				slog.Warn("Không thể cập nhật kho mã mỗi phút ACCESSTRADE", "err", err)
			} else if cycle == 0 || cycle%30 == 0 {
				if jsonStr, ok := atClient.ExportVoucherCacheJSON(); ok {
					h := fnv1aHash64([]byte(jsonStr))
					if !hasLastHash || lastJSONHash != h {
						if err := db.SetSetting(appDB.DB, "market_vouchers_cache_json", jsonStr); err == nil {
							lastJSONHash = h
							hasLastHash = true
						}
					}
				}
			}
		} else {
			slog.Warn("⚠️ Chưa tìm thấy ACCESSTRADE_API_TOKEN trong Environment hoặc DB.")
		}

		if cycle%360 == 0 {
			if cleaned, err := appDB.CleanupExpiredSessions(); err == nil && cleaned > 0 {
				slog.Info("🧹 Đã dọn admin sessions hết hạn", "removed", cleaned)
			}
		}
		cycle++
	}

	runTick()
	ticker := time.NewTicker(60 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			runTick()
		}
	}
}

func runKeepaliveLoop(ctx context.Context, baseURL string) {
	healthURL := strings.TrimRight(baseURL, "/") + "/health"
	client := &http.Client{Timeout: 10 * time.Second}

	select {
	case <-time.After(45 * time.Second):
	case <-ctx.Done():
		return
	}

	loggedFirstOK := false
	pingOnce := func() {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, healthURL, nil)
		if err != nil {
			return
		}
		req.Header.Set("User-Agent", "DealX-KeepAlive/1.0")
		resp, err := client.Do(req)
		if err != nil {
			slog.Warn("[KEEPALIVE] Không thể ping", "url", healthURL, "err", err)
			return
		}
		_ = resp.Body.Close()
		if !loggedFirstOK {
			slog.Info("[KEEPALIVE] Ping thành công", "url", healthURL, "status", resp.StatusCode)
			loggedFirstOK = true
		}
	}

	pingOnce()
	ticker := time.NewTicker(540 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			pingOnce()
		}
	}
}

func fnv1aHash64(data []byte) uint64 {
	h := fnv.New64a()
	_, _ = h.Write(data)
	return h.Sum64()
}

func isTruthyEnv(raw string) bool {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "1", "true", "yes":
		return true
	default:
		return false
	}
}
