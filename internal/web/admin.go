package web

import (
	"fmt"
	"log/slog"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"salehunter-go/internal/config"
	"salehunter-go/internal/db"
	"salehunter-go/internal/services/accesstrade"
	"salehunter-go/internal/services/orders"
	"salehunter-go/internal/services/shopee"
	"salehunter-go/internal/services/social"
	"salehunter-go/internal/utils"
)

func resolveAccessTradeToken(appDB *db.AppDb, cfg config.Config) string {
	if appDB != nil && appDB.DB != nil {
		if val, err := db.GetSetting(appDB.DB, "accesstrade_api_token"); err == nil && val != nil && strings.TrimSpace(*val) != "" {
			return *val
		}
	}
	return cfg.AccessTradeAPIToken
}

func formatTimestampVN(raw string) string {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return "-"
	}
	if t, err := time.Parse(time.RFC3339, trimmed); err == nil {
		vn := t.UTC().Add(7 * time.Hour)
		return vn.Format("15:04:05 · 02/01/2006")
	}
	if t, err := time.Parse(time.RFC3339Nano, trimmed); err == nil {
		vn := t.UTC().Add(7 * time.Hour)
		return vn.Format("15:04:05 · 02/01/2006")
	}
	noFrac := strings.Split(trimmed, ".")[0]
	if t, err := time.Parse("2006-01-02T15:04:05", noFrac); err == nil {
		vn := t.Add(7 * time.Hour)
		return vn.Format("15:04:05 · 02/01/2006")
	}
	return trimmed
}

func isValidHTTPURL(u string) bool {
	return utils.IsSafeHTTPURL(u)
}

func extractTunnelToken(raw string) string {
	for _, tok := range strings.Fields(raw) {
		cand := strings.Trim(tok, `"'`)
		if strings.HasPrefix(cand, "eyJ") && len(cand) >= 40 {
			return cand
		}
	}
	return ""
}

func redirectWithNotice(w http.ResponseWriter, r *http.Request, msg string) {
	http.Redirect(w, r, "/admin?notice="+url.QueryEscape(msg), http.StatusSeeOther)
}

