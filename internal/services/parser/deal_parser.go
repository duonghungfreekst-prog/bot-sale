package parser

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"

	"salehunter-go/internal/services/accesstrade"
	"salehunter-go/internal/services/shopee"
	"salehunter-go/internal/services/social"
	"salehunter-go/internal/utils"

	"github.com/google/uuid"
)

const chromeUA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"

// PlatformKind phân loại sàn TMĐT.
type PlatformKind int

const (
	PlatformUnknown PlatformKind = iota
	PlatformShopee
	PlatformLazada
	PlatformTiki
	PlatformTikTokShop
	PlatformAlibaba
	PlatformDynamic
)

// Platform đại diện sàn TMĐT (cố định hoặc mở rộng qua ACCESSTRADE).
type Platform struct {
	Kind       PlatformKind
	Name       string
	CampaignID string
}

// AsStr trả tên hiển thị chuẩn của sàn.
func (p Platform) AsStr() string {
	switch p.Kind {
	case PlatformShopee:
		return "Shopee"
	case PlatformLazada:
		return "Lazada"
	case PlatformTiki:
		return "Tiki"
	case PlatformTikTokShop:
		return "TikTokShop"
	case PlatformAlibaba:
		return "Alibaba"
	case PlatformDynamic:
		return p.Name
	default:
		return "Unknown"
	}
}

// AffiliateSettings cấu hình tiếp thị liên kết dùng khi tạo link đầu ra.
type AffiliateSettings struct {
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
}

// SaleVoucher thông tin gợi ý voucher chính theo sàn.
type SaleVoucher struct {
	Name             string
	DiscountDesc     string
	MinSpendDesc     string
	RemainingPercent uint8
	BackHours        string
}

// ParsedDeal kết quả phân tích 1 đường link sản phẩm hoàn chỉnh.
type ParsedDeal struct {
	Platform              Platform
	ProductTitle          string
	ProductPriceVND       *float64
	ProductShop           *string
	ProductBrand          *string
	ProductImageURL       *string
	OriginalURL           string
	ResolvedURL           string
	CleanURL              string
	AffiliateURL          string
	IsAffiliate           bool
	AffiliateNote         *string
	ClickID               string
	SubID                 string
	PrimaryVoucher        SaleVoucher
	EstimatedCashbackText string
	Tips                  []string
}

var (
	reURL         = regexp.MustCompile(`https?://[^\s]+`)
	reShopeeItem  = regexp.MustCompile(`(?:-i\.|\/product\/)(\d+)\/(\d+)|\bi\.(\d+)\.(\d+)`)
	reLazadaItem  = regexp.MustCompile(`-i(\d+)(?:-s(\d+))?\.html`)
	reLazadaOrig  = regexp.MustCompile(`<link\s+[^>]*rel=["']origin["'][^>]*href=["']([^"']+)["']`)
	reLazadaJS    = regexp.MustCompile(`window\.location\.href\s*=\s*['"](https?://[^'"]*lazada\.vn/products/[^'"]+)['"]`)
	reLazadaApp   = regexp.MustCompile(`itemId=(\d+)`)
	reTikiItem    = regexp.MustCompile(`-p(\d+)\.html`)
	reHTMLTitle   = regexp.MustCompile(`(?is)<title[^>]*>([^<]+)</title>`)
)

func parserRedirectCheck(req *http.Request, via []*http.Request) error {
	if len(via) >= 8 {
		return http.ErrUseLastResponse
	}
	low := strings.ToLower(req.URL.String())
	if strings.Contains(low, "/verify/") ||
		strings.Contains(low, "/buyer/login") ||
		strings.Contains(low, "/login") ||
		strings.Contains(low, "captcha") ||
		strings.Contains(low, "security_check") {
		return http.ErrUseLastResponse
	}
	return nil
}

// DealParser bộ máy phân tích URL sản phẩm, trích xuất metadata và gắn link Affiliate.
type DealParser struct {
	httpClient        *http.Client
	socialProbeClient *http.Client
	at                *accesstrade.AccessTradeClient
}

// New tạo DealParser mới kèm AccessTradeClient mặc định.
func New() *DealParser {
	return WithAccessTrade(accesstrade.NewClient())
}

// WithAccessTrade tạo DealParser dùng chung AccessTradeClient.
func WithAccessTrade(at *accesstrade.AccessTradeClient) *DealParser {
	return &DealParser{
		httpClient:        utils.NewSafePublicHTTPClient(6*time.Second, parserRedirectCheck),
		socialProbeClient: utils.NewSafePublicHTTPClient(5*time.Second, parserRedirectCheck),
		at:                at,
	}
}

// AccessTrade trả về AccessTradeClient dùng chung.
func (d *DealParser) AccessTrade() *accesstrade.AccessTradeClient {
	return d.at
}

// ExtractFirstURL tách đường link đầu tiên hợp lệ từ văn bản.
func (d *DealParser) ExtractFirstURL(text string) (string, bool) {
	m := reURL.FindString(text)
	if m == "" {
		return "", false
	}
	clean := strings.TrimRight(m, `.,!?)]>"'`)
	if clean == "" {
		return "", false
	}
	return clean, true
}

