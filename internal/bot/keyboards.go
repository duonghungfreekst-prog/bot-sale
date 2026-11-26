package bot

import (
	"net/url"

	"salehunter-go/internal/utils"
)

// LabelURL biểu diễn cặp nhãn nút và URL đích.
type LabelURL struct {
	Label string
	URL   string
}

// PersistentReplyKeyboard trả về bàn phím Reply Keyboard thường trực dưới khung chat.
func PersistentReplyKeyboard() *ReplyKeyboardMarkup {
	return &ReplyKeyboardMarkup{
		Keyboard: [][]KeyboardButton{
			{
				{Text: "📖 Hướng Dẫn Săn Sale"},
				{Text: "🚀 Chia Sẻ Cho Bạn Bè"},
			},
		},
		ResizeKeyboard: true,
		IsPersistent:   true,
	}
}

// MainMenuKeyboard trả về Inline Keyboard cho Menu chính.
func MainMenuKeyboard(botUsername string) *InlineKeyboardMarkup {
	shareText := "Dán link sản phẩm Shopee/Lazada/Tiki/TikTok vào bot này để lấy link mua kèm mã giảm giá phù hợp nè: https://t.me/" + botUsername
	shareURL := "https://t.me/share/url?url=" +
		utils.PercentEncodeRFC3986("https://t.me/"+botUsername) +
		"&text=" +
		utils.PercentEncodeRFC3986(shareText)

	if _, err := url.ParseRequestURI(shareURL); err != nil {
		shareURL = "https://t.me"
	}

	return &InlineKeyboardMarkup{
		InlineKeyboard: [][]InlineKeyboardButton{
			{
				{
					Text:         "📖 Hướng Dẫn Áp Mã Thành Công 100%",
					CallbackData: "menu_guide",
				},
			},
			{
				{
					Text: "🚀 Chia Sẻ Thế Giới Siêu Sale Cho Bạn Bè",
					URL:  shareURL,
				},
			},
		},
	}
}

// DealActionKeyboard trả về bàn phím hành động khi người dùng gửi 1 link sản phẩm.
func DealActionKeyboard(
	wrappedProductURL string,
	platform string,
	voucherLinks []LabelURL,
	portalLinks []LabelURL,
) *InlineKeyboardMarkup {
	targetURL := wrappedProductURL
	if _, err := url.ParseRequestURI(targetURL); err != nil {
		targetURL = "https://shopee.vn"
	}

	rows := [][]InlineKeyboardButton{
		{
			{
				Text: "🛍️ Mở " + platform + " & Mua Ngay 🛍️",
				URL:  targetURL,
			},
		},
	}

	var voucherRow []InlineKeyboardButton
	for i, item := range voucherLinks {
		if i >= 2 {
			break
		}
		if _, err := url.ParseRequestURI(item.URL); err == nil {
			voucherRow = append(voucherRow, InlineKeyboardButton{
				Text: item.Label,
				URL:  item.URL,
			})
		}
	}
	if len(voucherRow) > 0 {
		rows = append(rows, voucherRow)
	}

	var portalRow []InlineKeyboardButton
	for _, item := range portalLinks {
		if _, err := url.ParseRequestURI(item.URL); err == nil {
			portalRow = append(portalRow, InlineKeyboardButton{
				Text: item.Label,
				URL:  item.URL,
			})
		}
	}
	if len(portalRow) > 0 {
		rows = append(rows, portalRow)
	}

	return &InlineKeyboardMarkup{InlineKeyboard: rows}
}

// BatchActionKeyboard trả về bàn phím hành động cho tin nhắn gom nhiều link (tối đa 5 nút).
func BatchActionKeyboard(items []LabelURL) *InlineKeyboardMarkup {
	var rows [][]InlineKeyboardButton
	for i, item := range items {
		if i >= 5 {
			break
		}
		if _, err := url.ParseRequestURI(item.URL); err == nil {
			rows = append(rows, []InlineKeyboardButton{
				{
					Text: item.Label,
					URL:  item.URL,
				},
			})
		}
	}
	return &InlineKeyboardMarkup{InlineKeyboard: rows}
}
