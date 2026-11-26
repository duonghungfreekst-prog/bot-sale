package web

import (
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"
	"unicode/utf8"

	salehunterassets "salehunter-go"
	"salehunter-go/internal/bot"
	"salehunter-go/internal/db"
	"salehunter-go/internal/services/accesstrade"
	"salehunter-go/internal/services/social"
	"salehunter-go/internal/utils"
)

// PublicHomeQuery tham số tìm kiếm & lọc sàn ở trang chủ công khai.
type PublicHomeQuery struct {
	Q    string
	Plat string
}

// HandleRoot xử lý GET `/`.
func HandleRoot(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if IsPublicHomeCloaked(state) {
			notFoundCloakResponse(w)
			return
		}
		q := PublicHomeQuery{
			Q:    r.URL.Query().Get("q"),
			Plat: r.URL.Query().Get("plat"),
		}
		RenderPublicHome(w, state, q, "", "")
	}
}

// HandlePublicConvert xử lý POST `/convert` (đổi link sản phẩm trực tiếp trên web).
func HandlePublicConvert(state *AppState) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if IsPublicHomeCloaked(state) {
			notFoundCloakResponse(w)
			return
		}
		if err := r.ParseForm(); err != nil {
			http.Error(w, "Bad request", http.StatusBadRequest)
			return
		}
		inputRaw := strings.TrimSpace(r.PostFormValue("url"))
		clientIP := ExtractClientIP(r)
		nowTS := time.Now().Unix()

		// Rate limit 20 lượt / 60 giây theo IP
		state.ConvertLimiterMu.Lock()
		PruneRateLimiterMap(state.ConvertLimiter, nowTS, 60, 4000)
		entry := state.ConvertLimiter[clientIP]
		if entry.LastTS == 0 || (nowTS-entry.LastTS) >= 60 {
			state.ConvertLimiter[clientIP] = RateEntry{Count: 1, LastTS: nowTS}
			state.ConvertLimiterMu.Unlock()
		} else if entry.Count >= 20 {
			state.ConvertLimiterMu.Unlock()
			errHTML := `<div class="convert-alert convert-alert--error" role="alert"><svg class="ico" aria-hidden="true"><use href="#ico-clock"/></svg><span>Bạn đang gửi quá nhanh. Vui lòng chờ khoảng 1 phút rồi thử đổi link tiếp nhé!</span></div>`
			RenderPublicHome(w, state, PublicHomeQuery{}, inputRaw, errHTML)
			return
		} else {
			entry.Count++
			state.ConvertLimiter[clientIP] = entry
			state.ConvertLimiterMu.Unlock()
		}

		if _, ok := state.Parser.ExtractFirstURL(inputRaw); !ok {
			errHTML := `<div class="convert-alert convert-alert--error" role="alert"><svg class="ico" aria-hidden="true"><use href="#ico-warn"/></svg><span>Vui lòng dán đúng đường link sản phẩm bắt đầu bằng <code>https://</code> từ Shopee, Tiki, Lazada, TikTok Shop hoặc Alibaba.</span></div>`
			RenderPublicHome(w, state, PublicHomeQuery{}, inputRaw, errHTML)
			return
		}

		if state.DB == nil || state.DB.DB == nil {
			errHTML := `<div class="convert-alert convert-alert--error" role="alert"><svg class="ico" aria-hidden="true"><use href="#ico-warn"/></svg><span>Hệ thống cơ sở dữ liệu tạm bận, vui lòng thử lại sau ít giây.</span></div>`
			RenderPublicHome(w, state, PublicHomeQuery{}, inputRaw, errHTML)
			return
		}

		webGuest := "web_guest"
		_, _, _ = db.GetOrCreateUser(state.DB.DB, 0, &webGuest, "Khách Web Thế Giới Siêu Sale", nil)

		ownedAff := bot.LoadOwnedAffSettingsCached(state.DB, state.Config)
		_ = state.AccessTrade.GetOrLoadApprovedCampaigns(ownedAff.AccessTradeToken)

		parsed, err := state.Parser.ProcessLink(inputRaw, 0, nil, ownedAff.ToParserSettings())
		if err != nil {
			errHTML := fmt.Sprintf(
				`<div class="convert-alert convert-alert--error" role="alert"><svg class="ico" aria-hidden="true"><use href="#ico-warn"/></svg><span>Không thể phân tích link này: %s</span></div>`,
				utils.EscapeHTML(err.Error()),
			)
			RenderPublicHome(w, state, PublicHomeQuery{}, inputRaw, errHTML)
			return
		}

		clickRecord := db.ClickRecord{
			ID:           parsed.ClickID,
			UserID:       0,
			Platform:     parsed.Platform.AsStr(),
			OriginalURL:  parsed.OriginalURL,
			CleanURL:     parsed.CleanURL,
			AffiliateURL: parsed.AffiliateURL,
			SubID:        parsed.SubID,
			CreatedAt:    time.Now().UTC().Format(time.RFC3339),
		}
		_ = db.RecordClick(state.DB.DB, clickRecord)

		wrappedMain := state.DB.WrapURLCached(
			ownedAff.ShortlinkBase,
			parsed.AffiliateURL,
			parsed.ProductTitle,
			parsed.Platform.AsStr(),
		)

		var metaParts []string
		if parsed.ProductShop != nil {
			metaParts = append(metaParts, fmt.Sprintf("Shop: <strong>%s</strong>", utils.EscapeHTML(*parsed.ProductShop)))
		}
		if parsed.ProductPriceVND != nil && *parsed.ProductPriceVND > 0 {
			metaParts = append(metaParts, fmt.Sprintf("Giá tham khảo: <strong>%s</strong>", utils.EscapeHTML(utils.FormatVND(*parsed.ProductPriceVND))))
		}
		metaLine := ""
		if len(metaParts) > 0 {
			metaLine = fmt.Sprintf(`<p class="convert-result__meta">%s</p>`, strings.Join(metaParts, " · "))
		}

		var matchedBtns strings.Builder
		seenBtnURLs := make(map[string]struct{})
		if deals, dErr := state.DB.GetActiveDealsCached(parsed.Platform.AsStr()); dErr == nil {
			for _, d := range bot.SelectTopRelevantDeals(deals, parsed, 2) {
				enrichedDealURL := bot.EnrichDealURLForPlatform(
					d.DealURL,
					d.Platform,
					ownedAff.ShopeeAffiliateID,
					ownedAff.ShopeeSocialParams,
					parsed.SubID,
				)
				if _, dup := seenBtnURLs[enrichedDealURL]; dup {
					continue
				}
				seenBtnURLs[enrichedDealURL] = struct{}{}

				wrappedEx := state.DB.WrapURLCached(ownedAff.ShortlinkBase, enrichedDealURL, d.Title, d.Platform)
				shortLabel := d.DiscountDesc
				if utf8.RuneCountInString(shortLabel) > 38 {
					shortLabel = utils.TakeRunes(shortLabel, 36) + "..."
				}
				codeLabel := ""
				if d.CouponCode != nil {
					trimmedCode := strings.TrimSpace(*d.CouponCode)
					if trimmedCode != "" && !social.IsSyntheticTrackingCode(trimmedCode) {
						codeLabel = " · " + trimmedCode
					}
				}
				matchedBtns.WriteString(fmt.Sprintf(
					`<a class="btn btn--secondary" href="%s" target="_blank" rel="noopener noreferrer"><svg class="ico" aria-hidden="true"><use href="#ico-crown"/></svg><span>%s%s</span><svg class="ico" aria-hidden="true"><use href="#ico-external"/></svg></a>`,
					utils.EscapeHTML(wrappedEx),
					utils.EscapeHTML(shortLabel),
					utils.EscapeHTML(codeLabel),
				))
			}
		}

		approvedCached := state.AccessTrade.PeekApprovedCampaigns()
		matchedMarket := state.AccessTrade.SearchMatchingVouchersForDeal(accesstrade.VoucherMatchQuery{
			Platform:     parsed.Platform.AsStr(),
			ProductTitle: parsed.ProductTitle,
			CleanURL:     parsed.CleanURL,
			ShopName:     parsed.ProductShop,
			BrandName:    parsed.ProductBrand,
			PriceVND:     parsed.ProductPriceVND,
			MaxItems:     2,
		})
		for _, mv := range matchedMarket {
			rawVURL := state.AccessTrade.VerifiedOfferURL(mv, approvedCached)
			enrichedVURL := bot.EnrichDealURLForPlatform(
				rawVURL,
				mv.Platform,
				ownedAff.ShopeeAffiliateID,
				ownedAff.ShopeeSocialParams,
				parsed.SubID,
			)
			wrappedV := state.DB.WrapURLCached(ownedAff.ShortlinkBase, enrichedVURL, mv.ShopName, mv.Platform)
			matchedBtns.WriteString(fmt.Sprintf(
				`<a class="btn btn--secondary" href="%s" target="_blank" rel="noopener noreferrer"><svg class="ico" aria-hidden="true"><use href="#ico-ticket"/></svg><span>Mã %s (%s)</span><svg class="ico" aria-hidden="true"><use href="#ico-external"/></svg></a>`,
				utils.EscapeHTML(wrappedV),
				utils.EscapeHTML(mv.CouponCode),
				utils.EscapeHTML(mv.DiscountBadge()),
			))
		}

		affBadgeText := "Đã gắn link ưu đãi trực tiếp"
		if ownedAff.ShortlinkBase != "" {
			affBadgeText = "Đã gắn link bọc ưu đãi"
		}
		platDotColor := platformBrandColor(parsed.Platform.AsStr())

		resultHTML := fmt.Sprintf(`<div class="convert-result" aria-live="polite">
                    <div class="convert-result__head">
                        <span class="badge badge--platform"><span class="plat-dot" style="background:%s;" aria-hidden="true"></span><span>%s</span></span>
                        <span class="badge badge--accent">%s</span>
                    </div>
                    <h2 class="convert-result__title">%s</h2>
                    %s
                    <div class="convert-result__url"><code>%s</code></div>
                    <div class="convert-actions">
                        <a class="btn btn--primary" href="%s" target="_blank" rel="noopener noreferrer"><svg class="ico" aria-hidden="true"><use href="#ico-rocket"/></svg><span>Mở Ứng Dụng &amp; Áp Mã Ngay</span><svg class="ico" aria-hidden="true"><use href="#ico-external"/></svg></a>
                        <button type="button" class="btn btn--secondary js-copy-link" data-copy-url="%s"><svg class="ico" aria-hidden="true"><use href="#ico-copy"/></svg><span class="copy-lbl">Sao chép link ưu đãi</span></button>
                        %s
                    </div>
                </div>`,
			platDotColor,
			utils.EscapeHTML(parsed.Platform.AsStr()),
			affBadgeText,
			utils.EscapeHTML(parsed.ProductTitle),
			metaLine,
			utils.EscapeHTML(wrappedMain),
			utils.EscapeHTML(wrappedMain),
			utils.EscapeHTML(wrappedMain),
			matchedBtns.String(),
		)

		RenderPublicHome(w, state, PublicHomeQuery{}, inputRaw, resultHTML)
	}
}