// HandleDashboard hiển thị trang quản trị Admin (`/admin`).
func HandleDashboard(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		if state.DB == nil || state.DB.DB == nil {
			http.Error(w, "Database unavailable", http.StatusServiceUnavailable)
			return
		}

		stats, _ := db.GetAdminDashboardStats(state.DB.DB)
		clicks, _ := db.GetRecentClicksForAdmin(state.DB.DB, 50)
		savedDeals, _ := db.GetAllDealsForAdmin(state.DB.DB)

		settingsMap, _ := state.DB.GetSettingsMapCached()
		getSetting := func(k string) string {
			return strings.TrimSpace(settingsMap[k])
		}
		firstNonEmpty := func(a, b string) string {
			if strings.TrimSpace(a) != "" {
				return strings.TrimSpace(a)
			}
			return strings.TrimSpace(b)
		}

		affiliateTemplate := settingsMap["affiliate_template"]
		dbATToken := firstNonEmpty(settingsMap["accesstrade_api_token"], state.Config.AccessTradeAPIToken)
		hasTurso := state.Config.TursoURL != "" && state.Config.TursoAuthToken != ""
		hasEnvToken := state.Config.AccessTradeAPIToken != ""
		isCloudRuntime := state.Config.PublicBaseURL != ""

		csrfToken := strings.TrimSpace(ExtractCSRFToken(r))
		if csrfToken == "" {
			csrfToken = generateSecureHexToken(16)
		}

		authActionHTML := `<form method="POST" action="/admin/logout" style="display:inline;margin:0;"><button type="submit" class="btn btn--danger btn--sm">Đăng Xuất</button></form>`
		if IsTrustedLocalAdminHeader(state, r) {
			authActionHTML = `<span class="badge badge--ok">🖥️ PC Local (Miễn đăng nhập)</span>`
		}

		var persistenceBanner string
		switch {
		case !isCloudRuntime && !hasTurso:
			persistenceBanner = fmt.Sprintf(`<div class="notice-banner notice-banner--ok">
                <span>💻</span>
                <span><b>Đang chạy trực tiếp trên PC (Local):</b> Dữ liệu được lưu bền vững trên máy tính tại <code>%s</code> · Bot chạy chế độ <b>Long Polling</b> và gọi API trực tiếp từ IP của bạn.</span>
            </div>`, utils.EscapeHTML(state.Config.DatabasePath))
		case !hasTurso:
			persistenceBanner = `<div class="notice-banner notice-banner--warn">
            <span>⚠️</span>
            <span><b>Chưa gắn Turso Cloud Database:</b> Khi chạy Hosting/Cloud bắt buộc phải thêm <code>TURSO_DATABASE_URL</code> &amp; <code>TURSO_AUTH_TOKEN</code> vào Environment (hệ thống không fallback về file SQLite tạm).</span>
        </div>`
		case !hasEnvToken:
			persistenceBanner = `<div class="notice-banner notice-banner--warn">
            <span>⚠️</span>
            <span><b>Đã kết nối Turso Cloud:</b> Nên thêm cố định <code>ACCESSTRADE_API_TOKEN</code> vào Environment của Hosting để không phải nhập lại sau mỗi lần deploy.</span>
        </div>`
		default:
			persistenceBanner = `<div class="notice-banner notice-banner--ok">
            <span>🟢</span>
            <span><b>Hệ thống lưu trữ bền vững:</b> Đã kết nối Turso Cloud Database &amp; cố định API Token an toàn.</span>
        </div>`
		}

		defaultPwdWarning := ""
		if isCloudRuntime && state.Config.AdminPassword == "DealX@Admin2026" {
			defaultPwdWarning = `<div class="notice-banner notice-banner--warn"><span>🚨</span><span><b>CẢNH BÁO BẢO MẬT CLOUD:</b> Hệ thống đang chạy Cloud với mật khẩu mặc định! Hãy đặt biến môi trường <code>ADMIN_PASSWORD</code> mạnh ngay để bảo vệ trang quản trị.</span></div>`
		}

		noticeQ := strings.TrimSpace(r.URL.Query().Get("notice"))
		noticeBanner := defaultPwdWarning
		if noticeQ != "" {
			noticeBanner += fmt.Sprintf(`<div class="notice-banner notice-banner--info"><span>ℹ️</span><span>%s</span></div>`, utils.EscapeHTML(noticeQ))
		}

		campShopee := settingsMap["campaign_shopee"]
		campLazada := settingsMap["campaign_lazada"]
		campTiki := settingsMap["campaign_tiki"]
		campTikTok := settingsMap["campaign_tiktok"]

		shopeeAffID := firstNonEmpty(settingsMap["shopee_affiliate_id"], state.Config.ShopeeAffiliateID)
		shopeeAppID := firstNonEmpty(settingsMap["shopee_app_id"], state.Config.ShopeeAppID)
		shopeeAppSecret := firstNonEmpty(settingsMap["shopee_app_secret"], state.Config.ShopeeAppSecret)
		shopeeSocialParams := firstNonEmpty(settingsMap["shopee_social_params"], state.Config.ShopeeSocialParams)

		socialDealsCount := 0
		for _, d := range savedDeals {
			if d.IsActive && strings.EqualFold(d.Platform, "Shopee") && d.CouponCode != nil {
				c := *d.CouponCode
				if strings.HasPrefix(c, "KOL_") || strings.HasPrefix(c, "SHP_AUTO_") {
					socialDealsCount++
				}
			}
		}

		socialSyncSnap := social.SnapshotSocialStatus()
		socialSigStrength := shopee.SocialParamStrength(shopeeSocialParams)
		var socialHarvesterBadge string
		switch {
		case socialSigStrength >= 3:
			socialHarvesterBadge = fmt.Sprintf(`<span class="badge badge--ok">🟢 Auto FB/IG đầy đủ (sm_sig_id · mức 3/3) · %d mã FB/IG/KOL</span>`, socialDealsCount)
		case socialSigStrength >= 2:
			socialHarvesterBadge = fmt.Sprintf(`<span class="badge badge--ok">🟢 Auto FB/IG cơ bản (mức 2/3) · %d mã FB/IG/KOL</span>`, socialDealsCount)
		case socialDealsCount > 0:
			socialHarvesterBadge = fmt.Sprintf(`<span class="badge badge--info">🎁 Đã tự nhặt %d mã Shopee FB/IG/KOL</span>`, socialDealsCount)
		default:
			socialHarvesterBadge = `<span class="badge badge--info">🔄 Đang tự động quét kênh FB/IG...</span>`
		}

		socialLastDisplay := "Đang chạy ngầm..."
		if socialSyncSnap.LastSyncVN != "" {
			socialLastDisplay = fmt.Sprintf("%s (Chu kỳ #%d)", socialSyncSnap.LastSyncVN, socialSyncSnap.SyncCycles)
		} else if socialSyncSnap.IsSyncing {
			socialLastDisplay = "Đang quét ngay khi khởi động..."
		}

		signalMeterHTML := renderSignalMeterSVG(socialSigStrength)
		socialAutoPanelHTML := fmt.Sprintf(`<div class="social-meta">
            <div style="display:flex;align-items:center;gap:0.5rem;flex-wrap:wrap;justify-content:flex-end;">
                %s
                <span class="badge badge--ok"><svg class="ico ico--xs"><use href="#ico-bot"/></svg> Tự Động Mỗi 5 Phút</span>
            </div>
            <span class="muted-text">Lần cuối: <strong class="meta-time">%s</strong></span>
        </div>`, signalMeterHTML, utils.EscapeHTML(socialLastDisplay))

		shortlinkDBRaw := settingsMap["shortlink_base_url"]
		hasCFTunnelToken := getSetting("cloudflare_tunnel_token") != "" || strings.TrimSpace(os.Getenv("CLOUDFLARE_TUNNEL_TOKEN")) != ""
		shortlinkBaseURL := config.ResolvePCRuntimeShortlinkBaseURL(
			shortlinkDBRaw,
			state.Config.ShortlinkBaseURL,
			"",
			"",
			hasCFTunnelToken,
		)
		shortlinkMobileOnly := getSetting("shortlink_mobile_only") == "1"
		publicHomeCloak := getSetting("public_home_cloak") == "1"

		originBaseForWorker := "https://origin.thegioisieusale.click"
		if sh := ExtractHostOnly(shortlinkBaseURL); sh != "" {
			originBaseForWorker = "https://origin." + sh
		}
		if strings.TrimSpace(state.Config.PublicBaseURL) != "" {
			originBaseForWorker = strings.TrimSpace(state.Config.PublicBaseURL)
		}

		adminTelegramIDVal := getSetting("admin_telegram_id")
		if adminTelegramIDVal == "" && state.Config.AdminTelegramID != 0 {
			adminTelegramIDVal = strconv.FormatInt(state.Config.AdminTelegramID, 10)
		}
		trackedOrders, _ := db.GetRecentTrackedOrders(state.DB.DB, 20)

		shopeeCardHTML := renderShopeeCard(shopeeCardView{
			AffID:               shopeeAffID,
			AppID:               shopeeAppID,
			AppSecret:           shopeeAppSecret,
			SocialParams:        shopeeSocialParams,
			ShortlinkBaseURL:    shortlinkBaseURL,
			ShortlinkMobileOnly: shortlinkMobileOnly,
			PublicHomeCloak:     publicHomeCloak,
			OriginBaseURL:       originBaseForWorker,
			BotUsername:         state.Config.BotUsername,
			HasCFTunnelToken:    hasCFTunnelToken,
		})
		orderAlertsCardHTML := renderOrderAlertsCard(adminTelegramIDVal, trackedOrders)

		var approvedOpt *accesstrade.ApprovedCampaigns
		var atStatusBadge, campaignStatusHTML string
		if dbATToken != "" {
			masked := utils.MaskToken(dbATToken)
			var approvedRes *accesstrade.ApprovedCampaigns
			var approvedErr error

			if cached := state.AccessTrade.PeekApprovedCampaigns(); cached != nil {
				approvedRes = cached
			} else if state.CampFetching.CompareAndSwap(false, true) {
				type fetchRes struct {
					app *accesstrade.ApprovedCampaigns
					err error
				}
				ch := make(chan fetchRes, 1)
				utils.SafeGo(nil, "admin.fetch_campaigns", func() {
					defer state.CampFetching.Store(false)
					a, e := state.AccessTrade.GetApprovedCampaigns(dbATToken, false)
					ch <- fetchRes{app: a, err: e}
				})
				select {
				case r := <-ch:
					approvedRes = r.app
					approvedErr = r.err
				case <-time.After(1200 * time.Millisecond):
					approvedErr = fmt.Errorf("Đang tải danh sách chiến dịch ACCESSTRADE ở nền...")
				}
			} else {
				approvedErr = fmt.Errorf("Đang tải danh sách chiến dịch ACCESSTRADE ở nền...")
			}

			if approvedErr == nil && approvedRes != nil {
				atStatusBadge = fmt.Sprintf(`<span class="badge badge--ok">🟢 Đã kết nối (%s)</span>`, masked)
				dynList := approvedRes.DynamicCampaigns()
				dynLine := `<div class="inset-note">✨ <b>Tự động nhận diện đa sàn:</b> Sẵn sàng — đăng ký thêm <i>Shein, CellphoneS, FPT Shop, Con Cưng, Fado, Klook...</i> trên <code>pub2.accesstrade.vn</code> sẽ tự kích hoạt không cần sửa code.</div>`
				if len(dynList) > 0 {
					var labels []string
					for i, c := range dynList {
						if i >= 8 {
							break
						}
						labels = append(labels, fmt.Sprintf("%s (<code>%s</code>)", utils.EscapeHTML(c.PlatformGroup), utils.EscapeHTML(c.Domain)))
					}
					dynLine = fmt.Sprintf(`<div class="inset-note inset-note--ok">✨ <b>Đã tự mở thêm %d sàn/hãng mở rộng:</b> %s</div>`, len(dynList), strings.Join(labels, ", "))
				}
				shopeeBackupNote := ""
				if shopeeAffID != "" || state.Config.ShopeeAffiliateID != "" {
					shopeeBackupNote = "Shopee Chính Chủ (100% Hoạt Động)"
				}
				campaignStatusHTML = "<div class=\"plat-list\">" +
					renderPlatformStatus("🟠 Shopee", approvedRes.Shopee, campShopee, shopeeBackupNote) +
					renderPlatformStatus("🔵 Lazada", approvedRes.Lazada, campLazada, "Deeplink Template Sẵn Sàng") +
					renderPlatformStatus("🔹 Tiki", approvedRes.Tiki, campTiki, "") +
					renderPlatformStatus("🎵 TikTok Shop", approvedRes.TikTok, campTikTok, "") +
					renderPlatformStatus("🌐 Alibaba", approvedRes.Alibaba, "", "") +
					"</div>" + dynLine
				approvedOpt = approvedRes
			} else {
				atStatusBadge = fmt.Sprintf(`<span class="badge badge--err">🔴 Lỗi kết nối (%s)</span>`, masked)
				errMsg := ""
				if approvedErr != nil {
					errMsg = approvedErr.Error()
				}
				campaignStatusHTML = fmt.Sprintf(`<p class="err-text">%s</p>`, utils.EscapeHTML(errMsg))
			}
		} else {
			atStatusBadge = `<span class="badge badge--muted">⚪ Chưa nhập API Key</span>`
			campaignStatusHTML = `<p class="muted-text">Vui lòng nhập <b>ACCESSTRADE API Access Key</b> bên dưới hoặc thiết lập <code>ACCESSTRADE_API_TOKEN</code> trong file <code>.env</code>.</p>`
		}

		syncMeta := state.AccessTrade.GetVoucherSyncMeta()
		lastSyncDisplay := "Đang khởi tạo chu kỳ đầu tiên..."
		if syncMeta.LastSyncVN != "" {
			lastSyncDisplay = fmt.Sprintf("%s (Chu kỳ #%d)", syncMeta.LastSyncVN, syncMeta.SyncCycles)
		}

		cachedVouchers := state.AccessTrade.PeekCachedHighDiscountVouchers()
		totalMarketVouchers := len(cachedVouchers)
		highPctVouchers := 0
		shopeeVouchers := 0
		tikiVouchers := 0
		for _, v := range cachedVouchers {
			if v.DiscountPercent >= 20 {
				highPctVouchers++
			}
			if v.Platform == "Shopee" {
				shopeeVouchers++
			}
			if v.Platform == "Tiki" {
				tikiVouchers++
			}
		}

		searchQ := strings.TrimSpace(r.URL.Query().Get("q"))
		filterPlat := strings.TrimSpace(r.URL.Query().Get("plat"))
		if filterPlat == "" {
			filterPlat = "all"
		}
		searchQLower := strings.ToLower(searchQ)

		var filteredVouchers []accesstrade.VoucherOffer
		for _, v := range cachedVouchers {
			platOK := true
			switch filterPlat {
			case "pct20":
				platOK = v.DiscountPercent >= 20
			case "Shopee":
				platOK = v.Platform == "Shopee"
			case "Tiki":
				platOK = v.Platform == "Tiki"
			case "other":
				platOK = v.DiscountPercent < 20
			}
			if !platOK {
				continue
			}
			if searchQLower != "" &&
				!strings.Contains(strings.ToLower(v.ShopName), searchQLower) &&
				!strings.Contains(strings.ToLower(v.CouponCode), searchQLower) &&
				!strings.Contains(strings.ToLower(v.DiscountDesc), searchQLower) &&
				!strings.Contains(strings.ToLower(v.Platform), searchQLower) {
				continue
			}
			filteredVouchers = append(filteredVouchers, v)
		}

		var voucherRowsBuilder strings.Builder
		shownVouchers := 0
		for _, v := range filteredVouchers {
			if shownVouchers >= 50 {
				break
			}
			shownVouchers++
			targetURL := v.BestTargetURL(approvedOpt)
			voucherRowsBuilder.WriteString(fmt.Sprintf(`<tr>
                <td><span class="badge badge--orange">%s</span></td>
                <td class="cell-strong">%s</td>
                <td><code class="code-pill">%s</code></td>
                <td><div class="cell-strong">%s</div><div class="cell-sub">%s</div></td>
                <td class="cell-muted">%s</td>
                <td><a href="%s" target="_blank" rel="noopener noreferrer" class="table-link">Mở Link Chuẩn ↗</a></td>
            </tr>`,
				utils.EscapeHTML(v.DiscountBadge()),
				utils.EscapeHTML(v.Platform),
				utils.EscapeHTML(v.CouponCode),
				utils.EscapeHTML(v.ShopName),
				utils.EscapeHTML(v.DiscountDesc),
				utils.EscapeHTML(v.EndTime),
				utils.SafeHrefURL(targetURL),
			))
		}
		voucherRows := voucherRowsBuilder.String()
		if voucherRows == "" {
			var emptyHint string
			switch {
			case dbATToken == "":
				emptyHint = `Chưa cấu hình ACCESSTRADE API Access Key. Vào tab <b>"Kết Nối Sàn &amp; API"</b> để nhập Token hoặc nạp mã vào mục <b>"Kho Mã Độc Quyền Hợp Tác"</b> bên dưới.`
			case approvedOpt == nil:
				emptyHint = `⚠️ <b>ACCESSTRADE API đang bị lỗi kết nối (HTTP 403 Forbidden / Sai Key):</b> Bot chưa được cấp quyền kéo mã tự động từ sàn về.<br><strong class="warn-hint">👉 Cách khắc phục:</strong> Vào <a href="https://pub2.accesstrade.vn" target="_blank" rel="noopener noreferrer" class="table-link">pub2.accesstrade.vn</a> &gt; <b>Công cụ &gt; API</b> lấy lại Access Key mới rồi dán vào tab <b>"Kết Nối Sàn &amp; API"</b>.`
			default:
				emptyHint = `Không có mã nào khớp bộ lọc (hoặc đang nạp chu kỳ đầu tiên). Bấm nút <b>Quét Sâu Toàn Bộ Ngay</b> nếu cần làm mới ngay.`
			}
			voucherRows = fmt.Sprintf(`<tr><td colspan="6" class="empty-cell">%s</td></tr>`, emptyHint)
		}

		var exclusiveRowsBuilder strings.Builder
		for _, d := range savedDeals {
			kindBadge := `<span class="badge badge--muted">Thường</span>`
			if d.IsExclusive {
				kindBadge = `<span class="badge badge--purple">👑 Độc Quyền</span>`
			}
			statusBadge := `<span class="badge badge--muted">⚫ Đang tắt</span>`
			if d.IsActive {
				statusBadge = `<span class="badge badge--ok">🟢 Đang bật</span>`
			}
			codeCellHTML := `<span class="badge badge--muted">Theo link</span>`
			if d.CouponCode != nil && strings.TrimSpace(*d.CouponCode) != "" {
				c := strings.TrimSpace(*d.CouponCode)
				if social.IsSyntheticTrackingCode(c) {
					codeCellHTML = fmt.Sprintf(`<span class="badge badge--info" title="Ưu đãi áp trực tiếp khi mở link sàn, không phải mã gõ tay">Áp qua link · %s</span>`, utils.EscapeHTML(c))
				} else {
					codeCellHTML = fmt.Sprintf(`<code class="code-pill">%s</code>`, utils.EscapeHTML(c))
				}
			}
			minNote := ""
			if d.MinSpend > 0 {
				minNote = " · Đơn từ " + utils.EscapeHTML(utils.FormatVND(d.MinSpend))
			}
			exclusiveRowsBuilder.WriteString(fmt.Sprintf(`<tr>
                <td>%s</td>
                <td class="cell-strong">%s</td>
                <td><div class="cell-strong">%s</div><div class="cell-sub">%s%s</div></td>
                <td>%s</td>
                <td>%s</td>
                <td>
                    <div class="row-actions">
                        <form method="POST" action="/admin/deals/toggle">
                            <input type="hidden" name="deal_id" value="%d">
                            <button type="submit" class="btn btn--secondary btn--sm">Bật/Tắt</button>
                        </form>
                        <form method="POST" action="/admin/deals/delete">
                            <input type="hidden" name="deal_id" value="%d">
                            <button type="submit" class="btn btn--danger btn--sm">Xoá</button>
                        </form>
                    </div>
                </td>
            </tr>`,
				kindBadge,
				utils.EscapeHTML(d.Platform),
				utils.EscapeHTML(d.Title),
				utils.EscapeHTML(d.DiscountDesc),
				minNote,
				codeCellHTML,
				statusBadge,
				d.ID,
				d.ID,
			))
		}
		exclusiveRows := exclusiveRowsBuilder.String()
		if exclusiveRows == "" {
			exclusiveRows = `<tr><td colspan="6" class="empty-cell">Chưa có Mã Độc Quyền Hợp Tác nào. Hãy dán danh sách mã từ AM ACCESSTRADE / MCN / Nhãn hàng vào ô bên trên.</td></tr>`
		}

		var clicksRowsBuilder strings.Builder
		for _, c := range clicks {
			platBadge := `<span class="badge badge--muted">Khác</span>`
			switch c.Platform {
			case "Shopee":
				platBadge = `<span class="badge badge--orange">Shopee</span>`
			case "TikTok Shop", "TikTok":
				platBadge = `<span class="badge badge--pink">TikTok Shop</span>`
			case "Lazada":
				platBadge = `<span class="badge badge--info">Lazada</span>`
			case "Tiki":
				platBadge = `<span class="badge badge--cyan">Tiki</span>`
			}
			isTracked := c.AffiliateURL != c.CleanURL ||
				strings.Contains(c.AffiliateURL, "go.isclix.com") ||
				strings.Contains(c.AffiliateURL, "s.shopee.vn") ||
				strings.Contains(c.AffiliateURL, "tiktok.com/t/")
			outLinkLabel := "🌐 Link Sạch (Chưa bật Aff) ↗"
			if isTracked {
				outLinkLabel = "🔗 Link Đã Gắn Tracking ↗"
			}
			vnTime := formatTimestampVN(c.CreatedAt)
			clicksRowsBuilder.WriteString(fmt.Sprintf(`<tr>
                <td class="font-mono cell-muted">%d</td>
                <td>%s</td>
                <td><div class="truncate-url" title="%s">%s</div></td>
                <td><a href="%s" target="_blank" rel="noopener noreferrer" class="table-link table-link--amber">%s</a></td>
                <td class="font-mono cell-muted">%s</td>
            </tr>`,
				c.UserID,
				platBadge,
				utils.EscapeHTML(c.CleanURL),
				utils.EscapeHTML(c.CleanURL),
				utils.SafeHrefURL(c.AffiliateURL),
				outLinkLabel,
				utils.EscapeHTML(vnTime),
			))
		}
		clicksRows := clicksRowsBuilder.String()
		if clicksRows == "" {
			clicksRows = `<tr><td colspan="5" class="empty-cell">Chưa có lượt dán link nào từ khách hàng. Dán link sản phẩm vào Bot Telegram @vvipass_vip_bot để kiểm tra.</td></tr>`
		}

		botModeBadge := `<span class="mode-tag mode-tag--info">🔵 Polling PC</span>`
		if state.WebhookCh != nil {
			botModeBadge = `<span class="mode-tag mode-tag--ok">🟢 Webhook 24/7</span>`
		}

		selIf := func(cond bool) string {
			if cond {
				return "selected"
			}
			return ""
		}

		otherVouchersCount := totalMarketVouchers - highPctVouchers
		if otherVouchersCount < 0 {
			otherVouchersCount = 0
		}

		chartsHTML := renderDashboardChartsHTML(
			stats,
			totalMarketVouchers,
			highPctVouchers,
			shopeeVouchers,
			tikiVouchers,
			len(savedDeals),
		)

		doc := renderDashboardDocument(dashboardRenderData{
			BotUsername:          state.Config.BotUsername,
			BotModeBadge:         botModeBadge,
			AdminUsername:        state.Config.AdminUsername,
			AuthActionHTML:       authActionHTML,
			NoticeBanner:         noticeBanner,
			PersistenceBanner:    persistenceBanner,
			Stats:                stats,
			TotalMarketVouchers:  totalMarketVouchers,
			HighPctVouchers:      highPctVouchers,
			OtherVouchersCount:   otherVouchersCount,
			ShopeeVouchers:       shopeeVouchers,
			TikiVouchers:         tikiVouchers,
			SavedDealsCount:      len(savedDeals),
			LastSyncDisplay:      lastSyncDisplay,
			ChartsHTML:           chartsHTML,
			SocialHarvesterBadge: socialHarvesterBadge,
			SocialAutoPanelHTML:  socialAutoPanelHTML,
			ClicksRows:           clicksRows,
			ATStatusBadge:        atStatusBadge,
			CampaignStatusHTML:   campaignStatusHTML,
			CampShopee:           campShopee,
			CampLazada:           campLazada,
			CampTiki:             campTiki,
			CampTikTok:           campTikTok,
			ShopeeCardHTML:       shopeeCardHTML,
			AffiliateTemplate:    affiliateTemplate,
			SearchQ:              searchQ,
			SelAll:               selIf(filterPlat == "all"),
			SelPct20:             selIf(filterPlat == "pct20"),
			SelOther:             selIf(filterPlat == "other"),
			SelShopee:            selIf(filterPlat == "Shopee"),
			SelTiki:              selIf(filterPlat == "Tiki"),
			ShownVouchersCount:   shownVouchers,
			MatchedVouchersCount: len(filteredVouchers),
			VoucherRows:          voucherRows,
			ExclusiveRows:        exclusiveRows,
			OrderAlertsCardHTML:  orderAlertsCardHTML,
			CSRFToken:            csrfToken,
		})

		isHTTPS := IsHTTPSRequest(state, r)
		secureAttr := ""
		if isHTTPS {
			secureAttr = "; Secure"
		}
		csrfCookieName := CSRFCookieName(isHTTPS)
		w.Header().Add("Set-Cookie", fmt.Sprintf("%s=%s; HttpOnly; Path=/; SameSite=Lax%s", csrfCookieName, csrfToken, secureAttr))
		setNoStoreHeaders(w)
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(doc))
	}
}