// ExtractAllURLsWithApproved trích xuất tối đa maxURLs link sàn TMĐT (hỗ trợ cả chiến dịch mở rộng).
func (d *DealParser) ExtractAllURLsWithApproved(text string, maxURLs int, approved *accesstrade.ApprovedCampaigns) []string {
	if maxURLs <= 0 {
		return nil
	}
	seen := make(map[string]struct{})
	var allUnique []string
	for _, raw := range reURL.FindAllString(text, -1) {
		clean := strings.TrimRight(raw, `.,!?)]>"'`)
		if clean == "" {
			continue
		}
		if _, dup := seen[clean]; !dup {
			seen[clean] = struct{}{}
			allUnique = append(allUnique, clean)
		}
	}

	var supported []string
	for _, u := range allUnique {
		if DetectPlatformWithApproved(u, approved).Kind != PlatformUnknown {
			supported = append(supported, u)
			if len(supported) >= maxURLs {
				break
			}
		}
	}
	if len(supported) > 0 {
		return supported
	}
	if len(allUnique) == 1 {
		return allUnique
	}
	return nil
}

// ExtractTitleFromText tách tên sản phẩm nếu người dùng gửi text kèm URL.
func (d *DealParser) ExtractTitleFromText(text, rawURL string) (string, bool) {
	withoutURL := strings.ReplaceAll(text, rawURL, "")
	var lines []string
	for _, line := range strings.Split(withoutURL, "\n") {
		t := strings.TrimSpace(line)
		if t != "" {
			lines = append(lines, t)
		}
	}
	clean := strings.TrimSpace(strings.Join(lines, " "))
	if len(clean) >= 6 {
		return clean, true
	}
	return "", false
}

func matchHostOrSubdomain(host string, domains ...string) bool {
	for _, d := range domains {
		if host == d || strings.HasSuffix(host, "."+d) {
			return true
		}
	}
	return false
}

// DetectPlatform nhận diện 5 sàn cốt lõi theo hostname chuẩn từ URL (chống giả mạo qua query/path).
func DetectPlatform(urlStr string) Platform {
	u, err := url.Parse(strings.TrimSpace(urlStr))
	if err != nil || u == nil {
		return Platform{Kind: PlatformUnknown}
	}
	scheme := strings.ToLower(u.Scheme)
	if scheme != "http" && scheme != "https" {
		return Platform{Kind: PlatformUnknown}
	}
	host := strings.ToLower(strings.TrimPrefix(u.Hostname(), "www."))
	if host == "" {
		return Platform{Kind: PlatformUnknown}
	}
	switch {
	case matchHostOrSubdomain(host, "shopee.vn", "shope.ee", "shp.ee"):
		return Platform{Kind: PlatformShopee}
	case matchHostOrSubdomain(host, "lazada.vn", "lzd.co"):
		return Platform{Kind: PlatformLazada}
	case matchHostOrSubdomain(host, "tiki.vn", "ti.ki"):
		return Platform{Kind: PlatformTiki}
	case matchHostOrSubdomain(host, "tiktok.com"):
		return Platform{Kind: PlatformTikTokShop}
	case matchHostOrSubdomain(host, "alibaba.com"):
		return Platform{Kind: PlatformAlibaba}
	default:
		return Platform{Kind: PlatformUnknown}
	}
}

// DetectPlatformWithApproved nhận diện sàn cốt lõi hoặc các chiến dịch mở rộng từ ACCESSTRADE.
func DetectPlatformWithApproved(urlStr string, approved *accesstrade.ApprovedCampaigns) Platform {
	p := DetectPlatform(urlStr)
	if p.Kind != PlatformUnknown {
		return p
	}
	if approved != nil {
		if dyn := approved.FindDynamicCampaignForURL(urlStr); dyn != nil {
			return Platform{
				Kind:       PlatformDynamic,
				Name:       dyn.PlatformGroup,
				CampaignID: dyn.ID,
			}
		}
	}
	return Platform{Kind: PlatformUnknown}
}

func isShortLink(urlStr string) bool {
	u, err := url.Parse(strings.TrimSpace(urlStr))
	if err != nil || u == nil {
		return false
	}
	scheme := strings.ToLower(u.Scheme)
	if scheme != "http" && scheme != "https" {
		return false
	}
	host := strings.ToLower(strings.TrimPrefix(u.Hostname(), "www."))
	path := strings.ToLower(u.Path)
	switch host {
	case "shope.ee", "shp.ee", "s.shopee.vn", "s.lazada.vn", "c.lazada.vn",
		"lzd.co", "ti.ki", "vt.tiktok.com", "vm.tiktok.com":
		return true
	}
	if (host == "lazada.vn" || strings.HasSuffix(host, ".lazada.vn")) && strings.HasPrefix(path, "/s.") {
		return true
	}
	if (host == "tiktok.com" || strings.HasSuffix(host, ".tiktok.com")) && strings.HasPrefix(path, "/t/") {
		return true
	}
	return false
}

// NormalizeShortURL chuẩn hóa link rút gọn Lazada bị mất subdomain s.lazada.vn.
func NormalizeShortURL(rawURL string) string {
	u, err := url.Parse(rawURL)
	if err != nil {
		return rawURL
	}
	host := strings.ToLower(u.Hostname())
	if strings.Contains(host, "lazada.vn") && !strings.HasPrefix(host, "s.lazada.vn") && !strings.HasPrefix(host, "c.lazada.vn") && strings.HasPrefix(u.Path, "/s.") {
		return "https://s.lazada.vn" + u.Path
	}
	return rawURL
}

func htmlEscapeDecode(s string) string {
	r := strings.NewReplacer(
		"&amp;", "&",
		"&quot;", `"`,
		"&#39;", "'",
		"&lt;", "<",
		"&gt;", ">",
	)
	return r.Replace(s)
}

