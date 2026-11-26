package social

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"

	"salehunter-go/internal/config"
	"salehunter-go/internal/db"
	"salehunter-go/internal/services/shopee"
	"salehunter-go/internal/utils"
)

const (
	SocialSyncIntervalSecs = 300
	MobileUA               = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1"
)

var DefaultChannels = []string{
	"https://t.me/s/magiamgiashopee",
	"https://t.me/s/meosansale",
	"https://t.me/s/dealngonmoingay",
	"https://t.me/s/xomsansale",
}

var DefaultWebDealPages = []string{
	"https://magiamgiashopee.vn",
	"https://bloggiamgia.vn/",
}

// SocialHarvesterStatus trạng thái thu thập chữ ký & deal FB/IG nền.
type SocialHarvesterStatus struct {
	IsRunning       bool
	IsSyncing       bool
	SyncCycles      uint64
	IntervalSecs    uint64
	LastSyncVN      string
	HarvestedParams *string
	SigStrength     uint8
	DealsFound      int
}

// SocialSyncReport kết quả mỗi chu kỳ chạy Social Harvester.
type SocialSyncReport struct {
	HarvestedParams *string
	DealsFound      int
	DealsInserted   int
	DealsUpdated    int
}

var (
	statusMu     sync.Mutex
	socialStatus = SocialHarvesterStatus{IntervalSecs: SocialSyncIntervalSecs}

	reHTTPUrlAttr1 = regexp.MustCompile(`httpUrl\s*:\s*"([^"]+)"`)
	reHTTPUrlAttr2 = regexp.MustCompile(`window\.location\.(?:replace|assign)\(\s*["']([^"']+)["']\s*\)`)
	reHTTPUrlMeta  = regexp.MustCompile(`(?is)<meta[^>]+http-equiv=["']refresh["'][^>]+content=["'][^"']*?url=([^"'\s>]+)["']`)

	reWS        = regexp.MustCompile(`\s+`)
	reAnyHTTP   = regexp.MustCompile(`https?://[^\s<>"']+`)
	reNoiseURLs = regexp.MustCompile(`(?i)(?:https?://\S+|(?:s\.shopee\.vn|shp\.ee|vn\.shp\.ee|nghien\.co|noti\.sale|sandeal\.co|go\.isclix\.com|bit\.ly)/\S+)`)
	reNoiseTxt  = regexp.MustCompile(`(?i)(?:►+|list\s*áp\s*dụng\s*[>:]*|áp\s*(?:cho\s*)?list\s*[>:]*|lưu\s*banner\s*|\[c\s+link=[^\]]*\]?)`)
	reHTMLTag   = regexp.MustCompile(`<[^>]+>`)
	reVNDHint   = regexp.MustCompile(`(?i)(?:đơn\s*từ|từ)\s*(\d+(?:[.,]\d+)?)\s*(k|tr|triệu|đ|d)?`)
	rePTag      = regexp.MustCompile(`(?is)<p\b[^>]*>(.*?)</p>`)
	reHrefAttr  = regexp.MustCompile(`(?i)href="([^"]+)"`)
	rePctHint   = regexp.MustCompile(`(\d{1,2})\s*%`)
	reShortLink = regexp.MustCompile(`(?i)https?://(?:s\.shopee\.vn|shp\.ee|vn\.shp\.ee|sandeal\.co)/[^\s"'<>)\]]+`)
	reBareShort = regexp.MustCompile(`(?i)\bs\.shopee\.vn/[A-Za-z0-9_-]{5,}`)
	reBeforePag = regexp.MustCompile(`href="/s/magiamgiashopee\?before=(\d+)"`)
)

// SnapshotSocialStatus trả về bản sao trạng thái harvester hiện tại.
func SnapshotSocialStatus() SocialHarvesterStatus {
	statusMu.Lock()
	defer statusMu.Unlock()
	cp := socialStatus
	if socialStatus.HarvestedParams != nil {
		v := *socialStatus.HarvestedParams
		cp.HarvestedParams = &v
	}
	return cp
}