// HandleUpdateAccessTrade cập nhật token & campaign IDs ACCESSTRADE (`POST /admin/settings/accesstrade`).
func HandleUpdateAccessTrade(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		if err := r.ParseForm(); err != nil {
			http.Error(w, "Bad request", http.StatusBadRequest)
			return
		}
		if state.DB != nil && state.DB.DB != nil {
			cleanTok := accesstrade.CleanAPIToken(r.PostFormValue("api_token"))
			if cleanTok != "" {
				_ = db.SetSetting(state.DB.DB, "accesstrade_api_token", cleanTok)
				slog.Info("✅ Quản trị viên đã cập nhật ACCESSTRADE API Token mới qua Web Admin.")
			}
			_ = db.SetSetting(state.DB.DB, "campaign_shopee", strings.TrimSpace(r.PostFormValue("campaign_shopee")))
			_ = db.SetSetting(state.DB.DB, "campaign_lazada", strings.TrimSpace(r.PostFormValue("campaign_lazada")))
			_ = db.SetSetting(state.DB.DB, "campaign_tiki", strings.TrimSpace(r.PostFormValue("campaign_tiki")))
			_ = db.SetSetting(state.DB.DB, "campaign_tiktok", strings.TrimSpace(r.PostFormValue("campaign_tiktok")))
			state.DB.InvalidateSettingsCache()
		}
		state.AccessTrade.InvalidateCache()
		http.Redirect(w, r, "/admin", http.StatusSeeOther)
	}
}