func isJunkTitle(t string) bool {
	low := strings.ToLower(strings.TrimSpace(t))
	if low == "" {
		return true
	}
	switch low {
	case "shopee", "shopee việt nam", "lazada", "tiktok":
		return true
	}
	for _, kw := range []string{
		"security check", "captcha", "just a moment", "access denied",
		"cloudflare", "không tồn tại", "mua và bán trên ứng dụng",
	} {
		if strings.Contains(low, kw) {
			return true
		}
	}
	return false
}

func cleanShopeeMetaDescription(raw string) (string, bool) {
	dec := strings.TrimSpace(htmlEscapeDecode(raw))
	if !strings.HasPrefix(dec, "Mua ") && !strings.HasPrefix(dec, "mua ") {
		return "", false
	}
	rest := dec[len("Mua "):]
	if idx := strings.Index(rest, " giá tốt"); idx >= 0 {
		rest = rest[:idx]
	} else if idx := strings.Index(rest, ". Mua hàng qua mạng"); idx >= 0 {
		rest = rest[:idx]
	}
	cleaned := strings.TrimSpace(rest)
	if utf8.RuneCountInString(cleaned) < 4 {
		return "", false
	}
	return cleaned, true
}

func cleanMetaTitleSuffix(raw string) string {
	s := htmlEscapeDecode(strings.TrimSpace(raw))
	s = strings.NewReplacer("| Shopee Việt Nam", "", "| Lazada Việt Nam", "", "- Lazada.vn", "").Replace(s)
	s = strings.TrimSpace(s)
	s = strings.TrimSuffix(s, "|...")
	s = strings.TrimRight(s, ".")
	s = strings.TrimSpace(s)
	s = strings.TrimRight(s, "|")
	return strings.TrimSpace(s)
}

func extractMetaContent(html, attrKey, attrVal string) (string, bool) {
	ak := regexp.QuoteMeta(attrKey)
	av := regexp.QuoteMeta(attrVal)
	p1 := regexp.MustCompile(fmt.Sprintf(`(?is)<meta[^>]*\b%s=["']%s["'][^>]*\bcontent=["']([^"']+)["']`, ak, av))
	if m := p1.FindStringSubmatch(html); len(m) > 1 {
		return m[1], true
	}
	p2 := regexp.MustCompile(fmt.Sprintf(`(?is)<meta[^>]*\bcontent=["']([^"']+)["'][^>]*\b%s=["']%s["']`, ak, av))
	if m := p2.FindStringSubmatch(html); len(m) > 1 {
		return m[1], true
	}
	return "", false
}

// ExtractTitleFromHTML trích tên sản phẩm từ meta tags hoặc thẻ title.
func ExtractTitleFromHTML(html string) (string, bool) {
	for _, pair := range [][2]string{{"name", "description"}, {"property", "og:description"}} {
		if raw, ok := extractMetaContent(html, pair[0], pair[1]); ok {
			if cleaned, okClean := cleanShopeeMetaDescription(raw); okClean && !isJunkTitle(cleaned) {
				return cleaned, true
			}
		}
	}
	if raw, ok := extractMetaContent(html, "property", "og:title"); ok {
		cleaned := cleanMetaTitleSuffix(raw)
		if !isJunkTitle(cleaned) {
			return cleaned, true
		}
	}
	if m := reHTMLTitle.FindStringSubmatch(html); len(m) > 1 {
		cleaned := cleanMetaTitleSuffix(m[1])
		if !isJunkTitle(cleaned) {
			return cleaned, true
		}
	}
	return "", false
}

// ExtractImageFromHTML trích URL ảnh sản phẩm từ meta og:image / twitter:image.
func ExtractImageFromHTML(html string) *string {
	for _, pair := range [][2]string{{"property", "og:image"}, {"name", "twitter:image"}, {"property", "twitter:image"}} {
		if raw, ok := extractMetaContent(html, pair[0], pair[1]); ok {
			dec := strings.TrimSpace(htmlEscapeDecode(raw))
			low := strings.ToLower(dec)
			if (strings.HasPrefix(dec, "https://") || strings.HasPrefix(dec, "http://")) &&
				!strings.Contains(low, "icon_favicon") && !strings.Contains(low, "favicon.") {
				return &dec
			}
		}
	}
	return nil
}

// ExtractLazadaTargetFromHTML trích link sản phẩm Lazada từ HTML redirect/origin link.
func ExtractLazadaTargetFromHTML(html string) (string, bool) {
	if m := reLazadaOrig.FindStringSubmatch(html); len(m) > 1 {
		raw := strings.TrimSpace(m[1])
		if strings.Contains(raw, "lazada.vn/products/") {
			if u, err := url.Parse(raw); err == nil {
				return "https://www.lazada.vn" + u.Path, true
			}
			return raw, true
		}
	}
	if m := reLazadaJS.FindStringSubmatch(html); len(m) > 1 {
		if u, err := url.Parse(m[1]); err == nil {
			return "https://www.lazada.vn" + u.Path, true
		}
	}
	if m := reLazadaApp.FindStringSubmatch(html); len(m) > 1 {
		return fmt.Sprintf("https://www.lazada.vn/products/-i%s.html", m[1]), true
	}
	return "", false
}

func (d *DealParser) fetchSocialPreviewMetadata(rawURL string) (*string, *string) {
	req, err := http.NewRequest(http.MethodGet, rawURL, nil)
	if err != nil {
		return nil, nil
	}
	req.Header.Set("User-Agent", "TelegramBot (like TwitterBot)")
	resp, err := d.socialProbeClient.Do(req)
	if err != nil {
		return nil, nil
	}
	defer resp.Body.Close()
	bodyBytes, err := io.ReadAll(io.LimitReader(resp.Body, 1024*1024))
	if err != nil {
		return nil, nil
	}
	html := string(bodyBytes)
	var tPtr *string
	if t, ok := ExtractTitleFromHTML(html); ok {
		tPtr = &t
	}
	return tPtr, ExtractImageFromHTML(html)
}