// DecodeEmbeddedHTMLEscapes giải mã escape JSON/HTML nhúng trong trang.
func DecodeEmbeddedHTMLEscapes(raw string) string {
	r := strings.NewReplacer(
		`\u003c`, `<`,
		`\u003C`, `<`,
		`\u003e`, `>`,
		`\u003E`, `>`,
		`\u0026`, `&`,
		`\u003d`, `=`,
		`\u003D`, `=`,
		`\/`, `/`,
	)
	return r.Replace(raw)
}

// ExtractShopeeConfigHTTPURL trích URL điều hướng ẩn trong trang trung gian Shopee/Facebook Reels.
func ExtractShopeeConfigHTTPURL(html string) (string, bool) {
	for _, re := range []*regexp.Regexp{reHTTPUrlAttr1, reHTTPUrlAttr2, reHTTPUrlMeta} {
		if m := re.FindStringSubmatch(html); len(m) > 1 {
			decoded := strings.ReplaceAll(DecodeEmbeddedHTMLEscapes(strings.TrimSpace(m[1])), "&amp;", "&")
			if strings.HasPrefix(decoded, "https://") || strings.HasPrefix(decoded, "http://") {
				return decoded, true
			}
		}
	}
	return "", false
}

// UpgradeShopeeShortURL nâng cấp HTTP sang HTTPS cho các short domain Shopee.
func UpgradeShopeeShortURL(raw string) string {
	t := strings.ReplaceAll(strings.TrimSpace(raw), "&amp;", "&")
	low := strings.ToLower(t)
	if strings.HasPrefix(low, "http://s.shopee.vn/") ||
		strings.HasPrefix(low, "http://shp.ee/") ||
		strings.HasPrefix(low, "http://vn.shp.ee/") {
		return "https://" + t[len("http://"):]
	}
	return t
}

// IsGuideNoiseDealText kiểm tra bài viết hướng dẫn rác không phải voucher thật.
func IsGuideNoiseDealText(text string) bool {
	low := strings.ToLower(text)
	for _, kw := range []string{
		"hướng dẫn lấy mã",
		"hướng dẫn săn mã",
		"hướng dẫn áp mã",
		"khi ấn link lấy mã",
		"nhớ vào live",
		"sếp nên lựa",
		"cách lấy mã",
	} {
		if strings.Contains(low, kw) {
			return true
		}
	}
	return false
}

// IsSyntheticTrackingCode kiểm tra mã nội bộ sinh tự động không phải mã gõ tay.
func IsSyntheticTrackingCode(code string) bool {
	up := strings.ToUpper(strings.TrimSpace(code))
	return up == "" || strings.HasPrefix(up, "KOL_") || strings.HasPrefix(up, "SHP_AUTO_") || strings.HasPrefix(up, "AUTO_")
}

// ProbeURLForSocialCandidates theo vết redirect tối đa maxHops để trích tham số phiên FB/IG Reels.
func ProbeURLForSocialCandidates(client *http.Client, rawURL string, maxHops uint8) []string {
	var out []string
	currentURL := UpgradeShopeeShortURL(rawURL)

	for hop := uint8(0); hop < maxHops; hop++ {
		req, err := http.NewRequest(http.MethodGet, currentURL, nil)
		if err != nil {
			break
		}
		req.Header.Set("User-Agent", MobileUA)
		resp, err := client.Do(req)
		if err != nil {
			break
		}
		finalURL := currentURL
		if resp.Request != nil && resp.Request.URL != nil {
			finalURL = resp.Request.URL.String()
		}
		bodyBytes, readErr := io.ReadAll(io.LimitReader(resp.Body, 1024*1024))
		_ = resp.Body.Close()

		out = append(out, finalURL)
		if shopee.SocialParamStrength(finalURL) >= 3 {
			break
		}
		if readErr != nil {
			break
		}
		body := string(bodyBytes)
		if cfgURL, ok := ExtractShopeeConfigHTTPURL(body); ok {
			upgraded := UpgradeShopeeShortURL(cfgURL)
			out = append(out, upgraded)
			if shopee.SocialParamStrength(upgraded) >= 3 {
				break
			}
			lowUp := strings.ToLower(upgraded)
			if upgraded != currentURL && (strings.Contains(lowUp, "s.shopee.vn/") || strings.Contains(lowUp, "shp.ee/")) {
				currentURL = upgraded
				continue
			}
		}
		out = append(out, body)
		break
	}
	return out
}