// HandleSyncAccessTrade đồng bộ lại chiến dịch ACCESSTRADE (`POST /admin/settings/accesstrade/sync`).
func HandleSyncAccessTrade(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		if token := resolveAccessTradeToken(state.DB, state.Config); token != "" {
			_, _ = state.AccessTrade.GetApprovedCampaigns(token, true)
		}
		http.Redirect(w, r, "/admin", http.StatusSeeOther)
	}
}

// HandleUpdateShopeeSettings lưu cấu hình Shopee Chính Chủ, Bọc Link & Cloak (`POST /admin/settings/shopee`).
func HandleUpdateShopeeSettings(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		if err := r.ParseForm(); err != nil {
			redirectWithNotice(w, r, "Dữ liệu form không hợp lệ.")
			return
		}

		affTrimmed := strings.TrimSpace(r.PostFormValue("shopee_affiliate_id"))
		if affTrimmed != "" && !shopee.IsValidShopeeAffiliateID(affTrimmed) {
			redirectWithNotice(w, r, "Shopee Affiliate ID phải là dãy gồm 5 đến 20 chữ số (hoặc để trống để tắt).")
			return
		}

		shortBaseTrimmed := strings.TrimRight(strings.TrimSpace(r.PostFormValue("shortlink_base_url")), "/")
		if shortBaseTrimmed != "" &&
			!config.IsShortlinkDisabledValue(shortBaseTrimmed) &&
			!isValidHTTPURL(shortBaseTrimmed) {
			redirectWithNotice(w, r, "Tên miền bọc link riêng phải bắt đầu bằng https:// hoặc http:// (hoặc nhập off để tắt).")
			return
		}

		socialTrimmed := strings.TrimSpace(r.PostFormValue("shopee_social_params"))
		var validatedSocial *string
		if r.PostFormValue("clear_social_params") == "1" {
			empty := ""
			validatedSocial = &empty
		} else if socialTrimmed != "" {
			httpProbe := utils.NewSafePublicHTTPClient(8*time.Second, nil)
			clean := social.ResolveSocialParamsFromInput(httpProbe, socialTrimmed)
			if clean == nil {
				redirectWithNotice(w, r, "Mẫu phiên IG/FB Reels chưa hợp lệ (phải chứa sm_sig_id, encrypted_payload hoặc link có chữ ký FB/IG).")
				return
			}
			validatedSocial = clean
		}

		tunnelTokTrimmed := strings.TrimSpace(r.PostFormValue("cloudflare_tunnel_token"))
		var validatedTunnelTok *string
		if r.PostFormValue("clear_tunnel_token") == "1" {
			empty := ""
			validatedTunnelTok = &empty
		} else if tunnelTokTrimmed != "" {
			tok := extractTunnelToken(tunnelTokTrimmed)
			if tok == "" {
				redirectWithNotice(w, r, "Cloudflare Tunnel Token chưa hợp lệ (phải chứa chuỗi token bắt đầu bằng eyJ...).")
				return
			}
			validatedTunnelTok = &tok
		}

		if state.DB == nil || state.DB.DB == nil {
			redirectWithNotice(w, r, "Lỗi kết nối cơ sở dữ liệu.")
			return
		}

		_ = db.SetSetting(state.DB.DB, "shopee_affiliate_id", affTrimmed)
		_ = db.SetSetting(state.DB.DB, "shopee_app_id", strings.TrimSpace(r.PostFormValue("shopee_app_id")))
		if secretTrimmed := strings.TrimSpace(r.PostFormValue("shopee_app_secret")); secretTrimmed != "" {
			_ = db.SetSetting(state.DB.DB, "shopee_app_secret", secretTrimmed)
		}
		if validatedSocial != nil {
			_ = db.SetSetting(state.DB.DB, "shopee_social_params", *validatedSocial)
		}
		if validatedTunnelTok != nil {
			_ = db.SetSetting(state.DB.DB, "cloudflare_tunnel_token", *validatedTunnelTok)
		}
		_ = db.SetSetting(state.DB.DB, "shortlink_base_url", shortBaseTrimmed)

		mobileFlag := "0"
		if r.PostFormValue("shortlink_mobile_only") == "1" {
			mobileFlag = "1"
		}
		_ = db.SetSetting(state.DB.DB, "shortlink_mobile_only", mobileFlag)

		homeCloakFlag := "0"
		if r.PostFormValue("public_home_cloak") == "1" {
			homeCloakFlag = "1"
		}
		_ = db.SetSetting(state.DB.DB, "public_home_cloak", homeCloakFlag)

		state.DB.InvalidateSettingsCache()
		slog.Info("✅ Quản trị viên đã cập nhật cấu hình Shopee Chính Chủ & Bọc Link trên Web Admin.")
		redirectWithNotice(w, r, "Đã lưu cấu hình Shopee Chính Chủ & Bọc Link ngay lập tức!")
	}
}

