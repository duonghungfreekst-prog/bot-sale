package bot

import (
	"context"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"salehunter-go/internal/config"
	"salehunter-go/internal/db"
	"salehunter-go/internal/services/accesstrade"
	"salehunter-go/internal/services/parser"
	"salehunter-go/internal/services/shopee"
	"salehunter-go/internal/services/social"
	"salehunter-go/internal/utils"
)

// Command biểu diễn các lệnh slash hỗ trợ bởi bot.
type Command string

const (
	CmdStart Command = "start"
	CmdHelp  Command = "help"
	CmdShare Command = "share"
	CmdStats Command = "stats"
)

// ParseCommand phân tích lệnh slash (có hỗ trợ dạng /start@bot_username).
func ParseCommand(text, botUsername string) (Command, bool) {
	trimmed := strings.TrimSpace(text)
	if !strings.HasPrefix(trimmed, "/") {
		return "", false
	}
	firstToken := strings.Fields(trimmed)[0]
	cmdPart := strings.TrimPrefix(firstToken, "/")
	if atIdx := strings.IndexByte(cmdPart, '@'); atIdx >= 0 {
		targetBot := strings.ToLower(strings.TrimSpace(cmdPart[atIdx+1:]))
		if botUsername != "" && targetBot != "" && targetBot != strings.ToLower(strings.TrimSpace(botUsername)) {
			return "", false
		}
		cmdPart = cmdPart[:atIdx]
	}
	switch Command(strings.ToLower(cmdPart)) {
	case CmdStart:
		return CmdStart, true
	case CmdHelp:
		return CmdHelp, true
	case CmdShare:
		return CmdShare, true
	case CmdStats:
		return CmdStats, true
	default:
		return "", false
	}
}

func disabledLinkPreview() *LinkPreviewOptions {
	return &LinkPreviewOptions{
		IsDisabled: true,
	}
}

// PlatformMatches so sánh tên sàn bất kể khoảng trắng và hoa thường.
func PlatformMatches(dealPlatform, parsedPlatform string) bool {
	normA := strings.ToLower(strings.ReplaceAll(strings.TrimSpace(dealPlatform), " ", ""))
	normB := strings.ToLower(strings.ReplaceAll(strings.TrimSpace(parsedPlatform), " ", ""))
	return normA == normB
}

// ShouldProbeAdminSocialInput chỉ probe chữ ký FB/IG cho Admin khi tin nhắn có tham số/cầu nối tiềm năng.
func ShouldProbeAdminSocialInput(text string) bool {
	lower := strings.ToLower(text)
	if strings.Contains(lower, "sm_sig_id=") ||
		strings.Contains(lower, "encrypted_payload=") ||
		strings.Contains(lower, "fb_content_id=") ||
		strings.Contains(lower, "channel_type=") {
		return true
	}
	for _, skip := range []string{
		"shopee.vn/product/",
		"shopee.vn/share-video/",
		"vn.shp.ee/",
		"shp.ee/",
		"tiktok.com",
		"tiki.vn",
		"ti.ki",
		"lazada.vn",
		"lzd.co",
		"alibaba.com",
	} {
		if strings.Contains(lower, skip) {
			return false
		}
	}
	return true
}

// FormatTelegramLinkDisplay định dạng hiển thị link gọn gàng trên Telegram.
func FormatTelegramLinkDisplay(rawURL, platform string, isWrapped bool) string {
	trimmed := strings.TrimSpace(rawURL)
	if isWrapped || utf8.RuneCountInString(trimmed) <= 75 {
		return utils.EscapeHTML(trimmed)
	}
	return fmt.Sprintf(
		`<a href="%s">🔗 Bấm mở %s kèm ưu đãi ↗</a>`,
		utils.EscapeHTML(trimmed),
		utils.EscapeHTML(platform),
	)
}

// OwnedAffSettings chứa cấu hình Affiliate nạp từ DB ưu tiên trước ENV.
type OwnedAffSettings struct {
	DeeplinkTemplate   string
	AccessTradeToken   string
	CampaignShopee     string
	CampaignLazada     string
	CampaignTiki       string
	CampaignTikTok     string
	ShopeeAffiliateID  string
	ShopeeAppID        string
	ShopeeAppSecret    string
	ShopeeSocialParams string
	ShortlinkBase      string
}

func (o *OwnedAffSettings) ToParserSettings() parser.AffiliateSettings {
	return parser.AffiliateSettings{
		DeeplinkTemplate:   o.DeeplinkTemplate,
		AccessTradeToken:   o.AccessTradeToken,
		CampaignShopee:     o.CampaignShopee,
		CampaignLazada:     o.CampaignLazada,
		CampaignTiki:       o.CampaignTiki,
		CampaignTikTok:     o.CampaignTikTok,
		ShopeeAffiliateID:  o.ShopeeAffiliateID,
		ShopeeAppID:        o.ShopeeAppID,
		ShopeeAppSecret:    o.ShopeeAppSecret,
		ShopeeSocialParams: o.ShopeeSocialParams,
	}
}

// BuildOwnedAffSettingsFromMap hợp nhất settings DB với Config gốc (bản Hosting headless).
func BuildOwnedAffSettingsFromMap(settingsMap map[string]string, cfg config.Config) OwnedAffSettings {
	readOpt := func(k string) string {
		if v, ok := settingsMap[k]; ok && strings.TrimSpace(v) != "" {
			return v
		}
		return ""
	}
	firstNonEmpty := func(a, b string) string {
		if strings.TrimSpace(a) != "" {
			return a
		}
		return b
	}

	deeplinkTemplate := firstNonEmpty(readOpt("affiliate_template"), cfg.AccessTradeDeeplinkTemplate)
	accesstradeToken := firstNonEmpty(readOpt("accesstrade_api_token"), cfg.AccessTradeAPIToken)
	shopeeAffID := firstNonEmpty(readOpt("shopee_affiliate_id"), cfg.ShopeeAffiliateID)
	shopeeAppID := firstNonEmpty(readOpt("shopee_app_id"), cfg.ShopeeAppID)
	shopeeAppSecret := firstNonEmpty(readOpt("shopee_app_secret"), cfg.ShopeeAppSecret)
	shopeeSocialParams := firstNonEmpty(readOpt("shopee_social_params"), cfg.ShopeeSocialParams)

	hasNamedTunnel := readOpt("cloudflare_tunnel_token") != "" || strings.TrimSpace(os.Getenv("CLOUDFLARE_TUNNEL_TOKEN")) != ""
	shortlinkBase := config.ResolvePCRuntimeShortlinkBaseURL(
		readOpt("shortlink_base_url"),
		cfg.ShortlinkBaseURL,
		cfg.PublicBaseURL,
		"",
		hasNamedTunnel,
	)

	return OwnedAffSettings{
		DeeplinkTemplate:   deeplinkTemplate,
		AccessTradeToken:   accesstradeToken,
		CampaignShopee:     readOpt("campaign_shopee"),
		CampaignLazada:     readOpt("campaign_lazada"),
		CampaignTiki:       readOpt("campaign_tiki"),
		CampaignTikTok:     readOpt("campaign_tiktok"),
		ShopeeAffiliateID:  shopeeAffID,
		ShopeeAppID:        shopeeAppID,
		ShopeeAppSecret:    shopeeAppSecret,
		ShopeeSocialParams: shopeeSocialParams,
		ShortlinkBase:      shortlinkBase,
	}
}