// HandleHomeJS phục vụ `/assets/home.js`.
func HandleHomeJS(w http.ResponseWriter, _ *http.Request) {
	js := `function syncPublicThemeBtn() {
    var themeBtn = document.getElementById("public-theme-btn");
    if (!themeBtn) return;
    var isDark = document.documentElement.getAttribute("data-theme") === "dark";
    var icoUse = document.getElementById("public-theme-ico-use");
    var lbl = document.getElementById("public-theme-text");
    if (icoUse && lbl) {
        icoUse.setAttribute("href", isDark ? "#ico-sun" : "#ico-moon");
        lbl.textContent = isDark ? "Giao diện Sáng" : "Giao diện Tối";
    } else {
        themeBtn.textContent = isDark ? "Giao diện Sáng" : "Giao diện Tối";
    }
    themeBtn.setAttribute("aria-pressed", isDark ? "true" : "false");
}
document.addEventListener("DOMContentLoaded", syncPublicThemeBtn);
document.addEventListener("click", function (e) {
    var tBtn = e.target.closest("#public-theme-btn");
    if (tBtn) {
        var isDark = document.documentElement.getAttribute("data-theme") === "dark";
        var next = isDark ? "light" : "dark";
        if (next === "dark") {
            document.documentElement.setAttribute("data-theme", "dark");
        } else {
            document.documentElement.removeAttribute("data-theme");
        }
        try { localStorage.setItem("dealx-theme", next); } catch (_) {}
        syncPublicThemeBtn();
        return;
    }
    var btn = e.target.closest(".js-copy-link");
    if (!btn) return;
    var url = btn.getAttribute("data-copy-url");
    var fromTarget = false;
    if (!url) {
        var targetId = btn.getAttribute("data-copy-target");
        if (targetId) {
            var el = document.getElementById(targetId);
            if (el) {
                url = typeof el.value === "string" ? el.value : el.textContent;
                fromTarget = true;
            }
        }
    }
    if (!url) return;
    if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(url).then(function () {
            var lbl = btn.querySelector(".copy-lbl");
            if (lbl) {
                var old = lbl.textContent;
                lbl.textContent = btn.classList.contains("copy-code-btn") ? "Đã chép" : (fromTarget ? "Đã sao chép code" : "Đã sao chép link");
                btn.classList.add("is-copied");
                setTimeout(function () {
                    lbl.textContent = old;
                    btn.classList.remove("is-copied");
                }, 2000);
            } else {
                var oldText = btn.textContent;
                btn.textContent = fromTarget ? "✅ Đã sao chép code" : "✅ Đã sao chép link";
                setTimeout(function () { btn.textContent = oldText; }, 2000);
            }
        });
    }
});`
	serveStaticAsset(w, []byte(js), "application/javascript; charset=utf-8", 300)
}

func serveStaticAsset(w http.ResponseWriter, body []byte, contentType string, maxAge int) {
	w.Header().Set("Content-Type", contentType)
	w.Header().Set("Cache-Control", fmt.Sprintf("public, max-age=%d", maxAge))
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(body)
}

// HandleBotLogoWebp phục vụ `/assets/bot-logo.webp`.
func HandleBotLogoWebp(w http.ResponseWriter, _ *http.Request) {
	serveStaticAsset(w, salehunterassets.BotLogoWebp, "image/webp", 86400)
}