// ConsiderBestParams cập nhật chữ ký tốt nhất nếu ứng viên mới có độ mạnh cao hơn (hoặc bằng khi chưa có).
func ConsiderBestParams(best **string, cand string) {
	if extracted, ok := shopee.ExtractSocialPassthroughParams(cand); ok {
		candScore := shopee.SocialParamStrength(extracted)
		curScore := uint8(0)
		if *best != nil {
			curScore = shopee.SocialParamStrength(**best)
		}
		if candScore > curScore || (candScore == curScore && *best == nil) {
			cp := extracted
			*best = &cp
		}
	}
}

// ResolveSocialParamsFromInput trích xuất tham số phiên FB/IG từ chuỗi hoặc URL do Admin gửi.
func ResolveSocialParamsFromInput(client *http.Client, rawInput string) *string {
	trimmed := strings.TrimSpace(rawInput)
	if direct, ok := shopee.ExtractSocialPassthroughParams(rawInput); ok {
		if shopee.SocialParamStrength(direct) >= 3 || !strings.HasPrefix(trimmed, "http") {
			return &direct
		}
	}
	var best *string
	if m := reAnyHTTP.FindString(trimmed); m != "" {
		for _, cand := range ProbeURLForSocialCandidates(client, m, 2) {
			ConsiderBestParams(&best, cand)
			if best != nil && shopee.SocialParamStrength(*best) >= 3 {
				return best
			}
		}
	}
	if best == nil {
		if direct, ok := shopee.ExtractSocialPassthroughParams(rawInput); ok {
			return &direct
		}
	}
	return best
}

// CleanHarvestedDesc làm sạch mô tả deal vừa quét và cắt tối đa 90 Unicode runes.
func CleanHarvestedDesc(text string) string {
	noURL := reNoiseURLs.ReplaceAllString(text, " ")
	noNoise := reNoiseTxt.ReplaceAllString(noURL, " ")
	collapsed := reWS.ReplaceAllString(noNoise, " ")
	cleaned := strings.Trim(collapsed, ">=:.- ")
	return utils.TakeRunes(cleaned, 90)
}

// StripHTMLEntitiesAndTags gỡ tag HTML và entity cơ bản.
func StripHTMLEntitiesAndTags(raw string) string {
	noTags := reHTMLTag.ReplaceAllString(raw, " ")
	r := strings.NewReplacer(
		"&amp;", "&",
		"&nbsp;", " ",
		"&#8211;", "-",
		"&#8212;", "-",
		"&gt;", ">",
		"&lt;", "<",
	)
	decoded := r.Replace(noTags)
	return strings.TrimSpace(reWS.ReplaceAllString(decoded, " "))
}

// ParseVNDAmountHint trích giá trị đơn tối thiểu VNĐ từ văn bản deal.
func ParseVNDAmountHint(text string) float64 {
	low := strings.ToLower(text)
	if strings.Contains(low, "đơn 0đ") || strings.Contains(low, "từ 0đ") {
		return 0.0
	}
	m := reVNDHint.FindStringSubmatch(text)
	if len(m) < 2 {
		return 0.0
	}
	numStr := strings.ReplaceAll(m[1], ",", ".")
	val, err := strconv.ParseFloat(numStr, 64)
	if err != nil {
		return 0.0
	}
	unit := ""
	if len(m) >= 3 {
		unit = strings.ToLower(m[2])
	}
	switch unit {
	case "tr", "triệu":
		val *= 1_000_000.0
	case "k":
		val *= 1_000.0
	}
	if val < 0.0 {
		return 0.0
	}
	return val
}