// LoadOwnedAffSettingsCached đọc cấu hình Affiliate qua L1 cache của AppDb.
func LoadOwnedAffSettingsCached(appDB *db.AppDb, cfg config.Config) OwnedAffSettings {
	m, err := appDB.GetSettingsMapCached()
	if err != nil {
		m = map[string]string{}
	}
	return BuildOwnedAffSettingsFromMap(m, cfg)
}

func resolveAdminIDFromCache(appDB *db.AppDb, cfg config.Config) int64 {
	m, err := appDB.GetSettingsMapCached()
	if err == nil {
		if raw, ok := m["admin_telegram_id"]; ok {
			if parsed, pErr := strconv.ParseInt(strings.TrimSpace(raw), 10, 64); pErr == nil && parsed != 0 {
				return parsed
			}
		}
	}
	return cfg.AdminTelegramID
}

// EnrichDealURLForPlatform gắn Shopee Affiliate ID (an_redir) & chữ ký FB/IG Reels cho link mã ưu đãi Shopee.
func EnrichDealURLForPlatform(rawURL, platform, shopeeAffID, shopeeSocialParams, subID string) string {
	if !strings.EqualFold(strings.TrimSpace(platform), "Shopee") {
		return rawURL
	}
	trimmed := strings.TrimSpace(rawURL)
	lower := strings.ToLower(trimmed)
	baseURL := trimmed
	affID := strings.TrimSpace(shopeeAffID)
	if shopee.IsValidShopeeAffiliateID(affID) {
		if !strings.Contains(lower, "s.shopee.vn/an_redir") && !strings.Contains(lower, "mmp_pid=an_") {
			baseURL = shopee.BuildDirectAnRedirLink(trimmed, affID, subID)
		}
	}
	return shopee.AppendSocialParamsToShopeeLink(baseURL, shopeeSocialParams)
}

func parseDiscountPctHint(text string) uint32 {
	var best uint32
	for i := 0; i < len(text); i++ {
		if text[i] == '%' && i > 0 {
			start := i
			for start > 0 && text[start-1] >= '0' && text[start-1] <= '9' {
				start--
			}
			if start < i {
				if v, err := strconv.ParseUint(text[start:i], 10, 32); err == nil && uint32(v) <= 99 {
					if uint32(v) > best {
						best = uint32(v)
					}
				}
			}
		}
	}
	return best
}

// IsExclusiveDealRelevant kiểm tra mã độc quyền trong DB có thực sự áp được cho sản phẩm đang dán hay không.
func IsExclusiveDealRelevant(deal db.DealItem, parsed *parser.ParsedDeal) bool {
	if parsed == nil || !PlatformMatches(deal.Platform, parsed.Platform.AsStr()) {
		return false
	}

	urlLower := strings.ToLower(deal.DealURL)
	descLower := strings.ToLower(deal.DiscountDesc)
	hasDirectVoucherTarget := strings.Contains(urlLower, "evcode=") ||
		strings.Contains(urlLower, "promotionid=") ||
		strings.Contains(urlLower, "shopee.vn/search?")

	if social.IsGuideNoiseDealText(deal.DiscountDesc) &&
		(!hasDirectVoucherTarget ||
			strings.Contains(descLower, "hướng dẫn lấy mã") ||
			strings.Contains(descLower, "khi ấn link lấy mã")) {
		return false
	}

	if parsed.ProductPriceVND != nil && *parsed.ProductPriceVND > 0 {
		if deal.MinSpend > 0 && *parsed.ProductPriceVND < deal.MinSpend {
			return false
		}
	}

	trimmedTitle := strings.TrimSpace(deal.Title)
	if strings.HasPrefix(trimmedTitle, "[") {
		if endIdx := strings.IndexByte(trimmedTitle, ']'); endIdx > 0 {
			tag := strings.ToLower(strings.TrimSpace(trimmedTitle[1:endIdx]))
			plat := strings.ToLower(parsed.Platform.AsStr())
			isGlobalTag := tag == "" ||
				tag == plat ||
				strings.Contains(tag, "toàn sàn") ||
				strings.Contains(tag, "chung")
			if isGlobalTag {
				return true
			}
			shopMatch := parsed.ProductShop != nil &&
				(strings.Contains(strings.ToLower(*parsed.ProductShop), tag) || strings.Contains(tag, strings.ToLower(*parsed.ProductShop)))
			brandMatch := parsed.ProductBrand != nil &&
				(strings.Contains(strings.ToLower(*parsed.ProductBrand), tag) || strings.Contains(tag, strings.ToLower(*parsed.ProductBrand)))
			titleMatch := strings.Contains(strings.ToLower(parsed.ProductTitle), tag)
			return shopMatch || brandMatch || titleMatch
		}
	}

	return true
}