func (d *DealParser) resolveURLWithMeta(rawURL string) (string, *string, *string) {
	if !isShortLink(rawURL) {
		return rawURL, nil, nil
	}
	fetchURL := NormalizeShortURL(rawURL)
	lowFetch := strings.ToLower(fetchURL)
	isShopeeShort := strings.Contains(lowFetch, "shope.ee") || strings.Contains(lowFetch, "shp.ee") || strings.Contains(lowFetch, "s.shopee.vn")

	var (
		wg          sync.WaitGroup
		finalURL    = fetchURL
		htmlTitle   *string
		htmlImg     *string
		socialTitle *string
		socialImg   *string
		fetchOK     bool
	)

	wg.Add(1)
	go func() {
		defer wg.Done()
		defer utils.RecoverPanic("parser.fetch_url")
		req, err := http.NewRequest(http.MethodGet, fetchURL, nil)
		if err != nil {
			return
		}
		req.Header.Set("User-Agent", chromeUA)
		resp, err := d.httpClient.Do(req)
		if err != nil {
			return
		}
		if resp.Request != nil && resp.Request.URL != nil {
			finalURL = resp.Request.URL.String()
		}
		bodyBytes, readErr := io.ReadAll(io.LimitReader(resp.Body, 1536*1024))
		_ = resp.Body.Close()
		fetchOK = true
		if readErr != nil {
			return
		}
		body := string(bodyBytes)
		if strings.Contains(finalURL, "lazada") || strings.Contains(fetchURL, "lazada") || strings.Contains(fetchURL, "lzd.co") {
			if target, ok := ExtractLazadaTargetFromHTML(body); ok {
				finalURL = target
			}
		} else if isShopeeShort {
			if cfgURL, ok := social.ExtractShopeeConfigHTTPURL(body); ok {
				finalURL = cfgURL
			}
		}
		if t, ok := ExtractTitleFromHTML(body); ok {
			htmlTitle = &t
		}
		htmlImg = ExtractImageFromHTML(body)
	}()

	if isShopeeShort {
		wg.Add(1)
		go func() {
			defer wg.Done()
			defer utils.RecoverPanic("parser.social_probe")
			socialTitle, socialImg = d.fetchSocialPreviewMetadata(fetchURL)
		}()
	}
	wg.Wait()

	if !fetchOK {
		return fetchURL, socialTitle, socialImg
	}
	bestTitle := socialTitle
	if bestTitle == nil {
		bestTitle = htmlTitle
	}
	bestImg := socialImg
	if bestImg == nil {
		bestImg = htmlImg
	}
	return finalURL, bestTitle, bestImg
}

func capitalizeWords(s string) string {
	fields := strings.Fields(s)
	for i, w := range fields {
		runes := []rune(w)
		if len(runes) > 0 {
			runes[0] = unicode.ToUpper(runes[0])
			fields[i] = string(runes)
		}
	}
	return strings.Join(fields, " ")
}

// ExtractTitleFromSlug rút tên sản phẩm đẹp từ URL slug.
func ExtractTitleFromSlug(urlStr string) string {
	u, err := url.Parse(urlStr)
	if err != nil {
		return "Sản Phẩm Khuyến Mãi"
	}
	var candidates []string
	for _, seg := range strings.Split(u.Path, "/") {
		if seg == "" {
			continue
		}
		low := strings.ToLower(seg)
		switch low {
		case "products", "product-detail", "product", "vi", "vi-vn", "en", "vn",
			"hotel", "hotels", "p", "dp", "c", "dien-thoai", "laptop", "may-tinh-bang", "phu-kien", "khuyen-mai":
			continue
		}
		allDigitDash := true
		for i := 0; i < len(seg); i++ {
			c := seg[i]
			if !((c >= '0' && c <= '9') || c == '.' || c == '-') {
				allDigitDash = false
				break
			}
		}
		if !allDigitDash {
			candidates = append(candidates, seg)
		}
	}

	bestSeg := ""
	maxLen := -1
	for _, seg := range candidates {
		// Giống Rust Iterator::max_by_key -> >= chọn phần tử cuối nếu cùng độ dài
		if len(seg) >= maxLen {
			maxLen = len(seg)
			bestSeg = seg
		}
	}

	if bestSeg != "" {
		cleanSeg := bestSeg
		if idx := strings.Index(cleanSeg, "-i."); idx >= 0 {
			cleanSeg = cleanSeg[:idx]
		}
		if loc := reTikiItem.FindStringIndex(cleanSeg); loc != nil {
			cleanSeg = cleanSeg[:loc[0]]
		}
		if idx := strings.Index(cleanSeg, "-i"); idx >= 0 {
			cleanSeg = cleanSeg[:idx]
		}
		cleanSeg = strings.TrimSuffix(cleanSeg, ".html")

		if idx := strings.LastIndex(cleanSeg, "-p-"); idx > 0 {
			suffix := cleanSeg[idx+len("-p-"):]
			if isAllASCII(suffix) {
				cleanSeg = cleanSeg[:idx]
			}
		}
		if idx := strings.LastIndexByte(cleanSeg, '_'); idx > 0 {
			suffix := cleanSeg[idx+1:]
			if isAllASCII(suffix) {
				cleanSeg = cleanSeg[:idx]
			}
		}

		title := strings.TrimSpace(strings.NewReplacer("-", " ", "_", " ").Replace(cleanSeg))
		hasLetter := false
		for _, r := range title {
			if unicode.IsLetter(r) {
				hasLetter = true
				break
			}
		}
		if hasLetter && len(title) > 3 && !strings.HasPrefix(title, "product") && !strings.HasPrefix(title, "s.") {
			return capitalizeWords(title)
		}
	}

	host := strings.ToLower(u.Hostname())
	switch {
	case strings.Contains(host, "shopee") || strings.Contains(host, "shp.ee"):
		return "Sản Phẩm Shopee"
	case strings.Contains(host, "lazada") || strings.Contains(host, "lzd.co"):
		return "Sản Phẩm Lazada"
	case strings.Contains(host, "tiki") || strings.Contains(host, "ti.ki"):
		return "Sản Phẩm Tiki"
	case strings.Contains(host, "tiktok"):
		return "Sản Phẩm TikTok Shop"
	default:
		return "Sản Phẩm Khuyến Mãi"
	}
}