func stableDealCodeSuffix(prefix, seed string) string {
	if u, err := url.Parse(seed); err == nil {
		if pid := strings.TrimSpace(u.Query().Get("promotionId")); pid != "" {
			runes := []rune(pid)
			if len(runes) > 10 {
				runes = runes[len(runes)-10:]
			}
			return prefix + "_" + string(runes)
		}
	}
	h := uint32(2166136261)
	for i := 0; i < len(seed); i++ {
		h ^= uint32(seed[i])
		h *= 16777619
	}
	return fmt.Sprintf("%s_%08X", prefix, h)
}

// ParseMaGiamGiaShopeeDeals trích danh sách deal KOL IG/FB và Shopee từ HTML trang công khai.
func ParseMaGiamGiaShopeeDeals(rawHTML string) []db.NewDealInput {
	html := DecodeEmbeddedHTMLEscapes(rawHTML)
	var (
		kolSocialDeals  []db.NewDealInput
		topVoucherDeals []db.NewDealInput
		seenCodes       = make(map[string]struct{})
		sectionURL      *string
	)

	matches := rePTag.FindAllStringSubmatch(html, -1)
	for _, m := range matches {
		if len(m) < 2 {
			continue
		}
		blockHTML := m[1]
		text := StripHTMLEntitiesAndTags(blockHTML)
		if text == "" {
			continue
		}
		low := strings.ToLower(text)

		var inlineCleanURL *string
		for _, hm := range reHrefAttr.FindAllStringSubmatch(blockHTML, -1) {
			if len(hm) < 2 {
				continue
			}
			href := strings.ReplaceAll(strings.TrimSpace(hm[1]), "&amp;", "&")
			if target, ok := shopee.ExtractOriginLinkFromAnRedir(href); ok && strings.Contains(target, "shopee.vn") {
				cp := target
				inlineCleanURL = &cp
				break
			}
		}

		if inlineCleanURL != nil {
			for _, secKW := range []string{
				"mã shopee kol",
				"mã link kol",
				"mã lưu từ banner",
				"ưu đãi shopeevip",
				"mã hot sẵn ví",
				"mã bách hoá",
				"mã shopee video",
			} {
				if strings.Contains(low, secKW) {
					cp := *inlineCleanURL
					sectionURL = &cp
					break
				}
			}
		}

		isIG := strings.Contains(low, "mã instagram") || strings.Contains(low, "instagram")
		isFB := strings.Contains(low, "mã facebook") || strings.Contains(low, "shopee x facebook")
		pctVal := uint32(0)
		if pm := rePctHint.FindStringSubmatch(low); len(pm) > 1 {
			if v, err := strconv.ParseUint(pm[1], 10, 32); err == nil {
				pctVal = uint32(v)
			}
		}

		isExplicitGuide := strings.Contains(low, "hướng dẫn lấy mã") ||
			strings.Contains(low, "hướng dẫn săn mã") ||
			strings.Contains(low, "hướng dẫn áp mã") ||
			strings.Contains(low, "khi ấn link lấy mã") ||
			strings.Contains(low, "cách lấy mã")
		hasDirectQuery := inlineCleanURL != nil &&
			(strings.Contains(*inlineCleanURL, "shopee.vn/search?") ||
				strings.Contains(*inlineCleanURL, "evcode=") ||
				strings.Contains(*inlineCleanURL, "promotionId="))
		if isExplicitGuide || (IsGuideNoiseDealText(text) && !hasDirectQuery) {
			continue
		}

		if (isIG || isFB) && pctVal >= 15 {
			var dealURL string
			if inlineCleanURL != nil {
				dealURL = *inlineCleanURL
			} else if sectionURL != nil {
				dealURL = *sectionURL
			} else {
				continue
			}
			var code, title string
			if isIG {
				code = fmt.Sprintf("KOL_IG_%d", pctVal)
				title = "[Toàn sàn] Mã Instagram KOL · Back 9H·15H·20H"
			} else {
				code = fmt.Sprintf("KOL_FB_%d", pctVal)
				title = "[Toàn sàn] Mã Facebook KOL · Back 9H·15H·20H"
			}
			if _, dup := seenCodes[code]; !dup {
				seenCodes[code] = struct{}{}
				codeCopy := code
				kolSocialDeals = append(kolSocialDeals, db.NewDealInput{
					Platform:     "Shopee",
					Title:        title,
					CouponCode:   &codeCopy,
					DiscountDesc: CleanHarvestedDesc(text),
					MinSpend:     ParseVNDAmountHint(text),
					DealURL:      dealURL,
					ExpireAt:     nil,
					IsExclusive:  true,
				})
			}
			continue
		}

		if pctVal >= 20 && len(topVoucherDeals) < 8 && inlineCleanURL != nil {
			targetURL := *inlineCleanURL
			if strings.Contains(targetURL, "shopee.vn/search?") || strings.Contains(targetURL, "shopee.vn/m/") {
				code := stableDealCodeSuffix("SHP_AUTO", targetURL)
				if _, dup := seenCodes[code]; !dup {
					seenCodes[code] = struct{}{}
					codeCopy := code
					topVoucherDeals = append(topVoucherDeals, db.NewDealInput{
						Platform:     "Shopee",
						Title:        fmt.Sprintf("[Toàn sàn] Voucher Shopee %d%%", pctVal),
						CouponCode:   &codeCopy,
						DiscountDesc: CleanHarvestedDesc(text),
						MinSpend:     ParseVNDAmountHint(text),
						DealURL:      targetURL,
						ExpireAt:     nil,
						IsExclusive:  true,
					})
				}
			}
		}
	}

	combined := make([]db.NewDealInput, 0, len(kolSocialDeals)+len(topVoucherDeals))
	combined = append(combined, kolSocialDeals...)
	combined = append(combined, topVoucherDeals...)
	if len(combined) > 12 {
		combined = combined[:12]
	}
	return combined
}