// SelectTopRelevantDeals chọn tối đa limit mã độc quyền / FB-IG chất lượng nhất (ưu tiên link săn mã thật, % giảm cao, không trùng URL).
func SelectTopRelevantDeals(deals []db.DealItem, parsed *parser.ParsedDeal, limit int) []db.DealItem {
	type rankedDeal struct {
		specificity     uint8
		hasVoucherQuery bool
		pct             uint32
		id              int64
		deal            db.DealItem
	}

	ranked := make([]rankedDeal, 0, len(deals))
	for _, d := range deals {
		if !d.IsActive || !IsExclusiveDealRelevant(d, parsed) {
			continue
		}
		urlLower := strings.ToLower(d.DealURL)
		hasVoucherQuery := strings.Contains(urlLower, "evcode=") ||
			strings.Contains(urlLower, "promotionid=") ||
			strings.Contains(urlLower, "shopee.vn/search?")
		pct := parseDiscountPctHint(d.DiscountDesc)
		if tPct := parseDiscountPctHint(d.Title); tPct > pct {
			pct = tPct
		}
		var specificity uint8 = 1
		if strings.HasPrefix(d.Title, "[") && !strings.Contains(strings.ToLower(d.Title), "toàn sàn") {
			specificity = 2
		}
		ranked = append(ranked, rankedDeal{
			specificity:     specificity,
			hasVoucherQuery: hasVoucherQuery,
			pct:             pct,
			id:              d.ID,
			deal:            d,
		})
	}

	sort.SliceStable(ranked, func(i, j int) bool {
		if ranked[i].specificity != ranked[j].specificity {
			return ranked[i].specificity > ranked[j].specificity
		}
		if ranked[i].hasVoucherQuery != ranked[j].hasVoucherQuery {
			return ranked[i].hasVoucherQuery && !ranked[j].hasVoucherQuery
		}
		if ranked[i].pct != ranked[j].pct {
			return ranked[i].pct > ranked[j].pct
		}
		return ranked[i].id > ranked[j].id
	})

	seenURLs := make(map[string]struct{})
	selected := make([]db.DealItem, 0, limit)
	for _, item := range ranked {
		normURL := strings.TrimRight(strings.TrimSpace(item.deal.DealURL), "/")
		if normURL != "" {
			if _, exists := seenURLs[normURL]; exists {
				continue
			}
			seenURLs[normURL] = struct{}{}
		}
		selected = append(selected, item.deal)
		if len(selected) >= limit {
			break
		}
	}
	return selected
}

// HandleMessage xử lý tin nhắn văn bản hoặc bài đăng kèm Ảnh/Video có chứa link.
func HandleMessage(
	ctx context.Context,
	client *Client,
	msg *Message,
	appDB *db.AppDb,
	cfg config.Config,
	dealParser *parser.DealParser,
) {
	if msg == nil {
		return
	}
	rawText := msg.Text
	if strings.TrimSpace(rawText) == "" {
		rawText = msg.Caption
	}
	text := strings.TrimSpace(rawText)
	if text == "" {
		return
	}

	var userID int64
	var username *string
	fullName := "Bạn"
	if msg.From != nil {
		userID = msg.From.ID
		if msg.From.Username != "" {
			u := msg.From.Username
			username = &u
		}
		fullName = msg.From.FullName()
	}

	lowerText := strings.ToLower(text)
	var mappedCmd Command
	hasMappedCmd := false
	switch text {
	case "🚀 Bắt Đầu", "Bắt Đầu", "start":
		mappedCmd, hasMappedCmd = CmdStart, true
	case "📖 Hướng Dẫn Săn Sale", "Hướng Dẫn", "help":
		mappedCmd, hasMappedCmd = CmdHelp, true
	case "🚀 Chia Sẻ Cho Bạn Bè", "Chia Sẻ", "share":
		mappedCmd, hasMappedCmd = CmdShare, true
	}
	if hasMappedCmd {
		HandleCommand(ctx, client, msg, mappedCmd, appDB, cfg, dealParser)
		return
	}

	if text == "🎟️ Mã Hot Hôm Nay" ||
		text == "🎟️ Kho Mã 20% - 35%+" ||
		text == "🎟️ Kho Mã 20% - 35%" ||
		text == "Kho Mã 20% - 35%" ||
		lowerText == "deals" ||
		lowerText == "ma" ||
		strings.HasPrefix(lowerText, "/deals") ||
		strings.HasPrefix(lowerText, "/ma") {
		if _, err := client.SendMessage(
			ctx,
			msg.Chat.ID,
			"👉 Hãy <b>dán trực tiếp link sản phẩm Shopee, Lazada, Tiki hoặc TikTok Shop</b> vào đây để bot tạo link mua kèm mã khớp riêng cho đúng sản phẩm đó nhé!",
			SendMessageOpts{
				ParseMode:   "HTML",
				ReplyMarkup: PersistentReplyKeyboard(),
			},
		); err != nil {
			slog.Warn("Không thể gửi hướng dẫn dán link", "err", err)
		}
		return
	}

	if strings.HasPrefix(text, "/") {
		if cmd, ok := ParseCommand(text, cfg.BotUsername); ok {
			HandleCommand(ctx, client, msg, cmd, appDB, cfg, dealParser)
			return
		}
	}

	if _, ok := dealParser.ExtractFirstURL(text); ok {
		user, _, err := db.GetOrCreateUser(appDB.DB, userID, username, fullName, nil)
		if err != nil {
			slog.Error("Lỗi lấy hoặc tạo user", "err", err)
			return
		}

		adminID := resolveAdminIDFromCache(appDB, cfg)
		isAdmin := adminID != 0 && adminID == userID
		if isAdmin && ShouldProbeAdminSocialInput(text) {
			httpProbe := &http.Client{Timeout: 8 * time.Second}
			if socialParams := social.ResolveSocialParamsFromInput(httpProbe, text); socialParams != nil && shopee.SocialParamStrength(*socialParams) >= 3 {
				if err := db.SetSetting(appDB.DB, "shopee_social_params", *socialParams); err != nil {
					slog.Warn("Không thể lưu shopee_social_params", "err", err)
				} else {
					appDB.InvalidateSettingsCache()
					_, _ = client.SendMessage(
						ctx,
						msg.Chat.ID,
						fmt.Sprintf(
							"⚡ <b>[QUẢN TRỊ VIÊN]</b> Đã nạp chữ ký Shopee FB/Insta Reels:\n<code>%s</code>\n\n✅ Mọi link Shopee từ giờ sẽ gắn chữ ký này!",
							utils.EscapeHTML(*socialParams),
						),
						SendMessageOpts{ParseMode: "HTML"},
					)
				}
			}
		}

		ownedAff := LoadOwnedAffSettingsCached(appDB, cfg)
		isWrapped := ownedAff.ShortlinkBase != ""
		approvedDynamic := dealParser.AccessTrade().GetOrLoadApprovedCampaigns(ownedAff.AccessTradeToken)
		extractedURLs := dealParser.ExtractAllURLsWithApproved(text, 5, approvedDynamic)

		if len(extractedURLs) > 0 {
			if len(extractedURLs) >= 2 {
				handleBatchURLs(ctx, client, msg, appDB, dealParser, text, extractedURLs, user.TelegramID, ownedAff, isWrapped, approvedDynamic)
				return
			}

			handleSingleURL(ctx, client, msg, appDB, dealParser, text, user.TelegramID, isAdmin, ownedAff, isWrapped)
			return
		}
	}

	reply := fmt.Sprintf(
		"👋 Chào <b>%s</b>!\n\n"+
			"🤖 <b>Bot Tự Động Săn Mã Giảm Giá Đa Sàn (Shopee, Lazada, Tiki, TikTok Shop, Alibaba)</b>\n\n"+
			"📌 <b>Cách lấy mã giảm giá cực đơn giản:</b>\n"+
			"1️⃣ Mở app <b>Shopee, Lazada, Tiki, TikTok Shop hoặc Alibaba</b> và copy link món đồ bạn muốn mua.\n"+
			"2️⃣ Dán trực tiếp đường link vào khung chat này.\n"+
			"3️⃣ Bot sẽ tự động kiểm tra ưu đãi và tạo link mua hàng kèm mã giảm giá cho bạn!\n\n"+
			"👇 <i>Bấm các nút chức năng bên dưới hoặc gửi link sản phẩm ngay:</i>",
		utils.EscapeHTML(fullName),
	)
	if _, err := client.SendMessage(ctx, msg.Chat.ID, reply, SendMessageOpts{
		ParseMode:   "HTML",
		ReplyMarkup: MainMenuKeyboard(cfg.BotUsername),
	}); err != nil {
		slog.Warn("Không thể gửi tin nhắn chào", "err", err)
	}
}