func isAllASCII(s string) bool {
	if s == "" {
		return false
	}
	for i := 0; i < len(s); i++ {
		if s[i] < '0' || s[i] > '9' {
			return false
		}
	}
	return true
}

func stripTrackingQueryParams(u *url.URL) string {
	base := fmt.Sprintf("%s://%s%s", u.Scheme, u.Host, u.Path)
	rawQ := u.Query()
	var keptKeys []string
	for k := range rawQ {
		kl := strings.ToLower(k)
		if strings.HasPrefix(kl, "utm_") || strings.HasPrefix(kl, "aff_") || strings.HasPrefix(kl, "at_") {
			continue
		}
		switch kl {
		case "fbclid", "gclid", "ttclid", "spm", "scm", "sub1", "sub2", "sub3", "sub4", "sub5":
			continue
		}
		keptKeys = append(keptKeys, k)
	}
	if len(keptKeys) == 0 {
		return base
	}
	q := url.Values{}
	for _, k := range keptKeys {
		q[k] = rawQ[k]
	}
	if outU, err := url.Parse(base); err == nil {
		outU.RawQuery = q.Encode()
		return outU.String()
	}
	return base
}

// CleanURL loại bỏ toàn bộ tham số tracking thừa khỏi URL theo từng sàn.
func CleanURL(urlStr string, plat Platform) string {
	u, err := url.Parse(urlStr)
	if err != nil {
		return urlStr
	}
	switch plat.Kind {
	case PlatformShopee:
		if m := reShopeeItem.FindStringSubmatch(urlStr); len(m) >= 5 {
			if m[1] != "" && m[2] != "" {
				return fmt.Sprintf("https://shopee.vn/product/%s/%s", m[1], m[2])
			}
			if m[3] != "" && m[4] != "" {
				return fmt.Sprintf("https://shopee.vn/product/%s/%s", m[3], m[4])
			}
		}
		return "https://shopee.vn" + u.Path
	case PlatformLazada:
		if m := reLazadaItem.FindStringSubmatch(urlStr); len(m) >= 2 && m[1] != "" {
			return fmt.Sprintf("https://www.lazada.vn/products/-i%s.html", m[1])
		}
		host := strings.ToLower(u.Hostname())
		if strings.Contains(host, "s.lazada.vn") || strings.Contains(host, "c.lazada.vn") || strings.Contains(host, "lzd.co") {
			return fmt.Sprintf("https://%s%s", host, u.Path)
		}
		return "https://www.lazada.vn" + u.Path
	case PlatformTiki:
		if m := reTikiItem.FindStringSubmatch(urlStr); len(m) >= 2 && m[1] != "" {
			return fmt.Sprintf("https://tiki.vn/product-p%s.html", m[1])
		}
		return "https://tiki.vn" + u.Path
	case PlatformTikTokShop, PlatformAlibaba, PlatformUnknown:
		return fmt.Sprintf("%s://%s%s", u.Scheme, u.Host, u.Path)
	case PlatformDynamic:
		return stripTrackingQueryParams(u)
	default:
		return urlStr
	}
}

// BuildAffiliateURL tạo fallback affiliate URL theo deeplink template.
func BuildAffiliateURL(cleanURL, template string, userID int64, referrerID *int64, clickID string) (string, string, bool) {
	refStr := "0"
	if referrerID != nil {
		refStr = strconv.FormatInt(*referrerID, 10)
	}
	shortClick := clickID
	if len(shortClick) > 8 {
		shortClick = shortClick[:8]
	}
	subID := fmt.Sprintf("u%d_r%s_%s", userID, refStr, shortClick)

	trimmedTpl := strings.TrimSpace(template)
	if strings.Contains(trimmedTpl, "630000000000") || trimmedTpl == "" || !utils.IsSafeHTTPURL(trimmedTpl) {
		return cleanURL, subID, false
	}
	encoded := utils.PercentEncodeRFC3986(cleanURL)
	userStr := strconv.FormatInt(userID, 10)
	r := strings.NewReplacer(
		"{{CLEAN_URL}}", encoded,
		"{CLEAN_URL}", encoded,
		"{{USER_ID}}", userStr,
		"{USER_ID}", userStr,
		"{{REF_ID}}", refStr,
		"{REF_ID}", refStr,
		"{{CLICK_ID}}", shortClick,
		"{CLICK_ID}", shortClick,
	)
	built := r.Replace(trimmedTpl)
	if !utils.IsSafeHTTPURL(built) {
		return cleanURL, subID, false
	}
	return built, subID, true
}

