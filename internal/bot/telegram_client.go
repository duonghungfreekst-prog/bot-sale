package bot

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const telegramAPIBase = "https://api.telegram.org"

// User biểu diễn thông tin người dùng Telegram.
type User struct {
	ID        int64  `json:"id"`
	IsBot     bool   `json:"is_bot,omitempty"`
	FirstName string `json:"first_name"`
	LastName  string `json:"last_name,omitempty"`
	Username  string `json:"username,omitempty"`
}

// FullName ghép tên hiển thị giống teloxide `user.full_name()`.
func (u *User) FullName() string {
	if u == nil {
		return "Bạn"
	}
	first := strings.TrimSpace(u.FirstName)
	last := strings.TrimSpace(u.LastName)
	switch {
	case first != "" && last != "":
		return first + " " + last
	case first != "":
		return first
	case last != "":
		return last
	default:
		return "Bạn"
	}
}

// Chat biểu diễn chat Telegram.
type Chat struct {
	ID   int64  `json:"id"`
	Type string `json:"type,omitempty"`
}

// Message biểu diễn một tin nhắn đến từ Telegram.
type Message struct {
	MessageID int64  `json:"message_id"`
	From      *User  `json:"from,omitempty"`
	Chat      Chat   `json:"chat"`
	Text      string `json:"text,omitempty"`
	Caption   string `json:"caption,omitempty"`
}

// CallbackQuery biểu diễn sự kiện bấm nút Inline Keyboard.
type CallbackQuery struct {
	ID      string   `json:"id"`
	From    *User    `json:"from,omitempty"`
	Message *Message `json:"message,omitempty"`
	Data    string   `json:"data,omitempty"`
}

// Update biểu diễn một bản tin Telegram Bot API Update.
type Update struct {
	UpdateID      int64          `json:"update_id"`
	Message       *Message       `json:"message,omitempty"`
	CallbackQuery *CallbackQuery `json:"callback_query,omitempty"`
}

// BotCommand định nghĩa lệnh hiển thị ở nút Menu của Telegram.
type BotCommand struct {
	Command     string `json:"command"`
	Description string `json:"description"`
}

// LinkPreviewOptions cấu hình tắt khung xem trước liên kết.
type LinkPreviewOptions struct {
	IsDisabled       bool   `json:"is_disabled"`
	URL              string `json:"url,omitempty"`
	PreferSmallMedia bool   `json:"prefer_small_media,omitempty"`
	PreferLargeMedia bool   `json:"prefer_large_media,omitempty"`
	ShowAboveText    bool   `json:"show_above_text,omitempty"`
}

// InlineKeyboardButton là một nút bấm trên InlineKeyboardMarkup.
type InlineKeyboardButton struct {
	Text         string `json:"text"`
	URL          string `json:"url,omitempty"`
	CallbackData string `json:"callback_data,omitempty"`
}

// InlineKeyboardMarkup là bàn phím đính kèm dưới tin nhắn.
type InlineKeyboardMarkup struct {
	InlineKeyboard [][]InlineKeyboardButton `json:"inline_keyboard"`
}

// KeyboardButton là một nút trong ReplyKeyboardMarkup.
type KeyboardButton struct {
	Text string `json:"text"`
}

// ReplyKeyboardMarkup là bàn phím thường trực dưới ô chat.
type ReplyKeyboardMarkup struct {
	Keyboard       [][]KeyboardButton `json:"keyboard"`
	ResizeKeyboard bool               `json:"resize_keyboard,omitempty"`
	IsPersistent   bool               `json:"is_persistent,omitempty"`
}

// SendMessageOpts là tùy chọn gửi tin nhắn / ảnh.
type SendMessageOpts struct {
	ParseMode          string
	LinkPreviewOptions *LinkPreviewOptions
	ReplyMarkup        any
}

// Client gọi Telegram Bot API qua HTTP với cơ chế ẩn token khỏi thông báo lỗi.
type Client struct {
	token      string
	baseURL    string
	httpClient *http.Client
}

// NewClient khởi tạo Telegram Bot API client với timeout hợp lý cho cả API call và Long Polling.
func NewClient(token string) *Client {
	return &Client{
		token:   strings.TrimSpace(token),
		baseURL: telegramAPIBase,
		httpClient: &http.Client{
			Timeout: 45 * time.Second,
		},
	}
}

func (c *Client) redactTokenErr(err error) error {
	if err == nil {
		return nil
	}
	msg := err.Error()
	if c.token != "" {
		msg = strings.ReplaceAll(msg, c.token, "***")
	}
	return fmt.Errorf("%s", msg)
}

type apiResponse struct {
	OK          bool            `json:"ok"`
	Description string          `json:"description,omitempty"`
	ErrorCode   int             `json:"error_code,omitempty"`
	Result      json.RawMessage `json:"result,omitempty"`
}

func (c *Client) callJSON(ctx context.Context, method string, payload any, out any) error {
	var bodyReader io.Reader
	if payload != nil {
		raw, err := json.Marshal(payload)
		if err != nil {
			return fmt.Errorf("marshal %s payload: %w", method, err)
		}
		bodyReader = bytes.NewReader(raw)
	} else {
		bodyReader = bytes.NewReader([]byte("{}"))
	}

	endpoint := fmt.Sprintf("%s/bot%s/%s", strings.TrimRight(c.baseURL, "/"), c.token, method)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bodyReader)
	if err != nil {
		return c.redactTokenErr(err)
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return c.redactTokenErr(err)
	}
	defer resp.Body.Close()

	limited := io.LimitReader(resp.Body, 4*1024*1024)
	respBytes, err := io.ReadAll(limited)
	if err != nil {
		return c.redactTokenErr(err)
	}

	var parsed apiResponse
	if err := json.Unmarshal(respBytes, &parsed); err != nil {
		return fmt.Errorf("telegram %s status %d invalid json: %w", method, resp.StatusCode, err)
	}
	if !parsed.OK {
		return fmt.Errorf("telegram %s error %d: %s", method, parsed.ErrorCode, parsed.Description)
	}
	if out != nil && len(parsed.Result) > 0 {
		if err := json.Unmarshal(parsed.Result, out); err != nil {
			return fmt.Errorf("decode telegram %s result: %w", method, err)
		}
	}
	return nil
}