func handleBatchURLs(
	ctx context.Context,
	client *Client,
	msg *Message,
	appDB *db.AppDb,
	dealParser *parser.DealParser,
	text string,
	extractedURLs []string,
	telegramID int64,
	ownedAff OwnedAffSettings,
	isWrapped bool,
	approvedDynamic *accesstrade.ApprovedCampaigns,
) {
	waitLabel := fmt.Sprintf("🔍 <i>Đang phân tích và tạo hàng loạt %d link ưu đãi cho bạn...</i>", len(extractedURLs))
	if isWrapped {
		waitLabel = fmt.Sprintf("🔍 <i>Đang phân tích và bọc hàng loạt %d link sản phẩm cho bạn...</i>", len(extractedURLs))
	}
	waitMsg, _ := client.SendMessage(ctx, msg.Chat.ID, waitLabel, SendMessageOpts{ParseMode: "HTML"})

	activeDeals, _ := appDB.GetActiveDealsCached("")
	var summaryBlocks []string
	var btnItems []LabelURL
	rewrittenPost := text
	textWithoutURLs := text
	affSettings := ownedAff.ToParserSettings()

	for idx, rawURL := range extractedURLs {
		textWithoutURLs = strings.ReplaceAll(textWithoutURLs, rawURL, " ")
		parsed, err := dealParser.ProcessLink(rawURL, telegramID, nil, affSettings)
		if err != nil {
			summaryBlocks = append(summaryBlocks, fmt.Sprintf(
				"%d. ⚠️ Không phân tích được link (%s)",
				idx+1,
				utils.EscapeHTML(err.Error()),
			))
			continue
		}

		clickRecord := db.ClickRecord{
			ID:           parsed.ClickID,
			UserID:       telegramID,
			Platform:     parsed.Platform.AsStr(),
			OriginalURL:  parsed.OriginalURL,
			CleanURL:     parsed.CleanURL,
			AffiliateURL: parsed.AffiliateURL,
			SubID:        parsed.SubID,
			CreatedAt:    time.Now().UTC().Format(time.RFC3339),
		}
		if recErr := db.RecordClick(appDB.DB, clickRecord); recErr != nil {
			slog.Warn("Không thể ghi log click batch vào LibSQL", "err", recErr)
		}

		wrappedURL := appDB.WrapURLCached(
			ownedAff.ShortlinkBase,
			parsed.AffiliateURL,
			parsed.ProductTitle,
			parsed.Platform.AsStr(),
		)
		rewrittenPost = strings.ReplaceAll(rewrittenPost, rawURL, wrappedURL)

		priceTag := ""
		if parsed.ProductPriceVND != nil && *parsed.ProductPriceVND > 0 {
			priceTag = " · " + utils.FormatVND(*parsed.ProductPriceVND)
		}

		topDeals := SelectTopRelevantDeals(activeDeals, parsed, 1)
		voucherHint := ""
		if len(topDeals) > 0 {
			d := topDeals[0]
			code := ""
			if d.CouponCode != nil && strings.TrimSpace(*d.CouponCode) != "" {
				code = fmt.Sprintf(" (Mã: <code>%s</code>)", utils.EscapeHTML(*d.CouponCode))
			}
			voucherHint = fmt.Sprintf("\n   👑 %s%s", utils.EscapeHTML(d.Title), code)
		}

		if voucherHint == "" {
			matchedMV := dealParser.AccessTrade().SearchMatchingVouchersForDeal(accesstrade.VoucherMatchQuery{
				Platform:     parsed.Platform.AsStr(),
				ProductTitle: parsed.ProductTitle,
				CleanURL:     parsed.CleanURL,
				ShopName:     parsed.ProductShop,
				BrandName:    parsed.ProductBrand,
				PriceVND:     parsed.ProductPriceVND,
				MaxItems:     1,
			})
			allowMarketGeneric := parsed.Platform.Kind != parser.PlatformShopee ||
				parsed.IsAffiliate ||
				(approvedDynamic != nil && approvedDynamic.Shopee != nil)

			for _, mv := range matchedMV {
				if allowMarketGeneric || mv.IsShopSpecific() {
					voucherHint = fmt.Sprintf(
						"\n   🎟️ <b>%s</b> · Mã: <code>%s</code>",
						utils.EscapeHTML(mv.DiscountBadge()),
						utils.EscapeHTML(mv.CouponCode),
					)
					break
				}
			}
		}

		linkDisplay := FormatTelegramLinkDisplay(wrappedURL, parsed.Platform.AsStr(), isWrapped)
		summaryBlocks = append(summaryBlocks, fmt.Sprintf(
			"%d. <b>[%s] %s</b>%s\n   👉 %s%s",
			idx+1,
			utils.EscapeHTML(parsed.Platform.AsStr()),
			utils.EscapeHTML(parsed.ProductTitle),
			utils.EscapeHTML(priceTag),
			linkDisplay,
			voucherHint,
		))
		btnItems = append(btnItems, LabelURL{
			Label: fmt.Sprintf("🛒 Mở SP #%d (%s)", idx+1, parsed.Platform.AsStr()),
			URL:   wrappedURL,
		})
	}

	batchTitle := fmt.Sprintf("⚡ <b>ĐÃ TẠO HÀNG LOẠT %d LINK ƯU ĐÃI</b>", len(extractedURLs))
	copyTitle := "📋 <b>BÀI ĐÃ GẮN LINK ƯU ĐÃI (Copy đăng kênh/nhóm):</b>"
	if isWrapped {
		batchTitle = fmt.Sprintf("⚡ <b>ĐÃ BỌC HÀNG LOẠT %d LINK SẢN PHẨM</b>", len(extractedURLs))
		copyTitle = "📋 <b>BÀI ĐÃ THAY SẠCH LINK BỌC (Copy đăng kênh/nhóm):</b>"
	}

	extraCopyBlock := ""
	if utf8.RuneCountInString(strings.TrimSpace(textWithoutURLs)) >= 12 {
		previewCopy := utils.TakeRunes(rewrittenPost, 1200)
		extraCopyBlock = fmt.Sprintf("\n\n%s\n<pre>%s</pre>", copyTitle, utils.EscapeHTML(previewCopy))
	}

	replyHTML := fmt.Sprintf("%s\n\n%s%s", batchTitle, strings.Join(summaryBlocks, "\n\n"), extraCopyBlock)
	if utf8.RuneCountInString(replyHTML) > 3800 {
		replyHTML = fmt.Sprintf("%s\n\n%s", batchTitle, strings.Join(summaryBlocks, "\n\n"))
	}

	if waitMsg != nil {
		_ = client.DeleteMessage(ctx, msg.Chat.ID, waitMsg.MessageID)
	}
	if _, err := client.SendMessage(ctx, msg.Chat.ID, replyHTML, SendMessageOpts{
		ParseMode:          "HTML",
		LinkPreviewOptions: disabledLinkPreview(),
		ReplyMarkup:        BatchActionKeyboard(btnItems),
	}); err != nil {
		slog.Warn("Không thể gửi tin nhắn kết quả batch", "err", err)
	}
}