// HandleBotCart3DWebp phục vụ `/assets/bot-cart-3d.webp`.
func HandleBotCart3DWebp(w http.ResponseWriter, _ *http.Request) {
	serveStaticAsset(w, salehunterassets.BotCart3DWebp, "image/webp", 86400)
}

// HandleLandingJS phục vụ `/assets/landing.js`.
func HandleLandingJS(w http.ResponseWriter, _ *http.Request) {
	js := `(function () {
    if (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    var stage = document.getElementById("stage3d");
    if (!stage) return;
    stage.addEventListener("pointermove", function (e) {
        if (e.pointerType === "touch") return;
        var r = stage.getBoundingClientRect();
        if (!r.width || !r.height) return;
        var px = (e.clientX - r.left) / r.width;
        var py = (e.clientY - r.top) / r.height;
        stage.classList.add("is-interacting");
        stage.style.setProperty("--rx", ((0.5 - py) * 12).toFixed(2) + "deg");
        stage.style.setProperty("--ry", ((px - 0.5) * 14).toFixed(2) + "deg");
        stage.style.setProperty("--gx", (px * 100).toFixed(1) + "%");
        stage.style.setProperty("--gy", (py * 100).toFixed(1) + "%");
    });
    stage.addEventListener("pointerleave", function () {
        stage.classList.remove("is-interacting");
        stage.style.setProperty("--rx", "0deg");
        stage.style.setProperty("--ry", "0deg");
        stage.style.setProperty("--gx", "50%");
        stage.style.setProperty("--gy", "45%");
    });
})();`
	serveStaticAsset(w, []byte(js), "application/javascript; charset=utf-8", 3600)
}

func platformBrandColor(platform string) string {
	p := strings.ToLower(strings.TrimSpace(platform))
	switch {
	case strings.Contains(p, "shopee"):
		return "#EA580C"
	case strings.Contains(p, "lazada"):
		return "#DB2777"
	case strings.Contains(p, "tiki"):
		return "#0284C7"
	case strings.Contains(p, "tiktok"):
		return "#E11D48"
	case strings.Contains(p, "alibaba"):
		return "#D97706"
	default:
		return "#64748B"
	}
}

const publicSVGSprite = `<svg aria-hidden="true" style="position:absolute;width:0;height:0;overflow:hidden">
    <defs>
        <symbol id="ico-sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/></symbol>
        <symbol id="ico-moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/></symbol>
        <symbol id="ico-shield" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="m9 12 2 2 4-4"/></symbol>
        <symbol id="ico-ticket" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 9a3 3 0 0 1 0 6v2a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-2a3 3 0 0 1 0-6V7a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2Z"/><path d="M13 5v2"/><path d="M13 17v2"/><path d="M13 11v2"/></symbol>
        <symbol id="ico-clock" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15.5 14"/></symbol>
        <symbol id="ico-zap" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></symbol>
        <symbol id="ico-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></symbol>
        <symbol id="ico-copy" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></symbol>
        <symbol id="ico-external" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></symbol>
        <symbol id="ico-crown" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m2 4 3 12h14l3-12-6 7-4-7-4 7-6-7zm3 16h14"/></symbol>
        <symbol id="ico-rocket" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z"/><path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z"/></symbol>
        <symbol id="ico-warn" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></symbol>
    </defs>
</svg>`

type publicDealCardData struct {
	Platform   string
	Badge      string
	Title      string
	Desc       string
	CodeTxt    string
	NoteTxt    string
	WrappedURL string
	CTALabel   string
}

func renderPublicDealCard(card publicDealCardData) string {
	dotColor := platformBrandColor(card.Platform)
	trimmedCode := strings.TrimSpace(card.CodeTxt)
	hasManualCode := !social.IsSyntheticTrackingCode(trimmedCode) && !strings.EqualFold(trimmedCode, "Tự động áp dụng")
	var couponRowHTML string
	if hasManualCode {
		couponRowHTML = fmt.Sprintf(
			`<span class="coupon-pill">Mã: <code>%s</code></span>
                <button type="button" class="copy-code-btn js-copy-link" data-copy-url="%s" aria-label="Sao chép mã %s"><svg class="ico" style="width:13px;height:13px;" aria-hidden="true"><use href="#ico-copy"/></svg><span class="copy-lbl">Chép mã</span></button>`,
			utils.EscapeHTML(trimmedCode),
			utils.EscapeHTML(trimmedCode),
			utils.EscapeHTML(trimmedCode),
		)
	} else {
		couponRowHTML = `<span class="coupon-pill coupon-pill--link"><svg class="ico" style="width:13px;height:13px;" aria-hidden="true"><use href="#ico-zap"/></svg><span>Áp tự động qua link (Không cần gõ mã)</span></span>`
	}

	finalCTALabel := card.CTALabel
	if !hasManualCode {
		finalCTALabel = "Lưu & Áp ưu đãi " + card.Platform
	}

	return fmt.Sprintf(`<article class="deal-card" style="--plat-accent:%s;">
            <div class="deal-card__top">
                <span class="badge badge--platform"><span class="plat-dot" style="background:%s;" aria-hidden="true"></span><span>%s</span></span>
                <span class="badge badge--accent">%s</span>
            </div>
            <h3 class="deal-card__title">%s</h3>
            <p class="deal-card__desc">%s</p>
            <div class="deal-card__meta">
                <div class="coupon-row">
                    %s
                </div>
                <span class="deal-card__note">%s</span>
            </div>
            <a class="btn btn--primary deal-card__cta" href="%s" target="_blank" rel="noopener noreferrer">
                <span>%s</span>
                <svg class="ico" style="width:15px;height:15px;" aria-hidden="true"><use href="#ico-external"/></svg>
            </a>
        </article>`,
		dotColor,
		dotColor,
		utils.EscapeHTML(card.Platform),
		utils.EscapeHTML(card.Badge),
		utils.EscapeHTML(card.Title),
		utils.EscapeHTML(card.Desc),
		couponRowHTML,
		utils.EscapeHTML(card.NoteTxt),
		utils.EscapeHTML(card.WrappedURL),
		utils.EscapeHTML(finalCTALabel),
	)
}