// ChooseEffectiveSocialParams chọn tham số tốt nhất giữa bản mới quét và bản đang lưu.
func ChooseEffectiveSocialParams(scannedBest *string, currentSaved string) *string {
	var savedValid *string
	if ext, ok := shopee.ExtractSocialPassthroughParams(currentSaved); ok {
		savedValid = &ext
	}
	scannedScore := uint8(0)
	if scannedBest != nil {
		scannedScore = shopee.SocialParamStrength(*scannedBest)
	}
	savedScore := uint8(0)
	if savedValid != nil {
		savedScore = shopee.SocialParamStrength(*savedValid)
	}
	if scannedScore >= 3 {
		return scannedBest
	}
	if savedScore >= 3 {
		return savedValid
	}
	if scannedBest != nil {
		return scannedBest
	}
	return savedValid
}

// CollectShortlinksFromHTML thu thập shortlinks tiềm năng và trích sẵn signature nếu decode được an_redir.
func CollectShortlinksFromHTML(html string, newestFirst bool, seenURLs map[string]struct{}, outLinks *[]string, scannedBest **string, maxTotal int) {
	var extracted []string
	for _, m := range reShortLink.FindAllString(html, -1) {
		up := UpgradeShopeeShortURL(m)
		if target, ok := shopee.ExtractOriginLinkFromAnRedir(up); ok {
			ConsiderBestParams(scannedBest, target)
			continue
		}
		extracted = append(extracted, up)
	}
	for _, m := range reBareShort.FindAllString(html, -1) {
		extracted = append(extracted, UpgradeShopeeShortURL("https://"+m))
	}
	if newestFirst {
		for i, j := 0, len(extracted)-1; i < j; i, j = i+1, j-1 {
			extracted[i], extracted[j] = extracted[j], extracted[i]
		}
	}
	for _, link := range extracted {
		if len(*outLinks) >= maxTotal {
			break
		}
		if _, dup := seenURLs[link]; !dup {
			seenURLs[link] = struct{}{}
			ConsiderBestParams(scannedBest, link)
			*outLinks = append(*outLinks, link)
		}
	}
}