func handleSingleURL(
	ctx context.Context,
	client *Client,
	msg *Message,
	appDB *db.AppDb,
	dealParser *parser.DealParser,
	text string,
	telegramID int64,
	isAdmin bool,
	ownedAff OwnedAffSettings,
	isWrapped bool,
) {
	waitMsg, _ := client.SendMessage(
		ctx,
		msg.Chat.ID,
		"🔍 <i>Đang quét và gắn mã giảm giá tốt nhất cho sản phẩm này...</i>",
		SendMessageOpts{ParseMode: "HTML"},
	)

	parsed, err := dealParser.ProcessLink(text, telegramID, nil, ownedAff.ToParserSettings())
	if err != nil {
		if waitMsg != nil {
			_ = client.DeleteMessage(ctx, msg.Chat.ID, waitMsg.MessageID)
		}
		errText := fmt.Sprintf(
			"⚠️ <b>Không thể phân tích link này</b>\n\n"+
				"Chi tiết: %s\n\n"+
				"👉 Vui lòng dán đường link sản phẩm từ <b>Shopee</b>, <b>Lazada</b>, <b>Tiki</b>, <b>TikTok Shop</b>, <b>Alibaba</b> hoặc các sàn đã duyệt trên ACCESSTRADE.",
			utils.EscapeHTML(err.Error()),
		)
		_, _ = client.SendMessage(ctx, msg.Chat.ID, errText, SendMessageOpts{ParseMode: "HTML"})
		return
	}

	if parsed.AffiliateNote != nil {
		slog.Warn("Ghi chú Affiliate", "platform", parsed.Platform.AsStr(), "note", *parsed.AffiliateNote)
	}

	clickRecord := db.ClickRecord{
		ID:           parsed.ClickID,
		UserID:       telegramID,
		Platform:     parsed.Platform.AsStr(),
		OriginalURL:  parsed.OriginalURL,
		CleanURL:     parsed.CleanURL,
		AffiliateURL: parsed.AffiliateURL,
		SubID:        parsed.SubID,
		CreatedAt:    time.Now().UTC().Format(time.RFC3339),
	}
	if recErr := db.RecordClick(appDB.DB, clickRecord); recErr != nil {
		slog.Warn("Không thể ghi log click vào LibSQL", "err", recErr)
	}

	wrappedAffURL := appDB.WrapURLCached(
		ownedAff.ShortlinkBase,
		parsed.AffiliateURL,
		parsed.ProductTitle,
		parsed.Platform.AsStr(),
	)
	safeAffURL := FormatTelegramLinkDisplay(wrappedAffURL, parsed.Platform.AsStr(), isWrapped)
	socialStrength := shopee.SocialParamStrength(ownedAff.ShopeeSocialParams)

	displayTitle := parsed.ProductTitle
	if displayTitle == "Sản Phẩm Shopee" {
		displayTitle = "Sản Phẩm Shopee Đang Chọn"
	}

	var headerLine string
	if parsed.IsAffiliate {
		var label string
		if strings.EqualFold(parsed.Platform.AsStr(), "Shopee") && socialStrength >= 3 {
			label = "🎯 Link FB / IG Reels · Sẵn sàng áp mã 20-50%"
		} else if strings.EqualFold(parsed.Platform.AsStr(), "Shopee") {
			label = "🎯 Link Ưu Đãi Shopee Đã Kích Hoạt"
		} else {
			label = fmt.Sprintf("🎯 Link Ưu Đãi %s Đã Kích Hoạt", utils.EscapeHTML(parsed.Platform.AsStr()))
		}
		headerLine = fmt.Sprintf("<b>%s</b>\n👉 %s", label, safeAffURL)
	} else {
		adminHint := ""
		if isAdmin && strings.EqualFold(parsed.Platform.AsStr(), "Shopee") {
			adminHint = "\n⚙️ <i>Admin: Chưa gắn Shopee Affiliate ID</i>"
		}
		headerLine = fmt.Sprintf(
			"👉 <b>Link Mở App %s:</b>\n%s%s",
			utils.EscapeHTML(parsed.Platform.AsStr()),
			safeAffURL,
			adminHint,
		)
	}

	var metaParts []string
	if parsed.ProductShop != nil {
		metaParts = append(metaParts, "🏪 "+utils.EscapeHTML(*parsed.ProductShop))
	}
	if parsed.ProductBrand != nil {
		metaParts = append(metaParts, "🏷️ "+utils.EscapeHTML(*parsed.ProductBrand))
	}
	if parsed.ProductPriceVND != nil && *parsed.ProductPriceVND > 0 {
		metaParts = append(metaParts, fmt.Sprintf("💰 <b>%s</b>", utils.EscapeHTML(utils.FormatVND(*parsed.ProductPriceVND))))
	}
	metaLine := ""
	if len(metaParts) > 0 {
		metaLine = strings.Join(metaParts, " · ") + "\n"
	}

	var voucherButtons []LabelURL
	var extraVouchersBlock strings.Builder

	if deals, dErr := appDB.GetActiveDealsCached(""); dErr == nil {
		exList := SelectTopRelevantDeals(deals, parsed, 2)
		if len(exList) > 0 {
			extraVouchersBlock.WriteString("\n\n🎁 <b>Mã Nổi Bật Áp Dụng:</b>")
			for _, d := range exList {
				enrichedDealURL := EnrichDealURLForPlatform(
					d.DealURL,
					d.Platform,
					ownedAff.ShopeeAffiliateID,
					ownedAff.ShopeeSocialParams,
					parsed.SubID,
				)
				wrappedDealURL := appDB.WrapURLCached(
					ownedAff.ShortlinkBase,
					enrichedDealURL,
					d.Title,
					d.Platform,
				)
				shortDesc := utils.TakeRunes(d.DiscountDesc, 70)
				extraVouchersBlock.WriteString("\n• " + utils.EscapeHTML(shortDesc))

				if len(voucherButtons) < 2 && strings.TrimSpace(wrappedDealURL) != "" {
					var btnLabel string
					if d.CouponCode != nil && strings.TrimSpace(*d.CouponCode) != "" {
						code := *d.CouponCode
						if strings.HasPrefix(code, "KOL_IG") {
							btnLabel = "📸 Mã IG 50%"
						} else if strings.HasPrefix(code, "KOL_FB") {
							btnLabel = "📘 Mã FB 30%"
						} else {
							btnLabel = "🎟️ " + code
						}
					} else {
						btnLabel = "🎟️ " + utils.TakeRunes(d.Title, 18)
					}
					voucherButtons = append(voucherButtons, LabelURL{
						Label: btnLabel,
						URL:   wrappedDealURL,
					})
				}
			}
		}
	}

	approvedCached := dealParser.AccessTrade().PeekApprovedCampaigns()
	allowMarketGeneric := parsed.Platform.Kind != parser.PlatformShopee ||
		parsed.IsAffiliate ||
		(approvedCached != nil && approvedCached.Shopee != nil)

	rawMatched := dealParser.AccessTrade().SearchMatchingVouchersForDeal(accesstrade.VoucherMatchQuery{
		Platform:     parsed.Platform.AsStr(),
		ProductTitle: parsed.ProductTitle,
		CleanURL:     parsed.CleanURL,
		ShopName:     parsed.ProductShop,
		BrandName:    parsed.ProductBrand,
		PriceVND:     parsed.ProductPriceVND,
		MaxItems:     4,
	})
	var matchedMarket []accesstrade.VoucherOffer
	for _, mv := range rawMatched {
		if allowMarketGeneric || mv.IsShopSpecific() {
			matchedMarket = append(matchedMarket, mv)
			if len(matchedMarket) >= 2 {
				break
			}
		}
	}

	if len(matchedMarket) > 0 {
		extraVouchersBlock.WriteString("\n\n🎟️ <b>Mã Khớp Đúng Shop/Hãng:</b>")
		for _, mv := range matchedMarket {
			directVoucherURL := dealParser.AccessTrade().VerifiedOfferURL(mv, approvedCached)
			enrichedVoucherURL := EnrichDealURLForPlatform(
				directVoucherURL,
				mv.Platform,
				ownedAff.ShopeeAffiliateID,
				ownedAff.ShopeeSocialParams,
				parsed.SubID,
			)
			wrappedVoucherURL := appDB.WrapURLCached(
				ownedAff.ShortlinkBase,
				enrichedVoucherURL,
				fmt.Sprintf("%s - %s", mv.ShopName, mv.DiscountDesc),
				mv.Platform,
			)
			extraVouchersBlock.WriteString(fmt.Sprintf(
				"\n• <b>[%s]</b> <code>%s</code> — %s",
				utils.EscapeHTML(mv.DiscountBadge()),
				utils.EscapeHTML(mv.CouponCode),
				utils.EscapeHTML(mv.DiscountDesc),
			))
			if len(voucherButtons) < 2 {
				voucherButtons = append(voucherButtons, LabelURL{
					Label: fmt.Sprintf("🎟️ %s · %s", mv.DiscountBadge(), mv.CouponCode),
					URL:   wrappedVoucherURL,
				})
			}
		}
	}

	var rawPortals []LabelURL
	switch strings.ToLower(parsed.Platform.AsStr()) {
	case "shopee":
		rawPortals = []LabelURL{
			{Label: "🎟️ Kho Mã Shopee", URL: "https://shopee.vn/m/ma-giam-gia"},
			{Label: "🎬 Săn Mã Video", URL: "https://shopee.vn/m/shopee-video"},
		}
	case "lazada":
		rawPortals = []LabelURL{{Label: "🎟️ Voucher Lazada", URL: "https://www.lazada.vn/voucher"}}
	case "tiki":
		rawPortals = []LabelURL{{Label: "🎟️ Cổng Mã Tiki", URL: "https://tiki.vn/khuyen-mai/ma-giam-gia"}}
	case "alibaba":
		rawPortals = []LabelURL{{Label: "🌐 Alibaba.com", URL: "https://www.alibaba.com"}}
	}

	shopeeDirectAff := strings.TrimSpace(ownedAff.ShopeeAffiliateID)
	hasShopeeDirectAff := shopee.IsValidShopeeAffiliateID(shopeeDirectAff)

	batchPortalInputs := make([]db.WrapLinkItem, 0, len(rawPortals))
	portalLabels := make([]string, 0, len(rawPortals))
	for _, p := range rawPortals {
		portalTarget := p.URL
		if strings.EqualFold(parsed.Platform.AsStr(), "Shopee") {
			basePortal := p.URL
			if hasShopeeDirectAff {
				basePortal = shopee.BuildDirectAnRedirLink(p.URL, shopeeDirectAff, parsed.SubID)
			}
			portalTarget = shopee.AppendSocialParamsToShopeeLink(basePortal, ownedAff.ShopeeSocialParams)
		}
		portalLabels = append(portalLabels, p.Label)
		batchPortalInputs = append(batchPortalInputs, db.WrapLinkItem{
			TargetURL: portalTarget,
			Title:     p.Label,
			Platform:  parsed.Platform.AsStr(),
		})
	}
	wrappedPortals := appDB.WrapURLsBatchCached(ownedAff.ShortlinkBase, batchPortalInputs)
	portalButtons := make([]LabelURL, 0, len(wrappedPortals))
	for i, w := range wrappedPortals {
		portalButtons = append(portalButtons, LabelURL{
			Label: portalLabels[i],
			URL:   w,
		})
	}

	backLine := ""
	if parsed.Platform.AsStr() != "TikTokShop" {
		backLine = fmt.Sprintf("\n\n⏰ <b>Back mã:</b> %s", utils.EscapeHTML(parsed.PrimaryVoucher.BackHours))
	}
	tipLine := "Xoá sản phẩm cũ khỏi giỏ hàng trước khi bấm mua để nhận đúng mã."
	if len(parsed.Tips) > 0 {
		tipLine = utils.EscapeHTML(parsed.Tips[0])
	}

	replyHTML := fmt.Sprintf(
		"🛍️ <b>%s</b>\n%s%s%s%s\n💡 <i>%s</i>",
		utils.EscapeHTML(displayTitle),
		metaLine,
		headerLine,
		extraVouchersBlock.String(),
		backLine,
		tipLine,
	)

	if waitMsg != nil {
		_ = client.DeleteMessage(ctx, msg.Chat.ID, waitMsg.MessageID)
	}
	keyboard := DealActionKeyboard(
		wrappedAffURL,
		parsed.Platform.AsStr(),
		voucherButtons,
		portalButtons,
	)

	sentPhoto := false
	if parsed.ProductImageURL != nil && utf8.RuneCountInString(replyHTML) <= 960 {
		if _, pErr := client.SendPhoto(ctx, msg.Chat.ID, *parsed.ProductImageURL, replyHTML, SendMessageOpts{
			ParseMode:   "HTML",
			ReplyMarkup: keyboard,
		}); pErr == nil {
			sentPhoto = true
		}
	}
	if !sentPhoto {
		if _, sErr := client.SendMessage(ctx, msg.Chat.ID, replyHTML, SendMessageOpts{
			ParseMode:          "HTML",
			LinkPreviewOptions: disabledLinkPreview(),
			ReplyMarkup:        keyboard,
		}); sErr != nil {
			slog.Warn("Không thể gửi tin nhắn sản phẩm", "err", sErr)
		}
	}
}