// PickBestVoucher tạo gợi ý voucher & mẹo áp mã phù hợp từng sàn.
func PickBestVoucher(plat Platform, isAffiliate bool) (SaleVoucher, []string) {
	switch plat.Kind {
	case PlatformShopee:
		if isAffiliate {
			return SaleVoucher{
					Name:             "Mã Facebook / Affiliate 20%",
					DiscountDesc:     "Link đã gắn luồng Facebook (áp dụng tùy tài khoản & shop)",
					MinSpendDesc:     "Đơn từ 50k",
					RemainingPercent: 83,
					BackHours:        "0H · 9H · 15H · 20H",
				}, []string{
					"Khung 9H, 15H, 20H thường nhả thêm mã 20%-50% từ Shopee x FB/IG.",
					"Bấm mở link sang App rồi thử kiểm tra tại mục Shopee Voucher khi đặt hàng.",
					"Xoá sản phẩm cũ trong giỏ rồi thêm lại từ link vừa mở để ghi nhận luồng.",
				}
		}
		return SaleVoucher{
				Name:             "Link Sạch Sản Phẩm + Cổng Săn Mã Shopee",
				DiscountDesc:     "Bấm lưu mã tại Cổng Voucher / Shopee Video bên dưới trước khi mua",
				MinSpendDesc:     "Đơn từ 0đ - 50k",
				RemainingPercent: 83,
				BackHours:        "0H · 9H · 15H · 20H",
			}, []string{
				"Mở Cổng Mã Shopee hoặc Shopee Video bằng nút bên dưới để lưu mã trước khi đặt.",
				"Khung 0H, 9H, 15H, 20H thường mở thêm mã 20%-50% tùy tài khoản.",
				"Xoá sản phẩm cũ trong giỏ rồi thêm lại để áp được mã Video/Live.",
			}
	case PlatformLazada:
		return SaleVoucher{
				Name:             "Voucher Tích Lũy 100K + Hoàn Tiền MAX",
				DiscountDesc:     "Thu thập voucher tại cổng Lazada trước khi thanh toán",
				MinSpendDesc:     "Đơn từ 150k",
				RemainingPercent: 88,
				BackHours:        "0H · 9H · 12H · 20H",
			}, []string{
				"Lazada tự động cộng gộp 4 lớp mã: Voucher Sàn + Tích Luỹ + Freeship + Mã Shop.",
				"Thu thập thêm Voucher Tích Luỹ ở trang chủ Lazada trước khi thanh toán.",
			}
	case PlatformTiki:
		return SaleVoucher{
				Name:             "Mã Giảm Toàn Sàn + TikiNOW",
				DiscountDesc:     "Lưu mã tại Cổng Khuyến Mãi Tiki hoặc trang thanh toán",
				MinSpendDesc:     "Đơn từ 100k",
				RemainingPercent: 78,
				BackHours:        "8H · 12H · 16H · 20H",
			}, []string{
				"Đừng quên kiểm tra mục Mã Giảm Giá từ TikiCard hoặc Ví Momo ở bước thanh toán.",
				"Ưu tiên chọn sản phẩm có huy hiệu TikiTrading để nhận hàng trong 2 giờ.",
			}
	case PlatformAlibaba:
		return SaleVoucher{
				Name:             "Ưu Đãi Nguồn Hàng Giá Sỉ Alibaba",
				DiscountDesc:     "Giá sỉ tận xưởng & ưu đãi vận chuyển quốc tế Alibaba.com",
				MinSpendDesc:     "Đơn sỉ / lẻ quốc tế",
				RemainingPercent: 90,
				BackHours:        "Hoạt động 24/7",
			}, []string{
				"Nhắn trực tiếp nhà cung cấp (Chat Now) trên Alibaba để xin báo giá sỉ tốt hơn.",
				"Ưu tiên các nhà cung cấp có huy hiệu Verified Supplier & Trade Assurance.",
			}
	case PlatformDynamic:
		return SaleVoucher{
				Name:             "Link Đối Tác Chính Thức " + plat.Name,
				DiscountDesc:     "Đã gắn link tiếp thị trực tiếp qua hệ thống đối tác ACCESSTRADE",
				MinSpendDesc:     "Theo điều kiện đơn tại trang hãng",
				RemainingPercent: 92,
				BackHours:        "Cập nhật liên tục 24/7",
			}, []string{
				"Hoàn tất đặt hàng / đăng ký trực tiếp trên trình duyệt vừa mở từ bot để ghi nhận đơn.",
				"Không đóng giữa chừng hoặc chuyển sang tab khác chứa mã giảm giá của bên thứ 3.",
			}
	default:
		return SaleVoucher{
				Name:             "Voucher Livestream / Giỏ Hàng TikTok Shop",
				DiscountDesc:     "Áp mã trợ giá trực tiếp tại giỏ hàng TikTok Shop",
				MinSpendDesc:     "Đơn từ 80k",
				RemainingPercent: 85,
				BackHours:        "Cập nhật liên tục 24/7",
			}, []string{
				"Bấm link để nhảy thẳng vào Giỏ hàng / Livestream trên App TikTok.",
				"TikTok Shop thường trợ giá thêm 20%-30% cho đơn đặt từ liên kết ưu đãi.",
			}
	}
}

type tikiProductAPIResp struct {
	Name          *string `json:"name"`
	Price         any     `json:"price"`
	ThumbnailURL  *string `json:"thumbnail_url"`
	CurrentSeller *struct {
		Name *string `json:"name"`
	} `json:"current_seller"`
	Brand *struct {
		Name *string `json:"name"`
	} `json:"brand"`
}