func currentVNTimestampStr() string {
	loc := time.FixedZone("ICT", 7*3600)
	return time.Now().In(loc).Format("15:04:05 02/01/2006")
}

// SyncSocialAllOnce chạy 1 chu kỳ thu thập chữ ký FB/IG Reels và mã Shopee công khai.
func SyncSocialAllOnce(client *http.Client, appDB *db.AppDb, cfg config.Config) SocialSyncReport {
	return SyncSocialAllOnceWithContext(context.Background(), client, appDB, cfg)
}

// SyncSocialAllOnceWithContext chạy 1 chu kỳ thu thập có hỗ trợ hủy tức thì qua context khi Graceful Shutdown.
func SyncSocialAllOnceWithContext(ctx context.Context, client *http.Client, appDB *db.AppDb, cfg config.Config) SocialSyncReport {
	var report SocialSyncReport
	if ctx.Err() != nil || appDB == nil || appDB.DB == nil {
		return report
	}

	currentSaved := ""
	if v, err := db.GetSetting(appDB.DB, "shopee_social_params"); err == nil && v != nil {
		currentSaved = *v
	}

	var (
		scannedBest    *string
		candidateLinks []string
		seenURLs       = make(map[string]struct{})
	)

	// 1. Quét Telegram public channel HTML
	for _, channelURL := range DefaultChannels {
		if ctx.Err() != nil {
			return report
		}
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, channelURL, nil)
		if err != nil {
			continue
		}
		req.Header.Set("User-Agent", MobileUA)
		resp, err := client.Do(req)
		if err != nil {
			continue
		}
		bodyBytes, readErr := io.ReadAll(io.LimitReader(resp.Body, 2*1024*1024))
		_ = resp.Body.Close()
		if resp.StatusCode < 200 || resp.StatusCode >= 300 || readErr != nil {
			continue
		}
		html := string(bodyBytes)
		CollectShortlinksFromHTML(html, true, seenURLs, &candidateLinks, &scannedBest, 70)

		if strings.HasSuffix(channelURL, "/magiamgiashopee") && len(candidateLinks) < 70 && ctx.Err() == nil {
			if bm := reBeforePag.FindStringSubmatch(html); len(bm) > 1 {
				prevURL := channelURL + "?before=" + bm[1]
				if pReq, pErr := http.NewRequestWithContext(ctx, http.MethodGet, prevURL, nil); pErr == nil {
					pReq.Header.Set("User-Agent", MobileUA)
					if pResp, pDoErr := client.Do(pReq); pDoErr == nil {
						pBytes, pReadErr := io.ReadAll(io.LimitReader(pResp.Body, 2*1024*1024))
						_ = pResp.Body.Close()
						if pResp.StatusCode >= 200 && pResp.StatusCode < 300 && pReadErr == nil {
							CollectShortlinksFromHTML(string(pBytes), true, seenURLs, &candidateLinks, &scannedBest, 70)
						}
					}
				}
			}
		}

		if len(candidateLinks) >= 70 {
			break
		}
	}

	// 2. Quét trang deal công khai
	var harvestedDeals []db.NewDealInput
	seenDealCodes := make(map[string]struct{})
	for _, pageURL := range DefaultWebDealPages {
		if ctx.Err() != nil {
			return report
		}
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, pageURL, nil)
		if err != nil {
			continue
		}
		req.Header.Set("User-Agent", MobileUA)
		resp, err := client.Do(req)
		if err != nil {
			slog.Warn("[SOCIAL_HARVESTER] Không tải được nguồn deal", "url", utils.SanitizeCRLF(pageURL), "err", err)
			continue
		}
		bodyBytes, readErr := io.ReadAll(io.LimitReader(resp.Body, 2*1024*1024))
		_ = resp.Body.Close()
		if resp.StatusCode < 200 || resp.StatusCode >= 300 || readErr != nil {
			slog.Warn("[SOCIAL_HARVESTER] Nguồn trả HTTP lỗi", "url", utils.SanitizeCRLF(pageURL), "status", resp.StatusCode)
			continue
		}
		pageHTML := string(bodyBytes)
		CollectShortlinksFromHTML(pageHTML, false, seenURLs, &candidateLinks, &scannedBest, 80)
		for _, deal := range ParseMaGiamGiaShopeeDeals(pageHTML) {
			key := deal.Title
			if deal.CouponCode != nil {
				key = *deal.CouponCode
			}
			if _, dup := seenDealCodes[key]; !dup {
				seenDealCodes[key] = struct{}{}
				harvestedDeals = append(harvestedDeals, deal)
			}
		}
	}

	// 3. Truy vấn song song theo đợt 10 link để giải mã chữ ký mạnh nhất
	if scannedBest == nil || shopee.SocialParamStrength(*scannedBest) < 3 {
		const chunkSize = 10
		for start := 0; start < len(candidateLinks); start += chunkSize {
			end := start + chunkSize
			if end > len(candidateLinks) {
				end = len(candidateLinks)
			}
			chunk := candidateLinks[start:end]
			results := make([][]string, len(chunk))
			var wg sync.WaitGroup
			for i, link := range chunk {
				if ctx.Err() != nil {
					break
				}
				idx := i
				target := link
				utils.SafeGo(&wg, "social.probe_worker", func() {
					results[idx] = ProbeURLForSocialCandidates(client, target, 3)
				})
			}
			wg.Wait()
			for _, cands := range results {
				for _, c := range cands {
					ConsiderBestParams(&scannedBest, c)
				}
			}
			if scannedBest != nil && shopee.SocialParamStrength(*scannedBest) >= 3 {
				break
			}
		}
	}

	effective := ChooseEffectiveSocialParams(scannedBest, currentSaved)
	if effective != nil {
		if *effective != currentSaved {
			_ = db.SetSetting(appDB.DB, "shopee_social_params", *effective)
			appDB.InvalidateSettingsCache()
			slog.Info("🎉 [SOCIAL_HARVESTER] Đã tự động cập nhật chữ ký Shopee FB/IG",
				"strength", shopee.SocialParamStrength(*effective),
			)
			notifyAdminHarvested(client, appDB, *effective, cfg)
		}
		cp := *effective
		report.HarvestedParams = &cp
	}

	report.DealsFound = len(harvestedDeals)
	if len(harvestedDeals) > 0 {
		ins, upd, err := db.AddDealsBatch(appDB.DB, harvestedDeals)
		if err != nil {
			slog.Warn("[SOCIAL_HARVESTER] Lỗi lưu mã Shopee FB/IG vào DB", "err", err)
		} else {
			if ins > 0 || upd > 0 {
				appDB.InvalidateDealsCache()
			}
			report.DealsInserted = ins
			report.DealsUpdated = upd
			slog.Info("🎁 [SOCIAL_HARVESTER] Tự động đồng bộ mã Shopee FB/IG/KOL", "inserted", ins, "updated", upd, "total", report.DealsFound)
		}
	}

	sigStrength := uint8(0)
	if report.HarvestedParams != nil {
		sigStrength = shopee.SocialParamStrength(*report.HarvestedParams)
	}
	statusMu.Lock()
	socialStatus.IsRunning = true
	socialStatus.IsSyncing = false
	socialStatus.SyncCycles++
	socialStatus.IntervalSecs = SocialSyncIntervalSecs
	socialStatus.LastSyncVN = currentVNTimestampStr()
	socialStatus.SigStrength = sigStrength
	socialStatus.DealsFound = report.DealsFound
	if report.HarvestedParams != nil {
		v := *report.HarvestedParams
		socialStatus.HarvestedParams = &v
	} else {
		socialStatus.HarvestedParams = nil
	}
	statusMu.Unlock()

	return report
}