// HandleCommand xử lý các lệnh /start, /help, /share, /stats.
func HandleCommand(
	ctx context.Context,
	client *Client,
	msg *Message,
	cmd Command,
	appDB *db.AppDb,
	cfg config.Config,
	_ *parser.DealParser,
) {
	if msg == nil {
		return
	}
	var userID int64
	var username *string
	fullName := "Bạn"
	if msg.From != nil {
		userID = msg.From.ID
		if msg.From.Username != "" {
			u := msg.From.Username
			username = &u
		}
		fullName = msg.From.FullName()
	}

	if appDB == nil || appDB.DB == nil {
		_, _ = client.SendMessage(ctx, msg.Chat.ID, "❌ Hệ thống cơ sở dữ liệu tạm thời bận, vui lòng thử lại sau.", SendMessageOpts{})
		return
	}

	switch cmd {
	case CmdStart:
		if _, _, err := db.GetOrCreateUser(appDB.DB, userID, username, fullName, nil); err != nil {
			slog.Warn("Không thể lưu user khi /start", "err", err)
		}
		_, _ = client.SendMessage(
			ctx,
			msg.Chat.ID,
			"🔘 <i>Menu săn sale nhanh đã được ghim cố định ở đáy khung chat!</i>",
			SendMessageOpts{
				ParseMode:   "HTML",
				ReplyMarkup: PersistentReplyKeyboard(),
			},
		)
		welcome := "⚡ <b>CHÀO MỪNG BẠN ĐẾN VỚI THẾ GIỚI SIÊU SALE</b> ⚡\n\n" +
			"🔥 <b>Mua hàng tiết kiệm hơn chỉ với 1 bước:</b>\n" +
			"Chỉ cần <b>Dán trực tiếp link sản phẩm Shopee, Lazada, Tiki, TikTok Shop, Alibaba hoặc các đối tác chính hãng</b> vào khung chat này, bot sẽ tự phân tích sản phẩm/shop và trả về link mua kèm mã khớp riêng ngay cho bạn!\n\n" +
			"👇 <i>Hãy dán thử 1 link sản phẩm bất kỳ vào đây để trải nghiệm ngay:</i>"
		_, _ = client.SendMessage(ctx, msg.Chat.ID, welcome, SendMessageOpts{
			ParseMode:   "HTML",
			ReplyMarkup: MainMenuKeyboard(cfg.BotUsername),
		})

	case CmdHelp:
		helpText := "📖 <b>HƯỚNG DẪN 3 BƯỚC NHẬN LINK &amp; MÃ GIẢM GIÁ:</b>\n\n" +
			"1️⃣ <b>Bước 1:</b> Vào App Shopee, Lazada, Tiki hoặc TikTok Shop chọn sản phẩm bạn muốn mua.\n" +
			"2️⃣ <b>Bước 2:</b> Bấm nút <b>Chia sẻ</b> -&gt; chọn <b>Sao chép liên kết</b>.\n" +
			"3️⃣ <b>Bước 3:</b> Dán đường link vào khung chat bot này. Bot sẽ trả về link mua sản phẩm kèm mã khớp trực tiếp với shop/sản phẩm đó (nếu có) và nút Cổng Voucher sàn.\n\n" +
			"⚠️ <b>MẸO ÁP MÃ THÀNH CÔNG 100%:</b>\n" +
			"• Bấm vào link bot gửi để mở App sàn.\n" +
			"• Xoá sản phẩm cũ trong giỏ hàng trước khi mua để mã không bị xung đột.\n" +
			"• Canh đúng các khung giờ back mã: <code>0H · 9H · 12H · 15H · 20H</code> để áp mã giảm sâu nhất!"
		_, _ = client.SendMessage(ctx, msg.Chat.ID, helpText, SendMessageOpts{ParseMode: "HTML"})

	case CmdShare:
		shareLink := fmt.Sprintf("https://t.me/%s", cfg.BotUsername)
		shareHTML := fmt.Sprintf(
			"⚡ <b>CHIA SẺ THẾ GIỚI SIÊU SALE CHO BẠN BÈ CÙNG SĂN SALE</b> ⚡\n\n"+
				"Giúp bạn bè mua sắm tiết kiệm mỗi ngày trên Shopee, Lazada, Tiki &amp; TikTok Shop!\n\n"+
				"🔗 <b>Link bot:</b>\n"+
				"<code>%s</code>\n\n"+
				"👉 <i>Hãy copy link trên và chia sẻ lên Facebook, Zalo hoặc nhóm săn sale nhé!</i>",
			utils.EscapeHTML(shareLink),
		)
		_, _ = client.SendMessage(ctx, msg.Chat.ID, shareHTML, SendMessageOpts{
			ParseMode:   "HTML",
			ReplyMarkup: MainMenuKeyboard(cfg.BotUsername),
		})

	case CmdStats:
		adminID := resolveAdminIDFromCache(appDB, cfg)
		canViewAdminStats := adminID == 0 || adminID == userID

		var statsHTML string
		if canViewAdminStats {
			stats, _ := db.GetAdminDashboardStats(appDB.DB)
			trackedOrders, _ := db.GetRecentTrackedOrders(appDB.DB, 100)
			adminURL := fmt.Sprintf("http://127.0.0.1:%d/admin", cfg.Port)
			if cfg.PublicBaseURL != "" {
				adminURL = strings.TrimRight(cfg.PublicBaseURL, "/") + "/admin"
			}
			statsHTML = fmt.Sprintf(
				"📊 <b>BÁO CÁO HOẠT ĐỘNG BOT (DÀNH CHO CHỦ BOT)</b>\n\n"+
					"🆔 <b>Telegram ID của bạn:</b> <code>%d</code> <i>(Dùng để nhận báo đơn trên %s)</i>\n"+
					"👥 <b>Tổng số người dùng bot:</b> <b>%d người</b>\n"+
					"🔗 <b>Tổng số lượt dán link &amp; tạo mã:</b> <b>%d lượt</b>\n"+
					"🧾 <b>Số đơn hàng đã đối soát gần đây:</b> <b>%d đơn</b>\n\n"+
					"💰 <b>Hoa hồng tiếp thị:</b>\n"+
					"100%% doanh thu chiết khấu từ các lượt click trên sẽ được sàn TMĐT đối soát về tài khoản Affiliate của bạn!",
				userID, utils.EscapeHTML(adminURL), stats.TotalUsers, stats.TotalClicks, len(trackedOrders),
			)
		} else {
			statsHTML = fmt.Sprintf(
				"🆔 <b>Telegram ID của bạn:</b> <code>%d</code>\n\n"+
					"👉 Hãy dán trực tiếp link sản phẩm Shopee, Lazada, Tiki hoặc TikTok Shop vào đây để săn mã giảm giá nhé!",
				userID,
			)
		}
		_, _ = client.SendMessage(ctx, msg.Chat.ID, statsHTML, SendMessageOpts{ParseMode: "HTML"})
	}
}