// HandleUpdateNotifications cập nhật Telegram ID nhận báo đơn (`POST /admin/settings/notifications`).
func HandleUpdateNotifications(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		if err := r.ParseForm(); err != nil {
			redirectWithNotice(w, r, "Dữ liệu form không hợp lệ.")
			return
		}
		trimmed := strings.TrimSpace(r.PostFormValue("admin_telegram_id"))
		if trimmed != "" {
			if _, err := strconv.ParseInt(trimmed, 10, 64); err != nil {
				redirectWithNotice(w, r, "Telegram ID phải là dãy số nguyên (lấy bằng cách gõ /stats trong Bot).")
				return
			}
		}
		if state.DB == nil || state.DB.DB == nil {
			redirectWithNotice(w, r, "Lỗi kết nối cơ sở dữ liệu.")
			return
		}
		_ = db.SetSetting(state.DB.DB, "admin_telegram_id", trimmed)
		state.DB.InvalidateSettingsCache()
		if trimmed == "" {
			redirectWithNotice(w, r, "Đã tắt thông báo đơn hàng qua Telegram.")
		} else {
			redirectWithNotice(w, r, fmt.Sprintf("Đã lưu Telegram ID %s để nhận báo đơn tự động!", trimmed))
		}
	}
}

// HandleSyncOrders quét đơn mới thủ công từ Web Admin (`POST /admin/orders/sync`).
func HandleSyncOrders(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		summary := orders.SyncOrdersOnce(state.DB, state.Config, state.AccessTrade)
		if len(summary.Errors) == 0 {
			redirectWithNotice(w, r, fmt.Sprintf("Đối soát xong: Quét %d đơn trong 7 ngày qua, phát hiện %d đơn mới!", summary.Fetched, summary.NewlyInserted))
		} else {
			redirectWithNotice(w, r, fmt.Sprintf("Quét %d đơn (mới %d). Lưu ý: %s", summary.Fetched, summary.NewlyInserted, strings.Join(summary.Errors, "; ")))
		}
	}
}