func notifyAdminHarvested(client *http.Client, appDB *db.AppDb, params string, cfg config.Config) {
	adminID := cfg.AdminTelegramID
	if appDB != nil {
		if settings, err := appDB.GetSettingsMapCached(); err == nil {
			if rawID := strings.TrimSpace(settings["admin_telegram_id"]); rawID != "" {
				if parsedID, pErr := strconv.ParseInt(rawID, 10, 64); pErr == nil && parsedID != 0 {
					adminID = parsedID
				}
			}
		}
	}
	if adminID == 0 || strings.TrimSpace(cfg.TelegramToken) == "" || strings.Contains(cfg.TelegramToken, "DEMO_TOKEN") {
		return
	}
	msg := fmt.Sprintf(
		"🤖 <b>[TỰ ĐỘNG CẬP NHẬT CHỮ KÝ SHOPEE FB/INSTA]</b>\n\n"+
			"Hệ thống vừa tự động quét được bộ tham số phiên mạng xã hội mới:\n"+
			"<code>%s</code>\n\n"+
			"✅ Mọi link Shopee cấp cho khách từ thời điểm này sẽ tự động gắn chữ ký này để mở khóa <b>Mã Facebook / Instagram 20%% - 50%%</b>!",
		utils.EscapeHTML(params),
	)
	payload, _ := json.Marshal(map[string]any{
		"chat_id":    adminID,
		"text":       msg,
		"parse_mode": "HTML",
	})
	endpoint := fmt.Sprintf("https://api.telegram.org/bot%s/sendMessage", cfg.TelegramToken)
	req, err := http.NewRequest(http.MethodPost, endpoint, bytes.NewReader(payload))
	if err != nil {
		return
	}
	req.Header.Set("Content-Type", "application/json")
	if resp, err := client.Do(req); err == nil {
		_ = resp.Body.Close()
	}
}