// RenderPublicHome render toàn bộ trang chủ công khai `/` kèm form chuyển đổi link & kho mã.
func RenderPublicHome(
	w http.ResponseWriter,
	state *AppState,
	query PublicHomeQuery,
	convertInputVal string,
	convertResultHTML string,
) {
	searchQ := strings.TrimSpace(query.Q)
	filterPlat := strings.TrimSpace(query.Plat)
	if filterPlat == "" {
		filterPlat = "all"
	}

	dbDeals, _ := state.DB.GetActiveDealsCached("")
	ownedAff := bot.LoadOwnedAffSettingsCached(state.DB, state.Config)
	marketVouchers := state.AccessTrade.PeekCachedHighDiscountVouchers()
	approved := state.AccessTrade.PeekApprovedCampaigns()
	syncMeta := state.AccessTrade.GetVoucherSyncMeta()

	var cleanDBDeals []db.DealItem
	for _, d := range dbDeals {
		if !social.IsGuideNoiseDealText(d.DiscountDesc) && !social.IsGuideNoiseDealText(d.Title) {
			cleanDBDeals = append(cleanDBDeals, d)
		}
	}

	type preparedPublicCard struct {
		Platform    string
		Badge       string
		Title       string
		Desc        string
		CodeTxt     string
		NoteTxt     string
		EnrichedURL string
		CTALabel    string
	}

	qLower := strings.ToLower(searchQ)
	seenURLs := make(map[string]struct{})
	var preparedCards []preparedPublicCard

	for _, d := range cleanDBDeals {
		if len(preparedCards) >= 24 {
			break
		}
		platOK := filterPlat == "all" || strings.EqualFold(d.Platform, filterPlat)
		codeVal := ""
		if d.CouponCode != nil {
			codeVal = *d.CouponCode
		}
		qOK := qLower == "" ||
			strings.Contains(strings.ToLower(d.Title), qLower) ||
			strings.Contains(strings.ToLower(d.DiscountDesc), qLower) ||
			strings.Contains(strings.ToLower(codeVal), qLower) ||
			strings.Contains(strings.ToLower(d.Platform), qLower)
		if !platOK || !qOK {
			continue
		}
		norm := strings.ToLower(strings.TrimRight(strings.TrimSpace(d.DealURL), "/"))
		if norm != "" {
			if _, dup := seenURLs[norm]; dup {
				continue
			}
			seenURLs[norm] = struct{}{}
		}

		codeTxt := "Tự động áp dụng"
		if strings.TrimSpace(codeVal) != "" {
			codeTxt = codeVal
		}
		badge := "Mã giảm giá"
		if d.IsExclusive {
			badge = "Ưu đãi nổi bật"
		}
		noteTxt := "Không giới hạn đơn tối thiểu"
		if d.MinSpend > 0 {
			noteTxt = "Đơn từ " + utils.FormatVND(d.MinSpend)
		}
		enrichedURL := bot.EnrichDealURLForPlatform(
			d.DealURL,
			d.Platform,
			ownedAff.ShopeeAffiliateID,
			ownedAff.ShopeeSocialParams,
			"web_public_deal",
		)
		preparedCards = append(preparedCards, preparedPublicCard{
			Platform:    d.Platform,
			Badge:       badge,
			Title:       d.Title,
			Desc:        d.DiscountDesc,
			CodeTxt:     codeTxt,
			NoteTxt:     noteTxt,
			EnrichedURL: enrichedURL,
			CTALabel:    "Lấy mã & Xem ưu đãi",
		})
	}

	matchedFromAPI := 0
	for _, v := range marketVouchers {
		platOK := filterPlat == "all" || strings.EqualFold(v.Platform, filterPlat)
		qOK := qLower == "" ||
			strings.Contains(strings.ToLower(v.ShopName), qLower) ||
			strings.Contains(strings.ToLower(v.Title), qLower) ||
			strings.Contains(strings.ToLower(v.CouponCode), qLower) ||
			strings.Contains(strings.ToLower(v.DiscountDesc), qLower) ||
			strings.Contains(strings.ToLower(v.Platform), qLower)
		if !platOK || !qOK {
			continue
		}
		targetURL := v.BestTargetURL(approved)
		norm := strings.ToLower(strings.TrimRight(strings.TrimSpace(targetURL), "/"))
		if norm != "" {
			if _, dup := seenURLs[norm]; dup {
				continue
			}
			seenURLs[norm] = struct{}{}
		}
		matchedFromAPI++
		if len(preparedCards) >= 24 {
			continue
		}
		enrichedURL := bot.EnrichDealURLForPlatform(
			targetURL,
			v.Platform,
			ownedAff.ShopeeAffiliateID,
			ownedAff.ShopeeSocialParams,
			"web_public_voucher",
		)
		noteTxt := "Đang hiệu lực"
		if strings.TrimSpace(v.EndTime) != "" {
			noteTxt = "HSD: " + strings.TrimSpace(v.EndTime)
		}
		preparedCards = append(preparedCards, preparedPublicCard{
			Platform:    v.Platform,
			Badge:       v.DiscountBadge(),
			Title:       v.ShopName,
			Desc:        v.DiscountDesc,
			CodeTxt:     v.CouponCode,
			NoteTxt:     noteTxt,
			EnrichedURL: enrichedURL,
			CTALabel:    "Lấy mã & Mở gian hàng",
		})
	}

	totalAvailable := len(cleanDBDeals) + len(marketVouchers)
	matchedCount := len(preparedCards)
	if matchedFromAPI > 0 && len(preparedCards) >= 24 {
		// giữ chính xác tổng matched khi vượt ngưỡng 24 hiển thị
		matchedCount = len(seenURLs)
	}
	lastSyncText := syncMeta.LastSyncVN
	if lastSyncText == "" {
		lastSyncText = "Đang đồng bộ thời gian thực"
	}

	batchTargets := make([]db.WrapLinkItem, 0, len(preparedCards))
	for _, c := range preparedCards {
		batchTargets = append(batchTargets, db.WrapLinkItem{
			TargetURL: c.EnrichedURL,
			Title:     c.Title,
			Platform:  c.Platform,
		})
	}
	wrappedURLs := state.DB.WrapURLsBatchCached(ownedAff.ShortlinkBase, batchTargets)

	var cardsBuilder strings.Builder
	for i, c := range preparedCards {
		wrapped := c.EnrichedURL
		if i < len(wrappedURLs) && wrappedURLs[i] != "" {
			wrapped = wrappedURLs[i]
		}
		cardsBuilder.WriteString(renderPublicDealCard(publicDealCardData{
			Platform:   c.Platform,
			Badge:      c.Badge,
			Title:      c.Title,
			Desc:       c.Desc,
			CodeTxt:    c.CodeTxt,
			NoteTxt:    c.NoteTxt,
			WrappedURL: wrapped,
			CTALabel:   c.CTALabel,
		}))
	}

	cardsHTML := cardsBuilder.String()
	if cardsHTML == "" {
		emptyMsg := "Không tìm thấy ưu đãi nào khớp với từ khóa hoặc sàn bạn vừa chọn. Hãy thử xóa bộ lọc để xem toàn bộ ưu đãi đang hoạt động."
		if totalAvailable == 0 {
			emptyMsg = "Hệ thống đang đồng bộ danh sách ưu đãi mới nhất từ đối tác. Bạn có thể quay lại sau ít phút hoặc gửi trực tiếp link sản phẩm qua trợ lý Telegram để kiểm tra nhanh."
		}
		cardsHTML = fmt.Sprintf(`<div class="empty-state">
                <p>%s</p>
                <a class="btn btn--secondary" href="/">Xem toàn bộ ưu đãi</a>
            </div>`, emptyMsg)
	}

	filterDefs := [][2]string{
		{"all", "Tất cả sàn"},
		{"Shopee", "Shopee"},
		{"Tiki", "Tiki"},
		{"Lazada", "Lazada"},
		{"TikTok Shop", "TikTok Shop"},
		{"Alibaba", "Alibaba"},
	}
	var filterLinksBuilder strings.Builder
	for _, fd := range filterDefs {
		val, label := fd[0], fd[1]
		activeCls := "filter-pill"
		if strings.EqualFold(filterPlat, val) {
			activeCls = "filter-pill filter-pill--active"
		}
		href := "/?plat=" + url.QueryEscape(val)
		if searchQ != "" {
			href = "/?q=" + url.QueryEscape(searchQ) + "&plat=" + url.QueryEscape(val)
		}
		dotHTML := ""
		if val != "all" {
			dotHTML = fmt.Sprintf(`<span class="plat-dot" style="background:%s;" aria-hidden="true"></span>`, platformBrandColor(val))
		}
		filterLinksBuilder.WriteString(fmt.Sprintf(`<a class="%s" href="%s">%s%s</a>`, activeCls, href, dotHTML, label))
	}

	html := fmt.Sprintf(`<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Thế Giới Siêu Sale — Tổng Hợp Mã Giảm Giá &amp; Gợi Ý Sản Phẩm Chính Hãng</title>
    <meta name="description" content="Cổng tổng hợp mã giảm giá công khai theo từng ngành hàng từ Shopee, Tiki, Lazada, TikTok Shop giúp người mua tiết kiệm mỗi ngày.">
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600;700;800&amp;family=Lexend:wght@600;700;800&amp;family=JetBrains+Mono:wght@500;600&amp;display=swap">
    <script>
        (function () {
            try {
                if (localStorage.getItem("dealx-theme") === "dark") {
                    document.documentElement.setAttribute("data-theme", "dark");
                }
            } catch (e) {}
        })();
    </script>
    <style>
        :root {
            color-scheme: light;
            --color-bg: #F4F6F9;
            --color-header: rgba(255, 255, 255, 0.92);
            --color-surface: #FFFFFF;
            --color-surface-elevated: #F8FAFC;
            --color-inset: #F1F5F9;
            --color-text: #0F172A;
            --color-text-secondary: #334155;
            --color-text-muted: #475569;
            --color-primary: #C2410C;
            --color-primary-hover: #9A3412;
            --color-primary-bright: #EA580C;
            --color-primary-contrast: #FFFFFF;
            --color-primary-soft: #FFF7ED;
            --color-primary-soft-border: #FED7AA;
            --color-border: #E2E8F0;
            --color-border-strong: #CBD5E1;
            --color-success: #15803D;
            --color-success-bg: #DCFCE7;
            --color-success-border: #86EFAC;
            --color-err: #B91C1C;
            --color-err-bg: #FEE2E2;
            --color-err-border: #FCA5A5;
            --radius-sm: 8px;
            --radius-md: 12px;
            --radius-lg: 18px;
            --shadow-sm: 0 1px 2px rgba(15, 23, 42, 0.05);
            --shadow-md: 0 10px 28px -8px rgba(15, 23, 42, 0.1), 0 2px 6px rgba(15, 23, 42, 0.04);
            --shadow-hero: 0 24px 48px -14px rgba(194, 65, 12, 0.18), 0 8px 20px rgba(15, 23, 42, 0.06);
            --font-sans: "Be Vietnam Pro", system-ui, -apple-system, sans-serif;
            --font-display: "Lexend", "Be Vietnam Pro", system-ui, sans-serif;
            --font-mono: "JetBrains Mono", monospace;
        }
        html[data-theme="dark"] {
            color-scheme: dark;
            --color-bg: #0B1120;
            --color-header: rgba(15, 23, 42, 0.92);
            --color-surface: #131C31;
            --color-surface-elevated: #18233C;
            --color-inset: #0D1526;
            --color-text: #F8FAFC;
            --color-text-secondary: #E2E8F0;
            --color-text-muted: #94A3B8;
            --color-primary: #F97316;
            --color-primary-hover: #FB923C;
            --color-primary-bright: #FB923C;
            --color-primary-contrast: #090D16;
            --color-primary-soft: rgba(249, 115, 22, 0.16);
            --color-primary-soft-border: rgba(249, 115, 22, 0.38);
            --color-border: rgba(148, 163, 184, 0.18);
            --color-border-strong: rgba(148, 163, 184, 0.32);
            --color-success: #86EFAC;
            --color-success-bg: rgba(34, 197, 94, 0.16);
            --color-success-border: rgba(34, 197, 94, 0.38);
            --color-err: #FCA5A5;
            --color-err-bg: rgba(239, 68, 68, 0.16);
            --color-err-border: rgba(239, 68, 68, 0.38);
            --shadow-sm: 0 2px 6px rgba(0, 0, 0, 0.35);
            --shadow-md: 0 14px 32px -10px rgba(0, 0, 0, 0.5);
            --shadow-hero: 0 24px 48px -12px rgba(0, 0, 0, 0.6);
        }
        * { box-sizing: border-box; }
        html { scroll-behavior: smooth; }
        body {
            margin: 0;
            background:
                radial-gradient(circle at 85%% 0%%, rgba(234, 88, 12, 0.08), transparent 44%%),
                radial-gradient(circle at 10%% 35%%, rgba(14, 165, 233, 0.06), transparent 42%%),
                var(--color-bg);
            color: var(--color-text);
            font-family: var(--font-sans);
            font-size: 16px;
            line-height: 1.6;
            -webkit-font-smoothing: antialiased;
        }
        a, button, input { font: inherit; }
        a:focus-visible, button:focus-visible, input:focus-visible {
            outline: 2px solid var(--color-primary);
            outline-offset: 2px;
        }
        .ico { width: 18px; height: 18px; display: inline-block; vertical-align: middle; flex-shrink: 0; }
        .container { max-width: 1200px; margin: 0 auto; padding: 0 18px; }
        .site-header {
            position: sticky; top: 0; z-index: 30;
            background: var(--color-header);
            backdrop-filter: blur(12px);
            -webkit-backdrop-filter: blur(12px);
            border-bottom: 1px solid var(--color-border);
        }
        .site-header__inner {
            display: flex; align-items: center; justify-content: space-between;
            gap: 12px; min-height: 70px; flex-wrap: wrap; padding: 10px 0;
        }
        .brand {
            display: inline-flex; align-items: center; gap: 11px;
            text-decoration: none; color: var(--color-text);
            font-family: var(--font-display); font-weight: 800; font-size: 1.12rem;
            letter-spacing: -0.02em;
        }
        .brand__logo {
            width: 40px; height: 40px; border-radius: 9999px; object-fit: cover; display: block;
            border: 2px solid var(--color-primary-bright);
            box-shadow: 0 4px 12px rgba(234, 88, 12, 0.22);
        }
        .nav { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
        .nav__link {
            color: var(--color-text-muted); text-decoration: none; font-size: 0.92rem; font-weight: 600;
            padding: 8px 10px; min-height: 44px; display: inline-flex; align-items: center;
            border-radius: var(--radius-sm);
            transition: color 160ms ease, background-color 160ms ease;
        }
        .nav__link:hover { color: var(--color-text); background: var(--color-inset); }
        .theme-toggle {
            min-height: 42px; padding: 8px 14px; border-radius: 9999px;
            display: inline-flex; align-items: center; gap: 6px;
            background: var(--color-surface); border: 1px solid var(--color-border-strong);
            color: var(--color-text); font-size: 0.84rem; font-weight: 700; cursor: pointer;
            transition: border-color 160ms ease;
        }
        .theme-toggle:hover { border-color: var(--color-primary); }
        .plat-dot { width: 8px; height: 8px; border-radius: 9999px; flex-shrink: 0; display: inline-block; }
        .platforms-strip { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 18px; }
        .plat-chip {
            display: inline-flex; align-items: center; gap: 6px;
            padding: 5px 11px; border-radius: 999px; font-size: 0.8rem; font-weight: 700;
            background: var(--color-surface); border: 1px solid var(--color-border);
            color: var(--color-text-secondary); box-shadow: var(--shadow-sm);
        }
        .btn {
            display: inline-flex; align-items: center; justify-content: center; gap: 8px;
            min-height: 44px; padding: 10px 18px; border-radius: var(--radius-md);
            border: 1px solid transparent; font-weight: 700; font-size: 0.92rem;
            text-decoration: none; cursor: pointer;
            transition: background-color 160ms ease, border-color 160ms ease, transform 160ms ease;
        }
        .btn.is-copied {
            border-color: var(--color-success); color: var(--color-success); background: var(--color-success-bg);
        }
        .btn--primary {
            background: var(--color-primary); color: var(--color-primary-contrast);
            border-color: var(--color-primary);
        }
        .btn--primary:hover { background: var(--color-primary-hover); border-color: var(--color-primary-hover); }
        .btn--secondary {
            background: var(--color-surface); color: var(--color-text);
            border-color: var(--color-border-strong);
        }
        .btn--secondary:hover { border-color: var(--color-primary); background: var(--color-inset); }
        .hero { padding: 34px 0 24px; }
        .hero-grid {
            display: grid;
            grid-template-columns: minmax(0, 1.25fr) minmax(280px, 0.85fr);
            gap: 28px;
            align-items: center;
            margin-bottom: 24px;
        }
        .hero__eyebrow {
            display: inline-flex; align-items: center; gap: 8px;
            padding: 6px 13px; border-radius: 999px;
            background: var(--color-success-bg); border: 1px solid var(--color-success-border);
            color: var(--color-success); font-size: 0.84rem; font-weight: 700;
        }
        .hero__title {
            font-family: var(--font-display);
            font-size: clamp(1.78rem, 3.6vw, 2.65rem);
            line-height: 1.18; letter-spacing: -0.025em;
            margin: 14px 0 12px; color: var(--color-text);
        }
        .hero__subtitle {
            color: var(--color-text-muted); max-width: 62ch; margin: 0 0 18px; font-size: 0.98rem;
        }
        .stats-bar {
            display: flex; flex-wrap: wrap; gap: 12px; margin-bottom: 20px;
        }
        .stat-chip {
            display: inline-flex; align-items: center; gap: 7px;
            padding: 6px 12px; border-radius: var(--radius-sm);
            background: var(--color-surface); border: 1px solid var(--color-border);
            color: var(--color-text-muted); font-size: 0.85rem; box-shadow: var(--shadow-sm);
        }
        .stat-chip strong { color: var(--color-text); font-family: var(--font-mono); }
        .hero-visual {
            display: flex; align-items: center; justify-content: center;
            perspective: 1100px;
        }
        .stage3d {
            position: relative; width: 100%%; max-width: 390px; aspect-ratio: 1 / 1;
            border-radius: 26px; padding: 14px;
            background: linear-gradient(145deg, var(--color-surface) 0%%, var(--color-surface-elevated) 100%%);
            border: 1px solid var(--color-border);
            box-shadow: var(--shadow-hero);
            transform-style: preserve-3d;
            transform: rotateX(var(--rx, 0deg)) rotateY(var(--ry, 0deg));
            transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
        }
        .stage3d::before {
            content: ""; position: absolute; inset: 0; border-radius: 26px; pointer-events: none;
            background: radial-gradient(circle at var(--gx, 50%%) var(--gy, 45%%), rgba(234, 88, 12, 0.16), transparent 62%%);
        }
        .stage3d__img {
            width: 100%%; height: 100%%; object-fit: cover; border-radius: 18px; display: block;
            box-shadow: 0 10px 24px rgba(15, 23, 42, 0.14);
        }
        .float-tag {
            position: absolute; display: inline-flex; align-items: center; gap: 7px;
            padding: 8px 12px; border-radius: 999px; font-size: 0.78rem; font-weight: 700;
            background: var(--color-surface); color: var(--color-text);
            border: 1px solid var(--color-border); box-shadow: var(--shadow-md);
            white-space: nowrap;
        }
        .float-tag--top { top: -10px; right: 14px; color: var(--color-primary); border-color: var(--color-primary-soft-border); }
        .float-tag--bot { bottom: -10px; left: 14px; color: var(--color-success); border-color: var(--color-success-border); }
        .convert-panel {
            background: var(--color-surface);
            border: 1px solid var(--color-border);
            border-top: 3px solid var(--color-primary-bright);
            border-radius: var(--radius-lg); padding: 18px; box-shadow: var(--shadow-md);
            margin-bottom: 16px;
        }
        .convert-panel__title {
            font-family: var(--font-display); font-size: 1.02rem; font-weight: 700;
            margin: 0 0 12px; color: var(--color-text);
            display: flex; align-items: center; gap: 8px;
        }
        .search-panel {
            background: var(--color-surface); border: 1px solid var(--color-border);
            border-radius: var(--radius-lg); padding: 16px; box-shadow: var(--shadow-sm);
        }
        .search-form { display: flex; flex-wrap: wrap; gap: 10px; align-items: stretch; }
        .sr-only {
            position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
            overflow: hidden; clip: rect(0, 0, 0, 0); border: 0;
        }
        .search-input {
            flex: 1 1 240px; min-height: 44px; padding: 10px 14px;
            border-radius: var(--radius-md); border: 1px solid var(--color-border-strong);
            background: var(--color-surface-elevated); color: var(--color-text);
        }
        .search-input::placeholder { color: var(--color-text-muted); opacity: 0.78; }
        .filter-row { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
        .filter-pill {
            min-height: 40px; padding: 6px 14px; border-radius: 999px;
            border: 1px solid var(--color-border); background: var(--color-surface-elevated);
            color: var(--color-text-muted); text-decoration: none; font-size: 0.88rem; font-weight: 600;
            display: inline-flex; align-items: center; gap: 7px;
            transition: border-color 150ms ease, background-color 150ms ease, color 150ms ease;
        }
        .filter-pill:hover { border-color: var(--color-primary); color: var(--color-text); }
        .filter-pill--active {
            background: var(--color-primary); color: var(--color-primary-contrast); border-color: var(--color-primary);
        }
        .convert-result {
            margin-top: 14px; background: var(--color-surface-elevated); border: 1px solid var(--color-border);
            border-radius: var(--radius-md); padding: 15px;
        }
        .convert-result__head { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 8px; }
        .convert-result__title { font-family: var(--font-display); font-size: 1rem; margin: 0 0 6px; color: var(--color-text); }
        .convert-result__meta { margin: 0 0 8px; color: var(--color-text-muted); font-size: 0.9rem; }
        .convert-result__url {
            background: var(--color-surface); border: 1px dashed var(--color-border-strong);
            padding: 9px 12px; border-radius: var(--radius-sm); word-break: break-all;
            font-size: 0.85rem; font-family: var(--font-mono); margin-bottom: 12px; color: var(--color-primary);
        }
        .convert-actions { display: flex; flex-wrap: wrap; gap: 10px; }
        .convert-alert {
            margin-top: 12px; padding: 12px 14px; border-radius: var(--radius-md); font-size: 0.92rem;
            display: flex; align-items: flex-start; gap: 10px;
        }
        .convert-alert--error {
            background: var(--color-err-bg); border: 1px solid var(--color-err-border); color: var(--color-err); font-weight: 600;
        }
        .section { padding: 26px 0; }
        .section__head {
            display: flex; flex-wrap: wrap; justify-content: space-between; align-items: baseline;
            gap: 12px; margin-bottom: 18px;
        }
        .section__title { font-family: var(--font-display); font-size: 1.4rem; margin: 0; color: var(--color-text); }
        .deals-grid {
            display: grid; gap: 16px; grid-template-columns: 1fr;
        }
        @media (min-width: 720px) { .deals-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
        @media (min-width: 1060px) { .deals-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
        .deal-card {
            display: flex; flex-direction: column; justify-content: space-between; gap: 12px;
            background: var(--color-surface); border: 1px solid var(--color-border);
            border-top: 3px solid var(--plat-accent, var(--color-primary-bright));
            border-radius: var(--radius-lg); padding: 18px; box-shadow: var(--shadow-sm);
            transition: border-color 160ms ease, box-shadow 160ms ease, transform 160ms ease;
        }
        .deal-card:hover { border-color: var(--color-primary-bright); box-shadow: var(--shadow-md); }
        .deal-card__top { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
        .badge {
            font-size: 0.78rem; font-weight: 700; padding: 4px 10px; border-radius: 999px;
        }
        .badge--platform {
            display: inline-flex; align-items: center; gap: 6px;
            background: var(--color-inset); color: var(--color-text-secondary); border: 1px solid var(--color-border);
        }
        .badge--accent { background: var(--color-primary-soft); color: var(--color-primary); border: 1px solid var(--color-primary-soft-border); }
        .deal-card__title { font-family: var(--font-display); font-size: 1.04rem; margin: 0; color: var(--color-text); }
        .deal-card__desc { margin: 0; color: var(--color-text-muted); font-size: 0.92rem; }
        .deal-card__meta { display: flex; flex-direction: column; gap: 6px; font-size: 0.85rem; }
        .coupon-row { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
        .copy-code-btn {
            min-height: 32px; padding: 4px 10px; border-radius: 8px;
            border: 1px solid var(--color-border-strong); background: var(--color-surface-elevated);
            color: var(--color-text); font-size: 0.78rem; font-weight: 700; cursor: pointer;
            display: inline-flex; align-items: center; gap: 5px;
            transition: border-color 150ms ease, background-color 150ms ease, color 150ms ease;
        }
        .copy-code-btn:hover { border-color: var(--color-primary); color: var(--color-primary); }
        .copy-code-btn.is-copied {
            border-color: var(--color-success); color: var(--color-success); background: var(--color-success-bg);
        }
        .coupon-pill code {
            background: var(--color-inset); border: 1px dashed var(--color-primary);
            padding: 2px 8px; border-radius: 6px; color: var(--color-primary);
            font-family: var(--font-mono); font-weight: 700;
        }
        .coupon-pill--link {
            display: inline-flex; align-items: center; gap: 6px;
            padding: 4px 10px; border-radius: 8px;
            background: var(--color-success-bg); border: 1px solid var(--color-success-border);
            color: var(--color-success); font-size: 0.78rem; font-weight: 700;
        }
        .deal-card__note { color: var(--color-text-muted); font-size: 0.84rem; }
        .deal-card__cta { width: 100%%; }
        .steps-grid {
            display: grid; gap: 16px; grid-template-columns: 1fr;
        }
        @media (min-width: 768px) { .steps-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
        .step-card {
            background: var(--color-surface); border: 1px solid var(--color-border);
            border-radius: var(--radius-lg); padding: 20px; box-shadow: var(--shadow-sm);
            display: flex; flex-direction: column; gap: 10px;
        }
        .step-num {
            width: 36px; height: 36px; border-radius: 10px;
            display: inline-flex; align-items: center; justify-content: center;
            background: var(--color-primary-soft); border: 1px solid var(--color-primary-soft-border);
            color: var(--color-primary); font-family: var(--font-mono); font-weight: 800; font-size: 0.92rem;
        }
        .step-card h3 { margin: 0; font-family: var(--font-display); font-size: 1.04rem; color: var(--color-text); }
        .step-card p { margin: 0; color: var(--color-text-muted); font-size: 0.92rem; }
        .empty-state {
            grid-column: 1 / -1; background: var(--color-surface); border: 1px solid var(--color-border);
            border-radius: var(--radius-lg); padding: 28px; text-align: center; color: var(--color-text-muted);
        }
        .site-footer {
            border-top: 1px solid var(--color-border); margin-top: 32px; padding: 28px 0 40px;
            background: var(--color-surface); color: var(--color-text-muted); font-size: 0.88rem;
        }
        .site-footer__links { display: flex; flex-wrap: wrap; gap: 16px; margin-top: 10px; }
        .site-footer a { color: var(--color-primary); font-weight: 700; text-decoration: none; }
        .site-footer a:hover { text-decoration: underline; }
        @media (max-width: 900px) {
            .hero-grid { grid-template-columns: minmax(0, 1fr); }
            .stage3d { max-width: 320px; }
        }
        @media (prefers-reduced-motion: no-preference) {
            .deal-card:hover { transform: translateY(-2px); }
            .btn--primary:hover { transform: translateY(-1px); }
        }
        @media (prefers-reduced-motion: reduce) {
            *, *::before, *::after { transition: none !important; scroll-behavior: auto !important; transform: none !important; }
        }
    </style>
</head>
<body>
    %s
    <header class="site-header">
        <div class="container site-header__inner">
            <a class="brand" href="/">
                <img src="/assets/bot-logo.webp" alt="Logo Thế Giới Siêu Sale" class="brand__logo">
                <span>Thế Giới Siêu Sale</span>
            </a>
            <nav class="nav" aria-label="Điều hướng chính">
                <a class="nav__link" href="#uu-dai">Ưu đãi nổi bật</a>
                <a class="nav__link" href="#huong-dan">Cách nhận mã</a>
                <a class="nav__link" href="#chinh-sach">Minh bạch đối tác</a>
                <button type="button" id="public-theme-btn" class="theme-toggle" aria-label="Chuyển đổi giao diện Sáng/Tối">
                    <svg class="ico" style="width:15px;height:15px;" aria-hidden="true"><use id="public-theme-ico-use" href="#ico-moon"/></svg>
                    <span id="public-theme-text">Giao diện Tối</span>
                </button>
                <a class="btn btn--primary" href="https://t.me/%s" target="_blank" rel="noopener noreferrer">Mở Trợ Lý @%s</a>
            </nav>
        </div>
    </header>

    <main class="container">
        <section class="hero">
            <div class="hero-grid">
                <div>
                    <span class="hero__eyebrow">
                        <svg class="ico" aria-hidden="true"><use href="#ico-shield"/></svg>
                        <span>Cập nhật tự động từ đối tác chính hãng</span>
                    </span>
                    <h1 class="hero__title">Tổng hợp mã giảm giá &amp; gợi ý sản phẩm tiết kiệm mỗi ngày</h1>
                    <p class="hero__subtitle">
                        Thế Giới Siêu Sale tổng hợp các chương trình ưu đãi, mã giảm giá công khai theo từng gian hàng và ngành hàng từ Shopee, Tiki, Lazada, TikTok Shop và Alibaba giúp người mua dễ dàng so sánh và đặt hàng trực tiếp trên sàn.
                    </p>
                    <div class="stats-bar">
                        <span class="stat-chip">
                            <svg class="ico" aria-hidden="true"><use href="#ico-ticket"/></svg>
                            <span>Đang hiển thị: <strong>%d</strong> / <strong>%d</strong> ưu đãi</span>
                        </span>
                        <span class="stat-chip">
                            <svg class="ico" aria-hidden="true"><use href="#ico-clock"/></svg>
                            <span>Đồng bộ gần nhất: <strong>%s</strong></span>
                        </span>
                    </div>
                    <div class="platforms-strip" aria-label="Các sàn TMĐT hỗ trợ">
                        <span class="plat-chip"><span class="plat-dot" style="background:#EA580C;" aria-hidden="true"></span>Shopee</span>
                        <span class="plat-chip"><span class="plat-dot" style="background:#0284C7;" aria-hidden="true"></span>Tiki</span>
                        <span class="plat-chip"><span class="plat-dot" style="background:#DB2777;" aria-hidden="true"></span>Lazada</span>
                        <span class="plat-chip"><span class="plat-dot" style="background:#E11D48;" aria-hidden="true"></span>TikTok Shop</span>
                        <span class="plat-chip"><span class="plat-dot" style="background:#D97706;" aria-hidden="true"></span>Alibaba</span>
                    </div>
                </div>

                <div class="hero-visual">
                    <div id="stage3d" class="stage3d">
                        <img src="/assets/bot-cart-3d.webp" alt="Trợ lý 3D Thế Giới Siêu Sale săn mã giảm giá" class="stage3d__img">
                        <div class="float-tag float-tag--top" aria-hidden="true">
                            <svg class="ico" aria-hidden="true"><use href="#ico-zap"/></svg>
                            <span>Săn mã giảm đến 50%%</span>
                        </div>
                        <div class="float-tag float-tag--bot" aria-hidden="true">
                            <svg class="ico" aria-hidden="true"><use href="#ico-check"/></svg>
                            <span>Kiểm tra link sống 100%%</span>
                        </div>
                    </div>
                </div>
            </div>

            <div class="convert-panel">
                <p class="convert-panel__title">
                    <svg class="ico" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>
                    <span>Dán link sản phẩm để lấy link ưu đãi &amp; mã khớp ngay (không cần cài App)</span>
                </p>
                <form class="search-form" method="POST" action="/convert">
                    <label class="sr-only" for="web-convert-url">Dán đường link sản phẩm cần săn mã</label>
                    <input id="web-convert-url" class="search-input" type="url" name="url" required value="%s" placeholder="Dán link sản phẩm Shopee, Tiki, Lazada, TikTok Shop hoặc Alibaba vào đây...">
                    <button class="btn btn--primary" type="submit">
                        <svg class="ico" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>
                        <span>Lấy Link Ưu Đãi</span>
                    </button>
                </form>
                %s
            </div>

            <div class="search-panel">
                <form class="search-form" method="GET" action="/">
                    <label class="sr-only" for="deal-search">Tìm mã giảm giá hoặc tên gian hàng</label>
                    <input id="deal-search" class="search-input" type="text" name="q" value="%s" placeholder="Tìm tên gian hàng, ngành hàng hoặc mã ưu đãi (VD: Tiki Trading, Gia dụng...)">
                    <input type="hidden" name="plat" value="%s">
                    <button class="btn btn--primary" type="submit">
                        <svg class="ico" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
                        <span>Tìm ưu đãi</span>
                    </button>
                </form>
                <div class="filter-row" aria-label="Lọc theo sàn thương mại điện tử">
                    %s
                </div>
            </div>
        </section>

        <section id="uu-dai" class="section">
            <div class="section__head">
                <h2 class="section__title">Danh sách ưu đãi &amp; mã giảm giá mới nhất</h2>
                <span class="deal-card__note">Chuyển tiếp trực tiếp sang trang chính thức của sàn</span>
            </div>
            <div class="deals-grid">
                %s
            </div>
        </section>

        <section id="huong-dan" class="section">
            <div class="section__head">
                <h2 class="section__title">Quy trình sử dụng nhanh chóng &amp; hợp lệ</h2>
            </div>
            <div class="steps-grid">
                <article class="step-card">
                    <span class="step-num" aria-hidden="true">01</span>
                    <h3>Chọn ưu đãi hoặc gửi link sản phẩm</h3>
                    <p>Tìm mã theo ngành hàng ngay trên website hoặc dán liên kết sản phẩm trực tiếp cho trợ lý Telegram @%s.</p>
                </article>
                <article class="step-card">
                    <span class="step-num" aria-hidden="true">02</span>
                    <h3>Lưu mã tại trang gian hàng chính thức</h3>
                    <p>Bấm nút mở gian hàng để truy cập trang khuyến mãi hợp lệ trên Shopee, Tiki, Lazada hoặc TikTok Shop.</p>
                </article>
                <article class="step-card">
                    <span class="step-num" aria-hidden="true">03</span>
                    <h3>Thanh toán trực tiếp trên sàn</h3>
                    <p>Người dùng chủ động chọn mã ở bước thanh toán trên ứng dụng thương mại điện tử để hưởng mức giá tốt nhất.</p>
                </article>
            </div>
        </section>
    </main>

    <footer id="chinh-sach" class="site-footer">
        <div class="container">
            <p><strong>Tuyên bố minh bạch tiếp thị liên kết (Affiliate Disclosure):</strong> Thế Giới Siêu Sale là website tổng hợp thông tin khuyến mãi độc lập, hoạt động theo quy chuẩn đối tác tiếp thị liên kết chính thức. Khi người dùng bấm vào liên kết ưu đãi và hoàn tất mua sắm tự nhiên trên sàn thương mại điện tử, nền tảng có thể nhận hoa hồng giới thiệu mà không làm tăng giá sản phẩm của người mua.</p>
            <div class="site-footer__links">
                <span>Thế Giới Siêu Sale © 2026</span>
                <a href="https://t.me/%s" target="_blank" rel="noopener noreferrer">Kênh hỗ trợ Telegram</a>
            </div>
        </div>
    </footer>
    <script src="/assets/home.js" defer></script>
    <script src="/assets/landing.js" defer></script>
</body>
</html>`,
		publicSVGSprite,
		utils.EscapeHTML(state.Config.BotUsername),
		utils.EscapeHTML(state.Config.BotUsername),
		matchedCount,
		totalAvailable,
		utils.EscapeHTML(lastSyncText),
		utils.EscapeHTML(convertInputVal),
		convertResultHTML,
		utils.EscapeHTML(searchQ),
		utils.EscapeHTML(filterPlat),
		filterLinksBuilder.String(),
		cardsHTML,
		utils.EscapeHTML(state.Config.BotUsername),
		utils.EscapeHTML(state.Config.BotUsername),
	)

	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte(html))
}