// HandleSyncVouchers quét sâu kho mã thị trường (`POST /admin/vouchers/sync`).
func HandleSyncVouchers(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		token := resolveAccessTradeToken(state.DB, state.Config)
		if token == "" {
			redirectWithNotice(w, r, "Chưa cấu hình ACCESSTRADE API Token.")
			return
		}
		list, err := state.AccessTrade.FetchHighDiscountVouchers(token, 0, true)
		if err != nil {
			redirectWithNotice(w, r, fmt.Sprintf("Lỗi đồng bộ kho mã: %v", err))
			return
		}
		redirectWithNotice(w, r, fmt.Sprintf("Đã đồng bộ thành công tổng cộng %d mã giảm giá toàn thị trường từ ACCESSTRADE (không bỏ sót mã nào)!", len(list)))
	}
}

// HandleAddDeal thêm lẻ 1 mã độc quyền (`POST /admin/deals/add`).
func HandleAddDeal(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		if err := r.ParseForm(); err != nil {
			redirectWithNotice(w, r, "Dữ liệu form không hợp lệ.")
			return
		}
		dealURL := strings.TrimSpace(r.PostFormValue("deal_url"))
		if !isValidHTTPURL(dealURL) {
			redirectWithNotice(w, r, "Link áp mã phải bắt đầu bằng https:// hoặc http://")
			return
		}
		if state.DB == nil || state.DB.DB == nil {
			redirectWithNotice(w, r, "Lỗi kết nối cơ sở dữ liệu.")
			return
		}

		var minSpend float64
		if ms, err := strconv.ParseFloat(strings.TrimSpace(r.PostFormValue("min_spend")), 64); err == nil && ms > 0 {
			minSpend = ms
		}
		var couponPtr *string
		if codeClean := strings.TrimSpace(r.PostFormValue("coupon_code")); codeClean != "" {
			couponPtr = &codeClean
		}

		item := db.NewDealInput{
			Platform:     strings.TrimSpace(r.PostFormValue("platform")),
			Title:        strings.TrimSpace(r.PostFormValue("title")),
			CouponCode:   couponPtr,
			DiscountDesc: strings.TrimSpace(r.PostFormValue("discount_desc")),
			MinSpend:     minSpend,
			DealURL:      dealURL,
			IsExclusive:  r.PostFormValue("is_exclusive") != "",
		}
		inserted, err := db.AddDeal(state.DB.DB, item)
		if err != nil {
			redirectWithNotice(w, r, fmt.Sprintf("Lỗi lưu mã: %v", err))
			return
		}
		state.DB.InvalidateDealsCache()
		actionWord := "cập nhật"
		msg := "Đã cập nhật lại Mã Độc Quyền trùng code trong kho!"
		if inserted {
			actionWord = "thêm mới"
			msg = "Đã lưu Mã Độc Quyền mới vào kho Bot!"
		}
		slog.Info("👑 Admin deal update", "action", actionWord, "title", utils.SanitizeCRLF(item.Title), "platform", utils.SanitizeCRLF(item.Platform))
		redirectWithNotice(w, r, msg)
	}
}