// GetMyName đọc tên hiện tại của bot (mặc định hoặc theo language_code).
func (c *Client) GetMyName(ctx context.Context, languageCode string) (string, error) {
	payload := map[string]any{}
	if languageCode != "" {
		payload["language_code"] = languageCode
	}
	var res struct {
		Name string `json:"name"`
	}
	if err := c.callJSON(ctx, "getMyName", payload, &res); err != nil {
		return "", err
	}
	return res.Name, nil
}

// SetMyName cập nhật tên hiển thị bot.
func (c *Client) SetMyName(ctx context.Context, name, languageCode string) error {
	payload := map[string]any{
		"name": name,
	}
	if languageCode != "" {
		payload["language_code"] = languageCode
	}
	return c.callJSON(ctx, "setMyName", payload, nil)
}

// SetMyCommands đăng ký danh sách lệnh bot.
func (c *Client) SetMyCommands(ctx context.Context, commands []BotCommand) error {
	return c.callJSON(ctx, "setMyCommands", map[string]any{
		"commands": commands,
	}, nil)
}

// SetWebhook đăng ký Telegram Webhook kèm secret_token.
func (c *Client) SetWebhook(ctx context.Context, webhookURL, secretToken string, dropPending bool) error {
	if _, err := url.ParseRequestURI(webhookURL); err != nil {
		return fmt.Errorf("invalid webhook URL: %w", err)
	}
	payload := map[string]any{
		"url":                  webhookURL,
		"drop_pending_updates": dropPending,
	}
	if secretToken != "" {
		payload["secret_token"] = secretToken
	}
	return c.callJSON(ctx, "setWebhook", payload, nil)
}

// DeleteWebhook gỡ Webhook cũ trước khi chạy Long Polling.
func (c *Client) DeleteWebhook(ctx context.Context, dropPending bool) error {
	return c.callJSON(ctx, "deleteWebhook", map[string]any{
		"drop_pending_updates": dropPending,
	}, nil)
}

// GetUpdates thực hiện Long Polling lấy danh sách Update mới.
func (c *Client) GetUpdates(ctx context.Context, offset int64, timeoutSec int) ([]Update, error) {
	payload := map[string]any{
		"offset":          offset,
		"timeout":         timeoutSec,
		"allowed_updates": []string{"message", "callback_query"},
	}
	var updates []Update
	if err := c.callJSON(ctx, "getUpdates", payload, &updates); err != nil {
		return nil, err
	}
	return updates, nil
}

// SendMessage gửi tin nhắn văn bản tới chatID.
func (c *Client) SendMessage(ctx context.Context, chatID int64, text string, opts SendMessageOpts) (*Message, error) {
	payload := map[string]any{
		"chat_id": chatID,
		"text":    text,
	}
	if opts.ParseMode != "" {
		payload["parse_mode"] = opts.ParseMode
	}
	if opts.LinkPreviewOptions != nil {
		payload["link_preview_options"] = opts.LinkPreviewOptions
	}
	if opts.ReplyMarkup != nil {
		payload["reply_markup"] = opts.ReplyMarkup
	}
	var msg Message
	if err := c.callJSON(ctx, "sendMessage", payload, &msg); err != nil {
		return nil, err
	}
	return &msg, nil
}

// SendPhoto gửi ảnh sản phẩm kèm caption tới chatID.
func (c *Client) SendPhoto(ctx context.Context, chatID int64, photoURL, caption string, opts SendMessageOpts) (*Message, error) {
	if _, err := url.ParseRequestURI(photoURL); err != nil {
		return nil, fmt.Errorf("invalid photo URL: %w", err)
	}
	payload := map[string]any{
		"chat_id": chatID,
		"photo":   photoURL,
	}
	if caption != "" {
		payload["caption"] = caption
	}
	if opts.ParseMode != "" {
		payload["parse_mode"] = opts.ParseMode
	}
	if opts.ReplyMarkup != nil {
		payload["reply_markup"] = opts.ReplyMarkup
	}
	var msg Message
	if err := c.callJSON(ctx, "sendPhoto", payload, &msg); err != nil {
		return nil, err
	}
	return &msg, nil
}

// DeleteMessage xóa tin nhắn chờ hoặc tin nhắn tạm thời.
func (c *Client) DeleteMessage(ctx context.Context, chatID, messageID int64) error {
	if messageID == 0 {
		return nil
	}
	return c.callJSON(ctx, "deleteMessage", map[string]any{
		"chat_id":    chatID,
		"message_id": messageID,
	}, nil)
}

// AnswerCallbackQuery xác nhận đã xử lý callback bấm nút.
func (c *Client) AnswerCallbackQuery(ctx context.Context, callbackQueryID string) error {
	if callbackQueryID == "" {
		return nil
	}
	return c.callJSON(ctx, "answerCallbackQuery", map[string]any{
		"callback_query_id": callbackQueryID,
	}, nil)
}