// ParseTikiProductJSON phân tích JSON phản hồi từ API Tiki v2 product.
func ParseTikiProductJSON(body string) (title string, price *float64, shop *string, brand *string, ok bool) {
	var parsed tikiProductAPIResp
	if err := json.Unmarshal([]byte(body), &parsed); err != nil {
		return "", nil, nil, nil, false
	}
	if parsed.Name != nil {
		title = strings.TrimSpace(*parsed.Name)
	}
	switch v := parsed.Price.(type) {
	case float64:
		if v > 0 {
			cp := v
			price = &cp
		}
	case string:
		if f, err := strconv.ParseFloat(strings.TrimSpace(v), 64); err == nil && f > 0 {
			price = &f
		}
	}
	if parsed.CurrentSeller != nil && parsed.CurrentSeller.Name != nil {
		if s := strings.TrimSpace(*parsed.CurrentSeller.Name); s != "" {
			shop = &s
		}
	}
	if parsed.Brand != nil && parsed.Brand.Name != nil {
		if b := strings.TrimSpace(*parsed.Brand.Name); b != "" {
			brand = &b
		}
	}
	if title == "" && price == nil && shop == nil && brand == nil {
		return "", nil, nil, nil, false
	}
	return title, price, shop, brand, true
}

func (d *DealParser) fetchTikiProductInfo(productID string) (title string, price *float64, shop *string, brand *string, thumb *string, ok bool) {
	apiURL := "https://tiki.vn/api/v2/products/" + productID
	req, err := http.NewRequest(http.MethodGet, apiURL, nil)
	if err != nil {
		return
	}
	req.Header.Set("User-Agent", chromeUA)
	resp, err := d.httpClient.Do(req)
	if err != nil {
		return
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return
	}
	bodyBytes, err := io.ReadAll(io.LimitReader(resp.Body, 1024*1024))
	if err != nil {
		return
	}
	body := string(bodyBytes)
	title, price, shop, brand, ok = ParseTikiProductJSON(body)
	if ok {
		var tmp tikiProductAPIResp
		if json.Unmarshal(bodyBytes, &tmp) == nil && tmp.ThumbnailURL != nil {
			t := strings.TrimSpace(*tmp.ThumbnailURL)
			if strings.HasPrefix(t, "https://") || strings.HasPrefix(t, "http://") {
				thumb = &t
			}
		}
	}
	return
}