// ParseBulkExclusiveDeals phân tích văn bản nhập hàng loạt (dấu `|` hoặc Tab từ Excel/Google Sheets).
func ParseBulkExclusiveDeals(rawText string) ([]db.NewDealInput, int) {
	var items []db.NewDealInput
	skipped := 0

	for _, rawLine := range strings.Split(rawText, "\n") {
		line := strings.TrimSpace(rawLine)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		sep := "|"
		if strings.Contains(line, "\t") {
			sep = "\t"
		}
		parts := strings.Split(line, sep)
		if len(parts) < 5 {
			skipped++
			continue
		}
		for i := range parts {
			parts[i] = strings.TrimSpace(parts[i])
		}
		platform := parts[0]
		title := parts[1]
		code := parts[2]
		desc := parts[3]
		targetURL := parts[4]
		var minSpend float64
		if len(parts) > 5 {
			cleanNum := strings.NewReplacer(",", "", ".", "").Replace(parts[5])
			if parsed, err := strconv.ParseFloat(cleanNum, 64); err == nil && parsed > 0 {
				minSpend = parsed
			}
		}

		if platform == "" || title == "" || desc == "" || !isValidHTTPURL(targetURL) {
			skipped++
			continue
		}
		var codePtr *string
		if code != "" {
			c := code
			codePtr = &c
		}
		items = append(items, db.NewDealInput{
			Platform:     platform,
			Title:        title,
			CouponCode:   codePtr,
			DiscountDesc: desc,
			MinSpend:     minSpend,
			DealURL:      targetURL,
			IsExclusive:  true,
		})
	}
	return items, skipped
}

