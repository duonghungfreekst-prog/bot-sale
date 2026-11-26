package orders

import (
	"bytes"
	"encoding/json"
	"fmt"
	"log/slog"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"salehunter-go/internal/config"
	"salehunter-go/internal/db"
	"salehunter-go/internal/services/accesstrade"
	"salehunter-go/internal/services/shopee"
	"salehunter-go/internal/utils"
)

var (
	orderClientOnce sync.Once
	orderHTTPClient *http.Client
)

func getOrderHTTPClient() *http.Client {
	orderClientOnce.Do(func() {
		orderHTTPClient = &http.Client{Timeout: 10 * time.Second}
	})
	return orderHTTPClient
}

// OrderSyncSummary kết quả 1 lần quét đơn đối soát.
type OrderSyncSummary struct {
	Fetched       int
	NewlyInserted int
	Errors        []string
}

// FormatOrderAlertHTML định dạng tin nhắn HTML báo đơn hàng mới gửi tới Telegram Admin.
func FormatOrderAlertHTML(order db.TrackedOrder) string {
	subLine := ""
	if strings.TrimSpace(order.SubID) != "" {
		subLine = fmt.Sprintf("\n🏷️ <b>SubID:</b> <code>%s</code>", utils.EscapeHTML(order.SubID))
	}
	timeLine := ""
	if strings.TrimSpace(order.OrderTime) != "" {
		timeLine = fmt.Sprintf("\n🕒 <b>Thời gian:</b> <code>%s</code>", utils.EscapeHTML(order.OrderTime))
	}
	return fmt.Sprintf(
		"🎉 <b>CÓ ĐƠN HÀNG MỚI TỪ %s!</b>\n\n"+
			"🧾 <b>Mã đơn:</b> <code>%s</code>\n"+
			"📌 <b>Trạng thái:</b> <b>%s</b>\n"+
			"🛒 <b>Giá trị đơn:</b> <b>%s</b>\n"+
			"💰 <b>Hoa hồng dự kiến:</b> <b>%s</b>%s%s",
		utils.EscapeHTML(order.Platform),
		utils.EscapeHTML(order.OrderID),
		utils.EscapeHTML(order.Status),
		utils.EscapeHTML(utils.FormatVND(order.OrderAmount)),
		utils.EscapeHTML(utils.FormatVND(order.Commission)),
		subLine,
		timeLine,
	)
}

func sendTelegramHTML(client *http.Client, token string, chatID int64, htmlText string) error {
	payload, err := json.Marshal(map[string]any{
		"chat_id":    chatID,
		"text":       htmlText,
		"parse_mode": "HTML",
	})
	if err != nil {
		return err
	}
	endpoint := fmt.Sprintf("https://api.telegram.org/bot%s/sendMessage", token)
	req, err := http.NewRequest(http.MethodPost, endpoint, bytes.NewReader(payload))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	resp, err := client.Do(req)
	if err != nil {
		return err
	}
	_ = resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("telegram sendMessage trả HTTP %d", resp.StatusCode)
	}
	return nil
}

// SyncOrdersOnce quét đơn hàng từ ACCESSTRADE & Shopee Open API, lưu DB chống trùng và báo Admin.
func SyncOrdersOnce(appDB *db.AppDb, cfg config.Config, at *accesstrade.AccessTradeClient) OrderSyncSummary {
	var summary OrderSyncSummary
	if appDB == nil || appDB.DB == nil {
		summary.Errors = append(summary.Errors, "Không thể mở kết nối DB để đối soát đơn")
		return summary
	}

	settings, _ := appDB.GetSettingsMapCached()
	getCfg := func(k string) string {
		return strings.TrimSpace(settings[k])
	}

	atToken := getCfg("accesstrade_api_token")
	if atToken == "" {
		atToken = cfg.AccessTradeAPIToken
	}
	shopeeAppID := getCfg("shopee_app_id")
	if shopeeAppID == "" {
		shopeeAppID = cfg.ShopeeAppID
	}
	shopeeAppSecret := getCfg("shopee_app_secret")
	if shopeeAppSecret == "" {
		shopeeAppSecret = cfg.ShopeeAppSecret
	}
	adminTelegramID := cfg.AdminTelegramID
	if rawID := getCfg("admin_telegram_id"); rawID != "" {
		if v, err := strconv.ParseInt(rawID, 10, 64); err == nil && v != 0 {
			adminTelegramID = v
		}
	}

	var allOrders []db.TrackedOrder
	if atToken != "" {
		list, err := at.FetchRecentOrders(atToken, 7)
		if err != nil {
			summary.Errors = append(summary.Errors, "ACCESSTRADE: "+err.Error())
		} else {
			allOrders = append(allOrders, list...)
		}
	}

	if shopeeAppID != "" && shopeeAppSecret != "" {
		list, err := shopee.FetchOpenAPIRecentOrders(getOrderHTTPClient(), shopeeAppID, shopeeAppSecret, 7)
		if err != nil {
			summary.Errors = append(summary.Errors, "Shopee Open API: "+err.Error())
		} else {
			allOrders = append(allOrders, list...)
		}
	}

	summary.Fetched = len(allOrders)
	var newlyAdded []db.TrackedOrder
	for _, ord := range allOrders {
		isNew, err := db.UpsertTrackedOrder(appDB.DB, ord)
		if err != nil {
			slog.Warn("Lỗi lưu đơn hàng", "order_key", utils.SanitizeCRLF(ord.OrderKey), "err", err)
			continue
		}
		if isNew {
			summary.NewlyInserted++
			newlyAdded = append(newlyAdded, ord)
		}
	}

	if len(newlyAdded) > 0 {
		slog.Info("🎉 Đối soát phát hiện đơn hàng mới", "newly", len(newlyAdded), "fetched", summary.Fetched)
		if adminTelegramID != 0 && !strings.Contains(cfg.TelegramToken, "DEMO_TOKEN") && strings.TrimSpace(cfg.TelegramToken) != "" {
			limit := len(newlyAdded)
			if limit > 5 {
				limit = 5
			}
			for _, ord := range newlyAdded[:limit] {
				if err := sendTelegramHTML(getOrderHTTPClient(), cfg.TelegramToken, adminTelegramID, FormatOrderAlertHTML(ord)); err != nil {
					slog.Warn("Không thể gửi thông báo đơn mới cho Admin", "admin_id", adminTelegramID, "err", err)
				}
			}
		}
	}

	return summary
}

// SpawnOrderTrackerTask chạy nền đối soát đơn hàng mỗi 10 phút.
func SpawnOrderTrackerTask(stopCh <-chan struct{}, appDB *db.AppDb, cfg config.Config, at *accesstrade.AccessTradeClient) {
	go func() {
		select {
		case <-stopCh:
			return
		case <-time.After(35 * time.Second):
		}

		ticker := time.NewTicker(600 * time.Second)
		defer ticker.Stop()

		runOnce := func() {
			summary := SyncOrdersOnce(appDB, cfg, at)
			if summary.NewlyInserted > 0 || len(summary.Errors) > 0 {
				slog.Info("[ORDER_TRACKER] Đã quét đơn hàng",
					"fetched", summary.Fetched,
					"newly_inserted", summary.NewlyInserted,
					"errors", len(summary.Errors),
				)
			}
		}

		runOnce()
		for {
			select {
			case <-stopCh:
				return
			case <-ticker.C:
				runOnce()
			}
		}
	}()
}