// HandleCallback xử lý các sự kiện bấm nút Inline Keyboard.
func HandleCallback(
	ctx context.Context,
	client *Client,
	q *CallbackQuery,
	cfg config.Config,
) {
	if q == nil || q.Data == "" || q.Message == nil {
		return
	}
	chatID := q.Message.Chat.ID
	_ = client.AnswerCallbackQuery(ctx, q.ID)

	if strings.HasPrefix(q.Data, "deals_page:") || strings.HasPrefix(q.Data, "deals_refresh:") {
		_, _ = client.SendMessage(
			ctx,
			chatID,
			"👉 Hãy <b>dán trực tiếp link sản phẩm</b> vào chat để nhận link kèm mã khớp riêng cho sản phẩm đó!",
			SendMessageOpts{ParseMode: "HTML"},
		)
		return
	}

	switch q.Data {
	case "menu_guide":
		guide := "📖 <b>HƯỚNG DẪN 3 BƯỚC NHẬN MÃ GIẢM GIÁ:</b>\n\n" +
			"1️⃣ Copy link sản phẩm trên app Shopee, Lazada, Tiki hoặc TikTok Shop.\n" +
			"2️⃣ Dán link vào khung chat này để bot tạo link mua kèm mã khớp riêng (nếu có).\n" +
			"3️⃣ Bấm nút mở sản phẩm hoặc nút lấy mã đi kèm để đặt hàng!\n\n" +
			"<i>Mẹo: Xóa sản phẩm cũ trong giỏ hàng trước khi mua để tránh bị lỗi mã!</i>"
		_, _ = client.SendMessage(ctx, chatID, guide, SendMessageOpts{ParseMode: "HTML"})
	case "menu_main":
		_, _ = client.SendMessage(ctx, chatID, "🏠 <b>MENU CHÍNH THẾ GIỚI SIÊU SALE</b>", SendMessageOpts{
			ParseMode:   "HTML",
			ReplyMarkup: MainMenuKeyboard(cfg.BotUsername),
		})
	}
}