// HandleImportDeals nhập hàng loạt mã độc quyền (`POST /admin/deals/import`).
func HandleImportDeals(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		if err := r.ParseForm(); err != nil {
			redirectWithNotice(w, r, "Dữ liệu form không hợp lệ.")
			return
		}
		items, skipped := ParseBulkExclusiveDeals(r.PostFormValue("bulk_lines"))
		if len(items) == 0 {
			redirectWithNotice(w, r, "Không có dòng nào đúng định dạng (Sàn | Tên | Mã Code | Mức giảm | Link https://... | Đơn tối thiểu).")
			return
		}
		if state.DB == nil || state.DB.DB == nil {
			redirectWithNotice(w, r, "Lỗi kết nối cơ sở dữ liệu.")
			return
		}
		inserted, updated, err := db.AddDealsBatch(state.DB.DB, items)
		if err != nil {
			redirectWithNotice(w, r, fmt.Sprintf("Lỗi nhập hàng loạt: %v", err))
			return
		}
		state.DB.InvalidateDealsCache()
		slog.Info("📥 Admin nhập hàng loạt mã độc quyền", "inserted", inserted, "updated", updated, "skipped", skipped)
		redirectWithNotice(w, r, fmt.Sprintf("Nhập xong: +%d mã mới, cập nhật %d mã (bỏ qua %d dòng sai định dạng).", inserted, updated, skipped))
	}
}

// HandleToggleDeal bật/tắt trạng thái deal (`POST /admin/deals/toggle`).
func HandleToggleDeal(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		if err := r.ParseForm(); err == nil && state.DB != nil && state.DB.DB != nil {
			if id, pErr := strconv.ParseInt(strings.TrimSpace(r.PostFormValue("deal_id")), 10, 64); pErr == nil {
				_ = db.ToggleDeal(state.DB.DB, id)
				state.DB.InvalidateDealsCache()
			}
		}
		http.Redirect(w, r, "/admin", http.StatusSeeOther)
	}
}

// HandleDeleteDeal xóa deal (`POST /admin/deals/delete`).
func HandleDeleteDeal(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		if err := r.ParseForm(); err == nil && state.DB != nil && state.DB.DB != nil {
			if id, pErr := strconv.ParseInt(strings.TrimSpace(r.PostFormValue("deal_id")), 10, 64); pErr == nil {
				_ = db.DeleteDeal(state.DB.DB, id)
				state.DB.InvalidateDealsCache()
			}
		}
		http.Redirect(w, r, "/admin", http.StatusSeeOther)
	}
}

// HandleUpdateAffiliateSettings cập nhật mẫu deeplink dự phòng (`POST /admin/settings/affiliate`).
func HandleUpdateAffiliateSettings(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !IsAuthenticated(state, r) {
			http.Redirect(w, r, "/admin/login", http.StatusSeeOther)
			return
		}
		if err := r.ParseForm(); err != nil {
			redirectWithNotice(w, r, "Dữ liệu form không hợp lệ.")
			return
		}
		tmpl := strings.TrimSpace(r.PostFormValue("template"))
		if tmpl != "" && !isValidHTTPURL(tmpl) {
			redirectWithNotice(w, r, "Mẫu link Affiliate dự phòng phải bắt đầu bằng https:// hoặc http:// (hoặc để trống để tắt).")
			return
		}
		if state.DB != nil && state.DB.DB != nil {
			_ = db.SetSetting(state.DB.DB, "affiliate_template", tmpl)
			state.DB.InvalidateSettingsCache()
			slog.Info("✅ Đã cập nhật mẫu link Affiliate dự phòng trên Web Admin.")
		}
		http.Redirect(w, r, "/admin", http.StatusSeeOther)
	}
}