// SpawnSocialHarvesterTask chạy nền quét Social Harvester ngay khi khởi động và lặp mỗi 5 phút.
func SpawnSocialHarvesterTask(stopCh <-chan struct{}, wg *sync.WaitGroup, appDB *db.AppDb, cfg config.Config) {
	statusMu.Lock()
	socialStatus.IsRunning = true
	socialStatus.IsSyncing = true
	socialStatus.IntervalSecs = SocialSyncIntervalSecs
	statusMu.Unlock()

	client := utils.NewSafePublicHTTPClient(10*time.Second, func(req *http.Request, via []*http.Request) error {
		if len(via) >= 5 {
			return http.ErrUseLastResponse
		}
		return nil
	})

	taskCtx, cancel := context.WithCancel(context.Background())
	utils.SafeGo(nil, "social.stop_watcher", func() {
		<-stopCh
		cancel()
	})
	utils.SafeGo(wg, "social.harvester_loop", func() {
		defer cancel()
		select {
		case <-stopCh:
			return
		case <-time.After(1 * time.Second):
		}

		ticker := time.NewTicker(SocialSyncIntervalSecs * time.Second)
		defer ticker.Stop()

		runOnce := func() {
			defer utils.RecoverPanic("social.SyncSocialAllOnce")
			statusMu.Lock()
			socialStatus.IsSyncing = true
			statusMu.Unlock()
			report := SyncSocialAllOnceWithContext(taskCtx, client, appDB, cfg)
			if report.HarvestedParams == nil && report.DealsFound == 0 && taskCtx.Err() == nil {
				slog.Warn("⚠️ [SOCIAL_HARVESTER] Chưa tìm thấy chữ ký hoặc mã FB/IG mới trong chu kỳ này.")
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
	})
}