// ProcessLink phân tích URL, resolve redirect, trích metadata và gắn tracking Affiliate theo thứ tự ưu tiên.
func (d *DealParser) ProcessLink(inputText string, userID int64, referrerID *int64, settings AffiliateSettings) (*ParsedDeal, error) {
	rawURL, found := d.ExtractFirstURL(inputText)
	if !found {
		return nil, errors.New("không tìm thấy đường link hợp lệ trong tin nhắn")
	}
	if err := utils.ValidatePublicTargetURL(rawURL); err != nil {
		return nil, errors.New("đường link không hợp lệ hoặc thuộc địa chỉ mạng nội bộ bị chặn bảo mật")
	}

	approved := d.at.GetOrLoadApprovedCampaigns(settings.AccessTradeToken)
	initialPlat := DetectPlatformWithApproved(rawURL, approved)

	resolvedURL, htmlTitle, prodImgURL := d.resolveURLWithMeta(rawURL)
	plat := initialPlat
	if plat.Kind == PlatformUnknown {
		plat = DetectPlatformWithApproved(resolvedURL, approved)
	}
	if plat.Kind == PlatformUnknown {
		dynHint := ""
		if approved != nil {
			dynList := approved.DynamicCampaigns()
			if len(dynList) > 0 {
				var names []string
				for i, c := range dynList {
					if i >= 6 {
						break
					}
					names = append(names, c.PlatformGroup)
				}
				dynHint = " và các hãng: " + strings.Join(names, ", ")
			}
		}
		return nil, fmt.Errorf("hiện tại bot hỗ trợ Shopee, Lazada, Tiki, TikTok Shop, Alibaba%s. Vui lòng gửi link thuộc các sàn/trang đã được duyệt", dynHint)
	}

	userTitle, hasUserTitle := d.ExtractTitleFromText(inputText, rawURL)
	prodTitle := ExtractTitleFromSlug(resolvedURL)
	if hasUserTitle {
		prodTitle = userTitle
	} else if htmlTitle != nil {
		prodTitle = *htmlTitle
	}

	cleanedURL := CleanURL(resolvedURL, plat)
	clickID := strings.ReplaceAll(uuid.NewString(), "-", "")
	shortClick := clickID[:8]

	refStr := "0"
	if referrerID != nil {
		refStr = strconv.FormatInt(*referrerID, 10)
	}
	defaultSubID := fmt.Sprintf("u%d_r%s_%s", userID, refStr, shortClick)

	var (
		prodPrice *float64
		prodShop  *string
		prodBrand *string
	)
	if plat.Kind == PlatformTiki {
		tikiPID := ""
		if m := reTikiItem.FindStringSubmatch(cleanedURL); len(m) > 1 {
			tikiPID = m[1]
		} else if m := reTikiItem.FindStringSubmatch(resolvedURL); len(m) > 1 {
			tikiPID = m[1]
		}
		if tikiPID != "" {
			if tTitle, tPrice, tShop, tBrand, tThumb, ok := d.fetchTikiProductInfo(tikiPID); ok {
				if !hasUserTitle && tTitle != "" {
					prodTitle = tTitle
				}
				prodPrice = tPrice
				prodShop = tShop
				prodBrand = tBrand
				if prodImgURL == nil && tThumb != nil {
					prodImgURL = tThumb
				}
			}
		}
	}

	overrideCampID := ""
	switch plat.Kind {
	case PlatformShopee:
		overrideCampID = settings.CampaignShopee
	case PlatformLazada:
		overrideCampID = settings.CampaignLazada
	case PlatformTiki:
		overrideCampID = settings.CampaignTiki
	case PlatformTikTokShop:
		overrideCampID = settings.CampaignTikTok
	case PlatformDynamic:
		overrideCampID = plat.CampaignID
	}

	var (
		affNote      *string
		directAffRes *struct {
			url, subID string
			isAff      bool
		}
	)

	// Nhánh 0: Shopee Open API hoặc an_redir chính chủ
	if plat.Kind == PlatformShopee {
		appID := strings.TrimSpace(settings.ShopeeAppID)
		appSecret := strings.TrimSpace(settings.ShopeeAppSecret)
		if appID != "" && appSecret != "" {
			shortLink, err := shopee.GenerateOpenAPIShortLink(
				d.httpClient,
				appID,
				appSecret,
				cleanedURL,
				[]string{strconv.FormatInt(userID, 10), refStr, shortClick},
			)
			if err == nil {
				finalShopee := shopee.AppendSocialParamsToShopeeLink(shortLink, settings.ShopeeSocialParams)
				directAffRes = &struct {
					url, subID string
					isAff      bool
				}{url: finalShopee, subID: defaultSubID, isAff: true}
			} else {
				msg := err.Error()
				affNote = &msg
			}
		}

		if directAffRes == nil && shopee.IsValidShopeeAffiliateID(settings.ShopeeAffiliateID) {
			directLink := shopee.BuildDirectAnRedirLink(cleanedURL, settings.ShopeeAffiliateID, defaultSubID)
			finalShopee := shopee.AppendSocialParamsToShopeeLink(directLink, settings.ShopeeSocialParams)
			affNote = nil
			directAffRes = &struct {
				url, subID string
				isAff      bool
			}{url: finalShopee, subID: defaultSubID, isAff: true}
		}
	}

	var (
		affURL      string
		subID       string
		isAffiliate bool
	)

	if directAffRes != nil {
		affURL = directAffRes.url
		subID = directAffRes.subID
		isAffiliate = directAffRes.isAff
	} else if tok := strings.TrimSpace(settings.AccessTradeToken); tok != "" {
		// Nhánh 1: ACCESSTRADE API
		res, err := d.at.CreateAffiliateLink(tok, plat.AsStr(), overrideCampID, cleanedURL, strconv.FormatInt(userID, 10), shortClick)
		if err == nil {
			if !hasUserTitle && res.ProductName != nil {
				prodTitle = *res.ProductName
			}
			if res.ProductPriceVND != nil {
				prodPrice = res.ProductPriceVND
			}
			affURL = res.URL
			subID = defaultSubID
			isAffiliate = true
		} else {
			msg := err.Error()
			affNote = &msg
			fbURL, fbSub, fbIsAff := BuildAffiliateURL(cleanedURL, settings.DeeplinkTemplate, userID, referrerID, clickID)
			if plat.Kind == PlatformShopee {
				fbURL = shopee.AppendSocialParamsToShopeeLink(fbURL, settings.ShopeeSocialParams)
			}
			affURL, subID, isAffiliate = fbURL, fbSub, fbIsAff
		}
	} else {
		// Nhánh 2: Fallback Template / Link sạch
		fbURL, fbSub, fbIsAff := BuildAffiliateURL(cleanedURL, settings.DeeplinkTemplate, userID, referrerID, clickID)
		if plat.Kind == PlatformShopee {
			fbURL = shopee.AppendSocialParamsToShopeeLink(fbURL, settings.ShopeeSocialParams)
		}
		affURL, subID, isAffiliate = fbURL, fbSub, fbIsAff
	}

	primaryVoucher, tips := PickBestVoucher(plat, isAffiliate)
	var cashbackTxt string
	switch plat.Kind {
	case PlatformShopee:
		cashbackTxt = "Hoàn thêm 3.0% - 10.5% (Tối đa 150,000₫/đơn)"
	case PlatformLazada:
		cashbackTxt = "Hoàn thêm 2.5% - 8.0% (Tối đa 100,000₫/đơn)"
	case PlatformTiki:
		cashbackTxt = "Hoàn thêm 1.5% - 5.0% (Tối đa 80,000₫/đơn)"
	case PlatformTikTokShop:
		cashbackTxt = "Hoàn thêm 3.0% - 12.0% (Tối đa 120,000₫/đơn)"
	case PlatformAlibaba:
		cashbackTxt = "Hoa hồng đối tác quốc tế 3.92%/đơn"
	case PlatformDynamic:
		cashbackTxt = "Ưu đãi chính hãng & hoa hồng đối tác ACCESSTRADE"
	default:
		cashbackTxt = "Hoàn tiền lên tới 10%"
	}

	return &ParsedDeal{
		Platform:              plat,
		ProductTitle:          prodTitle,
		ProductPriceVND:       prodPrice,
		ProductShop:           prodShop,
		ProductBrand:          prodBrand,
		ProductImageURL:       prodImgURL,
		OriginalURL:           rawURL,
		ResolvedURL:           resolvedURL,
		CleanURL:              cleanedURL,
		AffiliateURL:          affURL,
		IsAffiliate:           isAffiliate,
		AffiliateNote:         affNote,
		ClickID:               clickID,
		SubID:                 subID,
		PrimaryVoucher:        primaryVoucher,
		EstimatedCashbackText: cashbackTxt,
		Tips:                  tips,
	}, nil
}
