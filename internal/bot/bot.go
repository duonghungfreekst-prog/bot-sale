package bot

import (
	"context"
	"fmt"
	"log/slog"
	"strings"
	"sync"
	"time"

	"salehunter-go/internal/config"
	"salehunter-go/internal/db"
	"salehunter-go/internal/services/accesstrade"
	"salehunter-go/internal/services/parser"
)

const botDisplayName = "Thế Giới Siêu Sale"

// RunBot khởi chạy Telegram Bot (Webhook 24/7 trên Cloud hoặc Long Polling fallback).
func RunBot(
	ctx context.Context,
	cfg config.Config,
	appDB *db.AppDb,
	atClient *accesstrade.AccessTradeClient,
	webhookCh <-chan Update,
	webReadyCh <-chan struct{},
) error {
	if strings.TrimSpace(cfg.TelegramToken) == "" {
		return fmt.Errorf("TELEGRAM_BOT_TOKEN rỗng, không thể khởi chạy bot")
	}

	slog.Info("Khởi tạo Telegram Bot", "username", "@"+cfg.BotUsername)
	client := NewClient(cfg.TelegramToken)
	dealParser := parser.WithAccessTrade(atClient)

	commands := []BotCommand{
		{Command: "start", Description: "🚀 Bắt đầu / Dán link sản phẩm nhận ưu đãi"},
		{Command: "help", Description: "📖 Mẹo áp mã giảm giá thành công"},
		{Command: "share", Description: "🤝 Chia sẻ bot cho bạn bè"},
	}
	cmdCtx, cmdCancel := context.WithTimeout(ctx, 10*time.Second)
	if err := client.SetMyCommands(cmdCtx, commands); err != nil {
		slog.Warn("Không thể tự động đăng ký commands với Telegram", "err", err)
	} else {
		slog.Info("✅ Đã đăng ký thành công Menu Commands với Telegram API!")
	}
	cmdCancel()

	syncBotDisplayName(ctx, client)

	// Worker pool giới hạn tối đa 64 goroutines đồng thời theo quy chuẩn chống OOM.
	sem := make(chan struct{}, 64)
	var wg sync.WaitGroup
	dispatchUpdate := func(u Update) {
		select {
		case sem <- struct{}{}:
		case <-ctx.Done():
			return
		}
		wg.Add(1)
		go func() {
			defer func() {
				<-sem
				wg.Done()
			}()
			opCtx, cancel := context.WithTimeout(ctx, 45*time.Second)
			defer cancel()

			if u.Message != nil {
				HandleMessage(opCtx, client, u.Message, appDB, cfg, dealParser)
			}
			if u.CallbackQuery != nil {
				HandleCallback(opCtx, client, u.CallbackQuery, cfg)
			}
		}()
	}

	if cfg.PublicBaseURL != "" && webhookCh != nil {
		if webReadyCh != nil {
			select {
			case <-webReadyCh:
			case <-time.After(15 * time.Second):
			case <-ctx.Done():
				return nil
			}
		}

		webhookEndpoint := strings.TrimRight(cfg.PublicBaseURL, "/") + "/telegram/webhook"
		if err := setupWebhookWithRetry(ctx, client, webhookEndpoint, cfg.TelegramWebhookSecret); err == nil {
			slog.Info("🌐 Bot đang chạy ở chế độ Webhook 24/7", "endpoint", webhookEndpoint)
			for {
				select {
				case <-ctx.Done():
					wg.Wait()
					return nil
				case u, ok := <-webhookCh:
					if !ok {
						wg.Wait()
						return nil
					}
					dispatchUpdate(u)
				}
			}
		} else {
			slog.Error("⚠️ Không thể kích hoạt Webhook — tự động chuyển về Long Polling", "endpoint", webhookEndpoint, "err", err)
		}
	}

	// Chế độ Long Polling (Local hoặc fallback khi Webhook chưa sẵn sàng)
	delCtx, delCancel := context.WithTimeout(ctx, 10*time.Second)
	if err := client.DeleteWebhook(delCtx, false); err != nil {
		slog.Warn("Không thể gỡ Webhook cũ trước khi bật Long Polling", "err", err)
	}
	delCancel()

	slog.Info("Bot đã sẵn sàng lắng nghe tin nhắn qua Long Polling!")
	var offset int64
	for {
		if ctx.Err() != nil {
			wg.Wait()
			return nil
		}
		updates, err := client.GetUpdates(ctx, offset, 30)
		if err != nil {
			if ctx.Err() != nil {
				wg.Wait()
				return nil
			}
			slog.Warn("Lỗi Long Polling Telegram, thử lại sau 2s", "err", err)
			select {
			case <-time.After(2 * time.Second):
			case <-ctx.Done():
				wg.Wait()
				return nil
			}
			continue
		}
		for _, u := range updates {
			if u.UpdateID >= offset {
				offset = u.UpdateID + 1
			}
			dispatchUpdate(u)
		}
	}
}

func syncBotDisplayName(ctx context.Context, client *Client) {
	opCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()

	current, err := client.GetMyName(opCtx, "")
	switch {
	case err == nil && strings.TrimSpace(current) == botDisplayName:
		slog.Info("✅ Tên hiển thị Bot Telegram đã đồng bộ", "name", botDisplayName)
	default:
		if err != nil {
			slog.Warn("Không thể đọc tên hiện tại của Bot Telegram", "err", err)
		}
		if setErr := client.SetMyName(opCtx, botDisplayName, ""); setErr != nil {
			slog.Warn("Không thể cập nhật tên Bot Telegram", "target", botDisplayName, "err", setErr)
		} else {
			slog.Info("✅ Đã cập nhật tên hiển thị Bot Telegram", "name", botDisplayName)
		}
	}

	viName, viErr := client.GetMyName(opCtx, "vi")
	if viErr != nil {
		slog.Warn("Không thể kiểm tra tên riêng tiếng Việt của Bot Telegram", "err", viErr)
		return
	}
	trimmedVi := strings.TrimSpace(viName)
	if trimmedVi != "" && trimmedVi != botDisplayName {
		if clrErr := client.SetMyName(opCtx, "", "vi"); clrErr != nil {
			slog.Warn("Không thể gỡ tên riêng tiếng Việt cũ của Bot Telegram", "err", clrErr)
		} else {
			slog.Info("✅ Đã gỡ tên riêng tiếng Việt cũ để đồng bộ theo tên chuẩn", "name", botDisplayName)
		}
	}
}

func setupWebhookWithRetry(ctx context.Context, client *Client, webhookEndpoint, secretToken string) error {
	var lastErr error
	for attempt := 1; attempt <= 3; attempt++ {
		opCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
		err := client.SetWebhook(opCtx, webhookEndpoint, secretToken, false)
		cancel()
		if err == nil {
			return nil
		}
		lastErr = err
		slog.Warn("Đăng ký Telegram Webhook thất bại", "attempt", attempt, "max", 3, "err", err)
		if attempt < 3 {
			select {
			case <-time.After(2 * time.Second):
			case <-ctx.Done():
				return ctx.Err()
			}
		}
	}
	return fmt.Errorf("SetWebhook thất bại sau 3 lần thử: %w", lastErr)
}
