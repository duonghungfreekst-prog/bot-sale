package accesstrade

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"math"
	"net/http"
	"net/url"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"salehunter-go/internal/db"
	"salehunter-go/internal/utils"
)

const (
	CacheTTLSecs        = int64(180)
	VoucherCacheTTLSecs = int64(55)
	TikiBrokenCampID    = "4348614231480407268"
	TikiPreferredCampID = "6985318064449073386"
	browserUA           = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
)

var (
	reDiscountPct = regexp.MustCompile(`(?i)(?:giảm|sale|off)\s*(\d{1,2})\s*%`)
	reDiscountAmt = regexp.MustCompile(`(?i)(?:giảm|trợ\s*giá)\s*(?:trực\s*tiếp\s*)?([\d,\.]+)\s*(k\b|triệu|tr\b|vnđ|đ\b)`)
	reMinSpend    = regexp.MustCompile(`(?i)(?:tối\s*thiểu|đơn\s*từ|hàng\s*từ|từ)\s*([\d,\.]+)\s*(k\b|triệu|tr\b|vnđ|đ)?`)
)

// VoucherSyncMeta thống kê lần quét kho mã mới nhất.
type VoucherSyncMeta struct {
	LastSyncVN string
	SyncCycles uint64
	TotalCount int
}

// VoucherOffer thông tin một mã giảm giá / khuyến mãi từ ACCESSTRADE.
type VoucherOffer struct {
	Platform        string  `json:"platform"`
	ShopName        string  `json:"shop_name"`
	Title           string  `json:"title"`
	CouponCode      string  `json:"coupon_code"`
	DiscountPercent uint32  `json:"discount_percent"`
	DiscountAmount  float64 `json:"discount_amount"`
	DiscountDesc    string  `json:"discount_desc"`
	MinSpend        float64 `json:"min_spend"`
	RawURL          string  `json:"raw_url"`
	AffURL          *string `json:"aff_url,omitempty"`
	EndTime         string  `json:"end_time"`
}

// BestTargetURL ưu tiên aff_link gắn campaign đang hoạt động và né gói Tiki bị HTTP 500.
func (v VoucherOffer) BestTargetURL(approved *ApprovedCampaigns) string {
	var activeCampID string
	if approved != nil {
		switch v.Platform {
		case "Shopee":
			if approved.Shopee != nil {
				activeCampID = approved.Shopee.ID
			}
		case "Lazada":
			if approved.Lazada != nil {
				activeCampID = approved.Lazada.ID
			}
		case "Tiki":
			if approved.Tiki != nil {
				activeCampID = approved.Tiki.ID
			}
		case "TikTok Shop":
			if approved.TikTok != nil {
				activeCampID = approved.TikTok.ID
			}
		case "Alibaba":
			if approved.Alibaba != nil {
				activeCampID = approved.Alibaba.ID
			}
		}
	}

	if activeCampID != "" && v.AffURL != nil {
		affTrimmed := strings.TrimSpace(*v.AffURL)
		if activeCampID != TikiBrokenCampID && affTrimmed != "" {
			if strings.Contains(affTrimmed, activeCampID) || !strings.Contains(affTrimmed, "go.isclix.com/deep_link/") {
				return affTrimmed
			}
			if beforeQ, afterQ, ok := strings.Cut(affTrimmed, "?"); ok {
				base := strings.TrimRight(strings.TrimSuffix(beforeQ, "/"+TikiBrokenCampID), "/")
				return fmt.Sprintf("%s/%s?%s", base, activeCampID, afterQ)
			}
		}
	}
	return v.RawURL
}

// DiscountBadge nhãn hiển thị mức giảm (VD: "-35%", "-300K", "-45đ", hoặc "ƯU ĐÃI").
func (v VoucherOffer) DiscountBadge() string {
	if v.DiscountPercent > 0 {
		return fmt.Sprintf("-%d%%", v.DiscountPercent)
	}
	if v.DiscountAmount >= 1000.0 {
		return fmt.Sprintf("-%dK", int64(math.Round(v.DiscountAmount/1000.0)))
	}
	if v.DiscountAmount > 0.0 {
		return fmt.Sprintf("-%dđ", int64(v.DiscountAmount))
	}
	return "ƯU ĐÃI"
}

// IsShopSpecific kiểm tra mã thuộc riêng Shop/Thương hiệu hay là mã chung toàn sàn.
func (v VoucherOffer) IsShopSpecific() bool {
	return !IsGenericPlatformVoucherShop(strings.ToLower(v.ShopName), strings.ToLower(v.Platform))
}

// CampaignPair cặp (CampaignID, CampaignName).
type CampaignPair struct {
	ID   string
	Name string
}

// ApprovedCampaignSummary tóm tắt 1 chiến dịch đã duyệt (kể cả chiến dịch mở rộng).
type ApprovedCampaignSummary struct {
	ID            string
	Merchant      string
	Name          string
	PlatformGroup string
	Domain        string
}

// ApprovedCampaigns tập chiến dịch đã duyệt được cache.
type ApprovedCampaigns struct {
	Shopee         *CampaignPair
	Lazada         *CampaignPair
	Tiki           *CampaignPair
	TikTok         *CampaignPair
	Alibaba        *CampaignPair
	AllApproved    []ApprovedCampaignSummary
	FetchedAtEpoch int64
}

// DynamicCampaigns lọc các chiến dịch thương hiệu/sàn mở rộng ngoài 5 sàn cốt lõi.
func (a *ApprovedCampaigns) DynamicCampaigns() []ApprovedCampaignSummary {
	if a == nil {
		return nil
	}
	var out []ApprovedCampaignSummary
	for _, c := range a.AllApproved {
		if c.Domain == "" {
			continue
		}
		switch c.PlatformGroup {
		case "Shopee", "Lazada", "Tiki", "TikTok Shop", "Alibaba", "Khác":
			continue
		default:
			out = append(out, c)
		}
	}
	return out
}

// FindDynamicCampaignForURL tìm chiến dịch mở rộng khớp domain nhất với URL sản phẩm.
func (a *ApprovedCampaigns) FindDynamicCampaignForURL(rawURL string) *ApprovedCampaignSummary {
	if a == nil {
		return nil
	}
	userHost := ExtractNormalizedHost(rawURL)
	if userHost == "" || isCoreMarketplaceDomain(userHost) {
		return nil
	}
	type matched struct {
		camp  ApprovedCampaignSummary
		exact bool
	}
	var matches []matched
	for _, c := range a.DynamicCampaigns() {
		if userHost == c.Domain {
			matches = append(matches, matched{camp: c, exact: true})
		} else if strings.HasSuffix(userHost, "."+c.Domain) {
			matches = append(matches, matched{camp: c, exact: false})
		}
	}
	if len(matches) == 0 {
		return nil
	}
	sort.SliceStable(matches, func(i, j int) bool {
		if matches[i].exact != matches[j].exact {
			return matches[i].exact
		}
		return len(matches[i].camp.Domain) > len(matches[j].camp.Domain)
	})
	cp := matches[0].camp
	return &cp
}

// AffiliateLinkResult kết quả gọi ACCESSTRADE API tạo link affiliate.
type AffiliateLinkResult struct {
	URL                 string
	CampaignID          string
	ProductName         *string
	ProductPriceVND     *float64
	CommissionAmountVND *float64
}

// VoucherMatchQuery bộ lọc tìm mã giảm giá khớp sản phẩm/shop/thương hiệu.
type VoucherMatchQuery struct {
	Platform     string
	ProductTitle string
	CleanURL     string
	ShopName     *string
	BrandName    *string
	PriceVND     *float64
	MaxItems     int
}

type cachedVoucherPool struct {
	token     string
	fetchedAt int64
	minPct    uint32
	list      []VoucherOffer
}

type campaignsCacheState struct {
	token    string
	approved *ApprovedCampaigns
}

// AccessTradeClient kết nối ACCESSTRADE API kèm singleflight, TTL cache và kiểm tra redirect.
type AccessTradeClient struct {
	httpClient         *http.Client
	redirectProbe      *http.Client
	muCamp             sync.RWMutex
	campCache          *campaignsCacheState
	campaignsFetchLock sync.Mutex
	muVoucher          sync.RWMutex
	voucherCache       *cachedVoucherPool
	muMeta             sync.RWMutex
	voucherMeta        VoucherSyncMeta
}

func isTrackingGatewayHost(host string) bool {
	switch strings.ToLower(strings.TrimSpace(host)) {
	case "shorten.asia", "go.isclix.com", "fast.accesstrade.com.vn":
		return true
	default:
		return false
	}
}

// NewClient khởi tạo AccessTradeClient với HTTP client và probe redirect chuẩn.
func NewClient() *AccessTradeClient {
	return &AccessTradeClient{
		httpClient: &http.Client{
			Timeout: 12 * time.Second,
		},
		redirectProbe: &http.Client{
			Timeout: 4 * time.Second,
			CheckRedirect: func(req *http.Request, via []*http.Request) error {
				if len(via) >= 4 {
					return http.ErrUseLastResponse
				}
				if isTrackingGatewayHost(req.URL.Hostname()) {
					return nil
				}
				return http.ErrUseLastResponse
			},
		},
	}
}

// CleanAPIToken chuẩn hóa API access key (xóa dấu ngoặc kép và tiền tố Token/Bearer).
func CleanAPIToken(raw string) string {
	t := strings.TrimSpace(raw)
	t = strings.Trim(t, `"'`)
	t = strings.TrimSpace(t)
	low := strings.ToLower(t)
	if strings.HasPrefix(low, "token ") {
		t = t[len("token "):]
	} else if strings.HasPrefix(low, "bearer ") {
		t = t[len("bearer "):]
	}
	return strings.TrimSpace(t)
}

// MaskToken che API token để hiển thị an toàn.
func MaskToken(tok string) string {
	return utils.MaskToken(tok)
}

// InvalidateCache xóa cache chiến dịch khi Admin đổi token/cấu hình.
func (c *AccessTradeClient) InvalidateCache() {
	c.muCamp.Lock()
	c.campCache = nil
	c.muCamp.Unlock()
}

// PeekApprovedCampaigns trả về chiến dịch đã cache trong RAM nếu có.
func (c *AccessTradeClient) PeekApprovedCampaigns() *ApprovedCampaigns {
	c.muCamp.RLock()
	defer c.muCamp.RUnlock()
	if c.campCache == nil || c.campCache.approved == nil {
		return nil
	}
	cp := *c.campCache.approved
	return &cp
}

// PeekCachedHighDiscountVouchers trả về danh sách voucher đang giữ trong RAM.
func (c *AccessTradeClient) PeekCachedHighDiscountVouchers() []VoucherOffer {
	c.muVoucher.RLock()
	defer c.muVoucher.RUnlock()
	if c.voucherCache == nil {
		return nil
	}
	out := make([]VoucherOffer, len(c.voucherCache.list))
	copy(out, c.voucherCache.list)
	return out
}

// GetVoucherSyncMeta trả về metadata lần đồng bộ voucher gần nhất.
func (c *AccessTradeClient) GetVoucherSyncMeta() VoucherSyncMeta {
	c.muMeta.RLock()
	defer c.muMeta.RUnlock()
	return c.voucherMeta
}

func vnNowStr() string {
	loc := time.FixedZone("ICT", 7*3600)
	return time.Now().In(loc).Format("15:04:05 02/01/2006")
}

func vnTodayYMD() string {
	loc := time.FixedZone("ICT", 7*3600)
	return time.Now().In(loc).Format("2006-01-02")
}

// SeedVoucherCacheFromJSON nạp cache kho mã đã lưu từ DB lúc khởi động nếu RAM đang rỗng.
func (c *AccessTradeClient) SeedVoucherCacheFromJSON(rawToken, jsonStr string) int {
	token := CleanAPIToken(rawToken)
	if token == "" || strings.TrimSpace(jsonStr) == "" {
		return 0
	}
	var offers []VoucherOffer
	if err := json.Unmarshal([]byte(jsonStr), &offers); err != nil {
		return 0
	}
	todayYMD := vnTodayYMD()
	valid := make([]VoucherOffer, 0, len(offers))
	for _, v := range offers {
		exp := strings.ReplaceAll(strings.TrimSpace(v.EndTime), "/", "-")
		if len(exp) >= 10 {
			exp = exp[:10]
			if exp < todayYMD {
				continue
			}
		}
		valid = append(valid, v)
	}
	if len(valid) == 0 {
		return 0
	}
	now := time.Now().Unix()

	c.muVoucher.Lock()
	if c.voucherCache != nil && len(c.voucherCache.list) > 0 {
		c.muVoucher.Unlock()
		return 0
	}
	c.voucherCache = &cachedVoucherPool{
		token:     token,
		fetchedAt: now - 60,
		minPct:    0,
		list:      valid,
	}
	count := len(valid)
	c.muVoucher.Unlock()

	c.muMeta.Lock()
	c.voucherMeta = VoucherSyncMeta{
		LastSyncVN: vnNowStr() + " (Cache DB)",
		SyncCycles: 0,
		TotalCount: count,
	}
	c.muMeta.Unlock()
	return count
}

// ExportVoucherCacheJSON xuất tối đa 700 mã đầu từ RAM để lưu vào DB.
func (c *AccessTradeClient) ExportVoucherCacheJSON() (string, bool) {
	c.muVoucher.RLock()
	defer c.muVoucher.RUnlock()
	if c.voucherCache == nil || len(c.voucherCache.list) == 0 {
		return "", false
	}
	limit := 700
	if len(c.voucherCache.list) < limit {
		limit = len(c.voucherCache.list)
	}
	b, err := json.Marshal(c.voucherCache.list[:limit])
	if err != nil {
		return "", false
	}
	return string(b), true
}

func (c *AccessTradeClient) doATRequest(method, rawURL, token string, body []byte) (int, []byte, error) {
	var bodyReader io.Reader
	if body != nil {
		bodyReader = bytes.NewReader(body)
	}
	req, err := http.NewRequest(method, rawURL, bodyReader)
	if err != nil {
		return 0, nil, err
	}
	req.Header.Set("Authorization", "Token "+token)
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", browserUA)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return 0, nil, err
	}
	defer resp.Body.Close()
	data, readErr := io.ReadAll(io.LimitReader(resp.Body, 8*1024*1024))
	return resp.StatusCode, data, readErr
}

// GetOrLoadApprovedCampaigns lấy danh sách chiến dịch đã duyệt (tự nạp nếu chưa có token cache).
func (c *AccessTradeClient) GetOrLoadApprovedCampaigns(rawToken string) *ApprovedCampaigns {
	if peeked := c.PeekApprovedCampaigns(); peeked != nil {
		return peeked
	}
	token := CleanAPIToken(rawToken)
	if token == "" {
		return nil
	}
	res, err := c.GetApprovedCampaigns(token, false)
	if err != nil {
		return nil
	}
	return res
}

type campaignsAPIResponse struct {
	Data []campaignRaw `json:"data"`
}

type campaignRaw struct {
	ID       any     `json:"id"`
	Name     string  `json:"name"`
	Merchant string  `json:"merchant"`
	Approval string  `json:"approval"`
	URL      *string `json:"url"`
	Status   *int64  `json:"status"`
}

func anyIDToString(v any) string {
	switch x := v.(type) {
	case string:
		return strings.TrimSpace(x)
	case float64:
		return strconv.FormatInt(int64(x), 10)
	case int64:
		return strconv.FormatInt(x, 10)
	default:
		return ""
	}
}

func isShopeeCampaign(mLower string) bool {
	switch mLower {
	case "shopee", "shopee_kolnew", "shopee_mcn", "shopee_addlostvn08", "shopee_kol", "shopee_cps", "shopee_smartlink":
		return true
	}
	return false
}

func isLazadaCampaign(mLower string) bool {
	switch mLower {
	case "lazada_kol", "lazada_cashback", "lazada", "lazada_cps", "lazada_vn":
		return true
	}
	return false
}

func isTikiCampaign(mLower string) bool {
	switch mLower {
	case "tikivn", "tiki_creator", "tiki_cashback", "tiki":
		return true
	}
	return false
}

func isTikTokCampaign(mLower, nLower string) bool {
	switch mLower {
	case "tiktok_cps", "tiktokshop_cps", "tiktok_shop":
		return true
	}
	return strings.Contains(mLower, "tiktok") && !strings.Contains(mLower, "business") && strings.Contains(nLower, "shop")
}

func isAlibabaCampaign(mLower, nLower string) bool {
	switch mLower {
	case "admitad_alibaba", "alibaba", "alibaba_cps":
		return true
	}
	return strings.Contains(mLower, "alibaba") && strings.Contains(nLower, "alibaba")
}

func isCoreMarketplaceDomain(host string) bool {
	h := strings.ToLower(strings.TrimSpace(host))
	for _, d := range []string{
		"shopee.vn", "shope.ee", "shp.ee",
		"lazada.vn", "lzd.co",
		"tiki.vn", "ti.ki",
		"tiktok.com", "alibaba.com",
	} {
		if h == d || strings.HasSuffix(h, "."+d) {
			return true
		}
	}
	return false
}

// ExtractNormalizedHost chuẩn hóa tên miền thuần từ URL (bỏ 1 tiền tố subdomain thông dụng).
func ExtractNormalizedHost(rawURL string) string {
	u, err := url.Parse(strings.TrimSpace(rawURL))
	if err != nil {
		return ""
	}
	if u.Scheme != "http" && u.Scheme != "https" {
		return ""
	}
	host := strings.ToLower(u.Hostname())
	if host == "" {
		return ""
	}
	for _, pref := range []string{"www.", "m.", "mobile.", "vn.", "vi.", "us.", "uk.", "get.", "shop.", "store."} {
		if strings.HasPrefix(host, pref) {
			stripped := host[len(pref):]
			if strings.Contains(stripped, ".") {
				switch stripped {
				case "com.vn", "net.vn", "org.vn", "edu.vn", "gov.vn", "co.uk", "com.au":
				default:
					host = stripped
				}
			}
			break
		}
	}
	return host
}

func resolveDynamicCampaignDomain(mLower, rawURL string) string {
	if mLower == "" {
		return ""
	}
	switch mLower {
	case "shopeepay_cpr", "shopeefood_taixe", "oneat_referral_firebase", "tiktok_business", "eco_comm":
		return ""
	}
	if strings.Contains(mLower, "fpt_software") || strings.Contains(mLower, "fptsoftware") ||
		strings.Contains(mLower, "cashloan") || strings.Contains(mLower, "_vay") ||
		strings.Contains(mLower, "vaytinchap") || strings.Contains(mLower, "thetindung") ||
		strings.Contains(mLower, "_credit") ||
		strings.HasSuffix(mLower, "_android") || strings.HasSuffix(mLower, "_ios") {
		return ""
	}

	switch mLower {
	case "adidas_vietnam":
		return "adidas.com.vn"
	case "aeonmall_eshop":
		return "aeoneshop.com"
	case "bitis.myharavan.com":
		return "bitis.com.vn"
	}

	if strings.TrimSpace(rawURL) == "" {
		return ""
	}
	host := ExtractNormalizedHost(rawURL)
	if host == "" || isCoreMarketplaceDomain(host) {
		return ""
	}
	blockedTrackers := []string{
		"accesstrade.vn", "caiapp.online", "pxf.io", "onelink.me", "page.link", "firebase.com",
		"play.google.com", "apps.apple.com", "itunes.apple.com", "shorten.asia", "go.isclix.com",
		"bit.ly", "tinyurl.com", "t.me", "facebook.com", "fb.com", "instagram.com", "zalo.me",
		"youtube.com", "google.com",
	}
	for _, b := range blockedTrackers {
		if host == b || strings.HasSuffix(host, "."+b) {
			return ""
		}
	}
	return host
}

func formatDynamicPlatformLabel(name, domain string) string {
	trimmed := strings.TrimSpace(name)
	if trimmed == "" {
		return domain
	}
	if utf8.RuneCountInString(trimmed) > 26 {
		if head, _, ok := strings.Cut(trimmed, " - "); ok && strings.TrimSpace(head) != "" {
			return strings.TrimSpace(head)
		}
		if head, _, ok := strings.Cut(trimmed, " _ "); ok && strings.TrimSpace(head) != "" {
			return strings.TrimSpace(head)
		}
	}
	return trimmed
}

// ParseApprovedCampaignsPage phân tích 1 trang JSON từ /v1/campaigns.
func ParseApprovedCampaignsPage(jsonStr string) (*ApprovedCampaigns, int, error) {
	var parsed campaignsAPIResponse
	if err := json.Unmarshal([]byte(jsonStr), &parsed); err != nil {
		return nil, 0, fmt.Errorf("không thể phân tích danh sách chiến dịch ACCESSTRADE: %w", err)
	}
	res := &ApprovedCampaigns{
		FetchedAtEpoch: time.Now().Unix(),
	}
	rawCount := len(parsed.Data)

	for _, c := range parsed.Data {
		statusVal := int64(0)
		if c.Status != nil {
			statusVal = *c.Status
		}
		if !strings.EqualFold(c.Approval, "successful") || statusVal != 1 {
			continue
		}
		idStr := anyIDToString(c.ID)
		if idStr == "" {
			continue
		}
		mLower := strings.ToLower(strings.TrimSpace(c.Merchant))
		nLower := strings.ToLower(strings.TrimSpace(c.Name))
		rawURL := ""
		if c.URL != nil {
			rawURL = *c.URL
		}

		platformGroup := "Khác"
		var dynDomain string

		switch {
		case isShopeeCampaign(mLower):
			platformGroup = "Shopee"
			if res.Shopee == nil || mLower == "shopee" || mLower == "shopee_kolnew" {
				res.Shopee = &CampaignPair{ID: idStr, Name: c.Name}
			}
		case isLazadaCampaign(mLower):
			platformGroup = "Lazada"
			if res.Lazada == nil || mLower == "lazada_kol" || mLower == "lazada_cashback" {
				res.Lazada = &CampaignPair{ID: idStr, Name: c.Name}
			}
		case isTikiCampaign(mLower):
			platformGroup = "Tiki"
			isPreferred := mLower == "tiki_creator" || idStr == TikiPreferredCampID
			curIsBroken := res.Tiki != nil && res.Tiki.ID == TikiBrokenCampID
			if res.Tiki == nil || isPreferred || (curIsBroken && idStr != TikiBrokenCampID) {
				res.Tiki = &CampaignPair{ID: idStr, Name: c.Name}
			}
		case isTikTokCampaign(mLower, nLower):
			platformGroup = "TikTok Shop"
			if res.TikTok == nil || mLower == "tiktok_cps" {
				res.TikTok = &CampaignPair{ID: idStr, Name: c.Name}
			}
		case isAlibabaCampaign(mLower, nLower):
			platformGroup = "Alibaba"
			if res.Alibaba == nil || mLower == "admitad_alibaba" {
				res.Alibaba = &CampaignPair{ID: idStr, Name: c.Name}
			}
		default:
			if d := resolveDynamicCampaignDomain(mLower, rawURL); d != "" {
				dynDomain = d
				platformGroup = formatDynamicPlatformLabel(c.Name, d)
			}
		}

		res.AllApproved = append(res.AllApproved, ApprovedCampaignSummary{
			ID:            idStr,
			Merchant:      c.Merchant,
			Name:          c.Name,
			PlatformGroup: platformGroup,
			Domain:        dynDomain,
		})
	}

	return res, rawCount, nil
}

func mergeApprovedCampaigns(into, page *ApprovedCampaigns) {
	if page.Shopee != nil && (into.Shopee == nil || page.Shopee.ID != "") {
		// giữ ưu tiên tên merchant
		if into.Shopee == nil || strings.Contains(strings.ToLower(page.Shopee.Name), "kol") {
			cp := *page.Shopee
			into.Shopee = &cp
		}
	}
	if page.Lazada != nil && into.Lazada == nil {
		cp := *page.Lazada
		into.Lazada = &cp
	}
	if page.Tiki != nil {
		isNewPref := page.Tiki.ID == TikiPreferredCampID || strings.Contains(strings.ToLower(page.Tiki.Name), "creator")
		curBroken := into.Tiki != nil && into.Tiki.ID == TikiBrokenCampID
		if into.Tiki == nil || isNewPref || (curBroken && page.Tiki.ID != TikiBrokenCampID) {
			cp := *page.Tiki
			into.Tiki = &cp
		}
	}
	if page.TikTok != nil && into.TikTok == nil {
		cp := *page.TikTok
		into.TikTok = &cp
	}
	if page.Alibaba != nil && into.Alibaba == nil {
		cp := *page.Alibaba
		into.Alibaba = &cp
	}

	seen := make(map[string]struct{}, len(into.AllApproved))
	for _, x := range into.AllApproved {
		seen[x.ID] = struct{}{}
	}
	for _, item := range page.AllApproved {
		if _, dup := seen[item.ID]; !dup {
			seen[item.ID] = struct{}{}
			into.AllApproved = append(into.AllApproved, item)
		}
	}
}

// GetApprovedCampaigns tải danh sách chiến dịch đã duyệt có singleflight & cache 3 phút.
func (c *AccessTradeClient) GetApprovedCampaigns(rawToken string, forceRefresh bool) (*ApprovedCampaigns, error) {
	token := CleanAPIToken(rawToken)
	if token == "" {
		return nil, errors.New("chưa cấu hình ACCESSTRADE_API_TOKEN")
	}
	now := time.Now().Unix()

	if !forceRefresh {
		c.muCamp.RLock()
		if c.campCache != nil && c.campCache.token == token && now-c.campCache.approved.FetchedAtEpoch < CacheTTLSecs {
			cp := *c.campCache.approved
			c.muCamp.RUnlock()
			return &cp, nil
		}
		c.muCamp.RUnlock()
	}

	c.campaignsFetchLock.Lock()
	defer c.campaignsFetchLock.Unlock()

	if !forceRefresh {
		c.muCamp.RLock()
		if c.campCache != nil && c.campCache.token == token && time.Now().Unix()-c.campCache.approved.FetchedAtEpoch < CacheTTLSecs {
			cp := *c.campCache.approved
			c.muCamp.RUnlock()
			return &cp, nil
		}
		c.muCamp.RUnlock()
	}

	combined := &ApprovedCampaigns{FetchedAtEpoch: time.Now().Unix()}
	for page := 1; page <= 10; page++ {
		apiURL := fmt.Sprintf("https://api.accesstrade.vn/v1/campaigns?approval=successful&limit=50&page=%d", page)
		status, bodyBytes, err := c.doATRequest(http.MethodGet, apiURL, token, nil)
		if err != nil {
			return nil, fmt.Errorf("không thể kết nối tới ACCESSTRADE API: %w", err)
		}
		bodyStr := string(bodyBytes)
		if status < 200 || status >= 300 {
			if strings.Contains(bodyStr, "IP") || strings.Contains(bodyStr, "whitelist") {
				return nil, fmt.Errorf("ACCESSTRADE từ chối IP (HTTP %d): Hãy kiểm tra cấu hình IP Whitelist hoặc Token API", status)
			}
			safeBody := strings.ReplaceAll(bodyStr, token, MaskToken(token))
			if len(safeBody) > 240 {
				safeBody = safeBody[:240] + "..."
			}
			return nil, fmt.Errorf("ACCESSTRADE API trả về HTTP %d: %s", status, utils.SanitizeCRLF(safeBody))
		}
		parsedPage, rawCount, err := ParseApprovedCampaignsPage(bodyStr)
		if err != nil {
			return nil, err
		}
		mergeApprovedCampaigns(combined, parsedPage)
		if rawCount < 50 {
			break
		}
	}

	c.muCamp.Lock()
	c.campCache = &campaignsCacheState{token: token, approved: combined}
	c.muCamp.Unlock()

	cp := *combined
	return &cp, nil
}

// VerifyAffiliateRedirectOK kiểm tra gateway tracking redirect có trả lỗi 4xx/5xx không.
func (c *AccessTradeClient) VerifyAffiliateRedirectOK(rawURL string) bool {
	u, err := url.Parse(strings.TrimSpace(rawURL))
	if err != nil {
		return false
	}
	if !isTrackingGatewayHost(u.Hostname()) {
		return true
	}
	req, err := http.NewRequest(http.MethodGet, rawURL, nil)
	if err != nil {
		return true
	}
	req.Header.Set("User-Agent", browserUA)
	resp, err := c.redirectProbe.Do(req)
	if err != nil {
		return true
	}
	_ = resp.Body.Close()
	return resp.StatusCode < 400
}

// VerifiedOfferURL trả link đích đã kiểm tra không chết gateway.
func (c *AccessTradeClient) VerifiedOfferURL(offer VoucherOffer, approved *ApprovedCampaigns) string {
	cand := offer.BestTargetURL(approved)
	if cand != offer.RawURL && !c.VerifyAffiliateRedirectOK(cand) {
		return offer.RawURL
	}
	return cand
}

type productLinkCreateReq struct {
	CampaignID  string   `json:"campaign_id"`
	URLs        []string `json:"urls"`
	URLEnc      bool     `json:"url_enc"`
	UTMSource   string   `json:"utm_source"`
	UTMMedium   string   `json:"utm_medium"`
	UTMCampaign string   `json:"utm_campaign"`
	Sub1        string   `json:"sub1"`
	Sub3        string   `json:"sub3"`
}

type productLinkCreateResp struct {
	Success *bool   `json:"success"`
	Message *string `json:"message"`
	Data    *struct {
		SuccessLink []struct {
			AffLink   *string `json:"aff_link"`
			ShortLink *string `json:"short_link"`
		} `json:"success_link"`
	} `json:"data"`
}

type tikTokFeedCreateReq struct {
	ProductURL  string `json:"product_url"`
	UTMSource   string `json:"utm_source"`
	UTMMedium   string `json:"utm_medium"`
	UTMCampaign string `json:"utm_campaign"`
	Sub1        string `json:"sub1"`
	Sub3        string `json:"sub3"`
}

type tikTokFeedCreateResp struct {
	Status *bool `json:"status"`
	Data   *struct {
		AffShortURL       *string `json:"aff_short_url"`
		AffURL            *string `json:"aff_url"`
		ProductName       *string `json:"product_name"`
		ProductPrice      *struct {
			Amount        any `json:"amount"`
			MinimumAmount any `json:"minimum_amount"`
		} `json:"product_price"`
		ProductCommission *struct {
			Amount        any `json:"amount"`
			MinimumAmount any `json:"minimum_amount"`
		} `json:"product_commission"`
	} `json:"data"`
}

func jsonAnyFloat(v any) *float64 {
	switch x := v.(type) {
	case float64:
		if x > 0 {
			cp := x
			return &cp
		}
	case int64:
		if x > 0 {
			cp := float64(x)
			return &cp
		}
	case string:
		if f, err := strconv.ParseFloat(strings.TrimSpace(x), 64); err == nil && f > 0 {
			return &f
		}
	}
	return nil
}

// CreateAffiliateLink tạo link Affiliate qua ACCESSTRADE (hỗ trợ TikTok Feed v2 & Product Link v1).
func (c *AccessTradeClient) CreateAffiliateLink(rawToken, platformName, overrideCampaignID, cleanURL, userIDStr, clickID string) (*AffiliateLinkResult, error) {
	token := CleanAPIToken(rawToken)
	if token == "" {
		return nil, errors.New("chưa cấu hình ACCESSTRADE_API_TOKEN")
	}

	campID := strings.TrimSpace(overrideCampaignID)
	if campID == "" {
		approved, err := c.GetApprovedCampaigns(token, false)
		if err != nil {
			return nil, err
		}
		switch platformName {
		case "Shopee":
			if approved.Shopee != nil {
				campID = approved.Shopee.ID
			}
		case "Lazada":
			if approved.Lazada != nil {
				campID = approved.Lazada.ID
			}
		case "Tiki":
			if approved.Tiki != nil {
				campID = approved.Tiki.ID
			}
		case "TikTokShop", "TikTok Shop":
			if approved.TikTok != nil {
				campID = approved.TikTok.ID
			}
		case "Alibaba":
			if approved.Alibaba != nil {
				campID = approved.Alibaba.ID
			}
		default:
			if dyn := approved.FindDynamicCampaignForURL(cleanURL); dyn != nil {
				campID = dyn.ID
			}
		}
	}
	if campID == "" {
		return nil, fmt.Errorf("chiến dịch %s chưa được duyệt trên tài khoản ACCESSTRADE của bạn", platformName)
	}

	if platformName == "TikTokShop" || platformName == "TikTok Shop" {
		if res, err := c.callV2TikTokFeed(token, campID, cleanURL, userIDStr, clickID); err == nil {
			if c.VerifyAffiliateRedirectOK(res.URL) {
				return res, nil
			}
		}
	}

	res, err := c.callV1ProductLink(token, campID, cleanURL, userIDStr, clickID)
	if err != nil {
		return nil, err
	}
	if !c.VerifyAffiliateRedirectOK(res.URL) {
		return nil, fmt.Errorf("gateway redirect ACCESSTRADE cho %s tạm lỗi HTTP 404/500 — tự chuyển về link gốc an toàn", platformName)
	}
	return res, nil
}

func (c *AccessTradeClient) callV2TikTokFeed(token, campID, productURL, userIDStr, clickID string) (*AffiliateLinkResult, error) {
	payload, _ := json.Marshal(tikTokFeedCreateReq{
		ProductURL:  productURL,
		UTMSource:   "facebook",
		UTMMedium:   "social",
		UTMCampaign: "dealx_bot",
		Sub1:        userIDStr,
		Sub3:        clickID,
	})
	status, data, err := c.doATRequest(http.MethodPost, "https://api.accesstrade.vn/v2/tiktokshop_product_feeds/create_link", token, payload)
	if err != nil {
		return nil, err
	}
	if status < 200 || status >= 300 {
		return nil, fmt.Errorf("TikTok Feed v2 HTTP %d", status)
	}
	var parsed tikTokFeedCreateResp
	if err := json.Unmarshal(data, &parsed); err != nil {
		return nil, err
	}
	if parsed.Status == nil || !*parsed.Status || parsed.Data == nil {
		return nil, errors.New("TikTok Feed v2 từ chối")
	}
	var targetURL string
	if parsed.Data.AffShortURL != nil && strings.TrimSpace(*parsed.Data.AffShortURL) != "" {
		targetURL = strings.TrimSpace(*parsed.Data.AffShortURL)
	} else if parsed.Data.AffURL != nil && strings.TrimSpace(*parsed.Data.AffURL) != "" {
		targetURL = strings.TrimSpace(*parsed.Data.AffURL)
	}
	if targetURL == "" {
		return nil, errors.New("TikTok Feed v2 không trả về URL")
	}
	var pname *string
	if parsed.Data.ProductName != nil && strings.TrimSpace(*parsed.Data.ProductName) != "" {
		v := strings.TrimSpace(*parsed.Data.ProductName)
		pname = &v
	}
	var priceVND, commVND *float64
	if parsed.Data.ProductPrice != nil {
		priceVND = jsonAnyFloat(parsed.Data.ProductPrice.MinimumAmount)
		if priceVND == nil {
			priceVND = jsonAnyFloat(parsed.Data.ProductPrice.Amount)
		}
	}
	if parsed.Data.ProductCommission != nil {
		commVND = jsonAnyFloat(parsed.Data.ProductCommission.Amount)
	}
	return &AffiliateLinkResult{
		URL:                 targetURL,
		CampaignID:          campID,
		ProductName:         pname,
		ProductPriceVND:     priceVND,
		CommissionAmountVND: commVND,
	}, nil
}

func (c *AccessTradeClient) callV1ProductLink(token, campID, cleanURL, userIDStr, clickID string) (*AffiliateLinkResult, error) {
	payload, _ := json.Marshal(productLinkCreateReq{
		CampaignID:  campID,
		URLs:        []string{cleanURL},
		URLEnc:      true,
		UTMSource:   "facebook",
		UTMMedium:   "social",
		UTMCampaign: "dealx_bot",
		Sub1:        userIDStr,
		Sub3:        clickID,
	})
	status, data, err := c.doATRequest(http.MethodPost, "https://api.accesstrade.vn/v1/product_link/create", token, payload)
	if err != nil {
		return nil, err
	}
	if status < 200 || status >= 300 {
		return nil, fmt.Errorf("v1/product_link/create trả về HTTP %d", status)
	}
	var parsed productLinkCreateResp
	if err := json.Unmarshal(data, &parsed); err != nil {
		return nil, err
	}
	if parsed.Success != nil && !*parsed.Success {
		msg := "ACCESSTRADE từ chối tạo link"
		if parsed.Message != nil && strings.TrimSpace(*parsed.Message) != "" {
			msg = strings.TrimSpace(*parsed.Message)
		}
		return nil, errors.New(msg)
	}
	if parsed.Data != nil {
		for _, item := range parsed.Data.SuccessLink {
			if item.ShortLink != nil && strings.TrimSpace(*item.ShortLink) != "" {
				return &AffiliateLinkResult{URL: strings.TrimSpace(*item.ShortLink), CampaignID: campID}, nil
			}
			if item.AffLink != nil && strings.TrimSpace(*item.AffLink) != "" {
				return &AffiliateLinkResult{URL: strings.TrimSpace(*item.AffLink), CampaignID: campID}, nil
			}
		}
	}
	return nil, errors.New("ACCESSTRADE không trả về success_link hợp lệ")
}

type offersAPIResponse struct {
	Data []offerRaw `json:"data"`
}

type offerRaw struct {
	Name               *string          `json:"name"`
	Content            *string          `json:"content"`
	Merchant           *string          `json:"merchant"`
	Domain             *string          `json:"domain"`
	Link               *string          `json:"link"`
	AffLink            *string          `json:"aff_link"`
	EndTime            *string          `json:"end_time"`
	DiscountPercentage any              `json:"discount_percentage"`
	DiscountValue      any              `json:"discount_value"`
	MinSpend           any              `json:"min_spend"`
	Coupons            []offerCouponRaw `json:"coupons"`
}

type offerCouponRaw struct {
	CouponCode *string `json:"coupon_code"`
	CouponDesc *string `json:"coupon_desc"`
}

func extractDiscountPercentText(txt string) (uint32, bool) {
	m := reDiscountPct.FindStringSubmatch(txt)
	if len(m) < 2 {
		return 0, false
	}
	v, err := strconv.ParseUint(m[1], 10, 32)
	if err != nil || v > 95 {
		return 0, false
	}
	return uint32(v), true
}

func parseMoneyWithUnit(numStr, rawUnit string) (float64, bool) {
	unit := strings.ToLower(strings.TrimSpace(rawUnit))
	switch unit {
	case "k":
		f, err := strconv.ParseFloat(strings.ReplaceAll(numStr, ",", "."), 64)
		return f * 1000.0, err == nil
	case "triệu", "tr":
		f, err := strconv.ParseFloat(strings.ReplaceAll(numStr, ",", "."), 64)
		return f * 1_000_000.0, err == nil
	default:
		var digits strings.Builder
		for i := 0; i < len(numStr); i++ {
			if numStr[i] >= '0' && numStr[i] <= '9' {
				digits.WriteByte(numStr[i])
			}
		}
		if digits.Len() == 0 {
			return 0, false
		}
		f, err := strconv.ParseFloat(digits.String(), 64)
		return f, err == nil
	}
}

func extractDiscountAmountText(txt string) (float64, bool) {
	m := reDiscountAmt.FindStringSubmatch(txt)
	if len(m) < 3 {
		return 0, false
	}
	return parseMoneyWithUnit(m[1], m[2])
}

func extractMinSpendText(txt string) (float64, bool) {
	m := reMinSpend.FindStringSubmatch(txt)
	if len(m) < 2 {
		return 0, false
	}
	unit := ""
	if len(m) >= 3 {
		unit = m[2]
	}
	return parseMoneyWithUnit(m[1], unit)
}

func detectOfferPlatform(merchant, domain, rawURL string) string {
	combined := strings.ToLower(merchant + " " + domain + " " + rawURL)
	switch {
	case strings.Contains(combined, "shopee"):
		return "Shopee"
	case strings.Contains(combined, "tiki"):
		return "Tiki"
	case strings.Contains(combined, "lazada"):
		return "Lazada"
	case strings.Contains(combined, "tiktok"):
		return "TikTok Shop"
	case strings.Contains(combined, "alibaba"):
		return "Alibaba"
	default:
		return "Khác"
	}
}

func splitShopAndDesc(rawName, platform string) (string, string) {
	trimmed := strings.TrimSpace(rawName)
	if strings.HasPrefix(trimmed, "[") {
		if idx := strings.IndexByte(trimmed, ']'); idx > 0 {
			shop := strings.TrimSpace(trimmed[1:idx])
			desc := strings.TrimSpace(strings.TrimLeft(trimmed[idx+1:], "-"))
			if desc == "" {
				desc = trimmed
			}
			if shop != "" {
				return shop, desc
			}
		}
	}
	return platform, trimmed
}

func firstNonEmptyLine(s string) string {
	for _, line := range strings.Split(s, "\n") {
		t := strings.TrimSpace(line)
		if t != "" {
			return t
		}
	}
	return ""
}

// ParseOffersInformationsPage phân tích JSON từ API /v1/offers_informations.
func ParseOffersInformationsPage(jsonStr, todayYMD string, minDiscountPct uint32) ([]VoucherOffer, int, error) {
	var parsed offersAPIResponse
	if err := json.Unmarshal([]byte(jsonStr), &parsed); err != nil {
		return nil, 0, fmt.Errorf("không thể phân tích kho voucher ACCESSTRADE: %w", err)
	}
	rawCount := len(parsed.Data)
	out := make([]VoucherOffer, 0, rawCount)

	for _, item := range parsed.Data {
		endLabel := "Đang hoạt động"
		if item.EndTime != nil {
			norm := strings.ReplaceAll(strings.TrimSpace(*item.EndTime), "/", "-")
			if len(norm) >= 10 {
				prefix := norm[:10]
				if prefix < todayYMD {
					continue
				}
				endLabel = prefix
			} else if norm != "" {
				endLabel = norm
			}
		}

		rawURL := ""
		if item.Link != nil {
			rawURL = strings.TrimSpace(*item.Link)
		}
		if !strings.HasPrefix(rawURL, "https://") && !strings.HasPrefix(rawURL, "http://") {
			continue
		}

		var affURL *string
		if item.AffLink != nil {
			a := strings.TrimSpace(*item.AffLink)
			if strings.HasPrefix(a, "https://") || strings.HasPrefix(a, "http://") {
				affURL = &a
			}
		}

		rawName := ""
		if item.Name != nil {
			rawName = strings.TrimSpace(*item.Name)
		}
		rawContent := ""
		if item.Content != nil {
			rawContent = strings.TrimSpace(*item.Content)
		}
		merchant := ""
		if item.Merchant != nil {
			merchant = *item.Merchant
		}
		domain := ""
		if item.Domain != nil {
			domain = *item.Domain
		}

		platform := detectOfferPlatform(merchant, domain, rawURL)
		shopName, cleanDesc := splitShopAndDesc(rawName, platform)

		apiPct := uint32(0)
		if fPtr := jsonAnyFloat(item.DiscountPercentage); fPtr != nil {
			apiPct = uint32(math.Round(*fPtr))
		}
		apiDiscVal := 0.0
		if fPtr := jsonAnyFloat(item.DiscountValue); fPtr != nil {
			apiDiscVal = *fPtr
		}
		apiMinSpend := 0.0
		if fPtr := jsonAnyFloat(item.MinSpend); fPtr != nil {
			apiMinSpend = *fPtr
		}

		if len(item.Coupons) == 0 {
			pct := uint32(0)
			if apiPct >= 1 && apiPct <= 100 {
				pct = apiPct
			} else if p, ok := extractDiscountPercentText(rawName); ok {
				pct = p
			} else if p, ok := extractDiscountPercentText(rawContent); ok {
				pct = p
			}
			if pct < minDiscountPct {
				continue
			}

			discAmt := apiDiscVal
			if discAmt <= 0 {
				if a, ok := extractDiscountAmountText(rawName); ok {
					discAmt = a
				} else if a, ok := extractDiscountAmountText(rawContent); ok {
					discAmt = a
				}
			}
			minSpend := apiMinSpend
			if minSpend <= 0 {
				if ms, ok := extractMinSpendText(rawName); ok {
					minSpend = ms
				} else if ms, ok := extractMinSpendText(rawContent); ok {
					minSpend = ms
				}
			}
			desc := cleanDesc
			if desc == "" {
				desc = firstNonEmptyLine(rawContent)
			}
			if desc == "" {
				desc = "Ưu đãi trực tiếp"
			}
			out = append(out, VoucherOffer{
				Platform:        platform,
				ShopName:        shopName,
				Title:           rawName,
				CouponCode:      "TỰ ĐỘNG ÁP",
				DiscountPercent: pct,
				DiscountAmount:  discAmt,
				DiscountDesc:    desc,
				MinSpend:        minSpend,
				RawURL:          rawURL,
				AffURL:          affURL,
				EndTime:         endLabel,
			})
			continue
		}

		for _, c := range item.Coupons {
			codeFinal := "TỰ ĐỘNG ÁP"
			if c.CouponCode != nil && strings.TrimSpace(*c.CouponCode) != "" {
				codeFinal = strings.TrimSpace(*c.CouponCode)
			}
			descCandidate := cleanDesc
			if c.CouponDesc != nil && strings.TrimSpace(*c.CouponDesc) != "" {
				descCandidate = strings.TrimSpace(*c.CouponDesc)
			}

			pct := uint32(0)
			if apiPct >= 1 && apiPct <= 100 {
				pct = apiPct
			} else if p, ok := extractDiscountPercentText(descCandidate); ok {
				pct = p
			} else if p, ok := extractDiscountPercentText(rawName); ok {
				pct = p
			} else if p, ok := extractDiscountPercentText(rawContent); ok {
				pct = p
			}
			if pct < minDiscountPct {
				continue
			}

			discAmt := apiDiscVal
			if discAmt <= 0 {
				if a, ok := extractDiscountAmountText(descCandidate); ok {
					discAmt = a
				} else if a, ok := extractDiscountAmountText(rawName); ok {
					discAmt = a
				} else if a, ok := extractDiscountAmountText(rawContent); ok {
					discAmt = a
				}
			}
			minSpend := apiMinSpend
			if minSpend <= 0 {
				if ms, ok := extractMinSpendText(rawName); ok {
					minSpend = ms
				} else if ms, ok := extractMinSpendText(descCandidate); ok {
					minSpend = ms
				} else if ms, ok := extractMinSpendText(rawContent); ok {
					minSpend = ms
				}
			}

			out = append(out, VoucherOffer{
				Platform:        platform,
				ShopName:        shopName,
				Title:           rawName,
				CouponCode:      codeFinal,
				DiscountPercent: pct,
				DiscountAmount:  discAmt,
				DiscountDesc:    descCandidate,
				MinSpend:        minSpend,
				RawURL:          rawURL,
				AffURL:          affURL,
				EndTime:         endLabel,
			})
		}
	}

	return out, rawCount, nil
}

func sortVoucherOffersPriority(list []VoucherOffer) []VoucherOffer {
	sort.SliceStable(list, func(i, j int) bool {
		a, b := list[i], list[j]
		aTier := 0
		if a.DiscountPercent >= 20 {
			aTier = 2
		} else if a.DiscountPercent > 0 {
			aTier = 1
		}
		bTier := 0
		if b.DiscountPercent >= 20 {
			bTier = 2
		} else if b.DiscountPercent > 0 {
			bTier = 1
		}
		if aTier != bTier {
			return aTier > bTier
		}
		if a.DiscountPercent != b.DiscountPercent {
			return a.DiscountPercent > b.DiscountPercent
		}
		if a.DiscountAmount != b.DiscountAmount {
			return a.DiscountAmount > b.DiscountAmount
		}
		if a.MinSpend != b.MinSpend {
			return a.MinSpend < b.MinSpend
		}
		return a.ShopName < b.ShopName
	})
	if len(list) > 1500 {
		return list[:1500]
	}
	return list
}

// FetchHighDiscountVouchers trả về danh sách voucher toàn thị trường (dùng cache 55s nếu không ép quét).
func (c *AccessTradeClient) FetchHighDiscountVouchers(rawToken string, minDiscountPct uint32, forceRefresh bool) ([]VoucherOffer, error) {
	token := CleanAPIToken(rawToken)
	if token == "" {
		return nil, errors.New("chưa cấu hình ACCESSTRADE_API_TOKEN")
	}
	now := time.Now().Unix()

	if !forceRefresh {
		c.muVoucher.RLock()
		if c.voucherCache != nil && c.voucherCache.token == token && now-c.voucherCache.fetchedAt < VoucherCacheTTLSecs && c.voucherCache.minPct <= minDiscountPct {
			out := make([]VoucherOffer, 0, len(c.voucherCache.list))
			for _, v := range c.voucherCache.list {
				if v.DiscountPercent >= minDiscountPct {
					out = append(out, v)
				}
			}
			c.muVoucher.RUnlock()
			return out, nil
		}
		c.muVoucher.RUnlock()
	}

	fullList, err := c.syncMarketVouchersInternal(token, minDiscountPct, forceRefresh)
	if err != nil {
		return nil, err
	}
	out := make([]VoucherOffer, 0, len(fullList))
	for _, v := range fullList {
		if v.DiscountPercent >= minDiscountPct {
			out = append(out, v)
		}
	}
	return out, nil
}

// SyncMarketVouchersTick đồng bộ kho mã định kỳ nền.
func (c *AccessTradeClient) SyncMarketVouchersTick(rawToken string, fullScan bool) (int, error) {
	token := CleanAPIToken(rawToken)
	if token == "" {
		return 0, errors.New("chưa cấu hình ACCESSTRADE_API_TOKEN")
	}
	list, err := c.syncMarketVouchersInternal(token, 0, fullScan)
	if err != nil {
		return 0, err
	}
	return len(list), nil
}

func (c *AccessTradeClient) syncMarketVouchersInternal(token string, minDiscountPct uint32, fullScan bool) ([]VoucherOffer, error) {
	todayYMD := vnTodayYMD()

	type epGroup struct {
		url      string
		maxPages int
	}
	var endpoints []epGroup
	if fullScan {
		endpoints = []epGroup{
			{"https://api.accesstrade.vn/v1/offers_informations/coupon?limit=50", 3},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&is_hot=1&limit=50", 2},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&scope=expiring&limit=50", 2},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&domain=shopee.vn&limit=50", 3},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiki.vn&limit=50", 3},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&domain=lazada.vn&limit=50", 2},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiktok.com&limit=50", 2},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&limit=50", 10},
		}
	} else {
		endpoints = []epGroup{
			{"https://api.accesstrade.vn/v1/offers_informations/coupon?limit=50", 1},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&is_hot=1&limit=50", 1},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&domain=shopee.vn&limit=50", 1},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiki.vn&limit=50", 1},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&domain=lazada.vn&limit=50", 1},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&domain=tiktok.com&limit=50", 1},
			{"https://api.accesstrade.vn/v1/offers_informations?status=1&limit=50", 2},
		}
	}

	type offerKey struct {
		plat, code, shop string
	}
	collected := make([]VoucherOffer, 0, 256)
	seen := make(map[offerKey]struct{})

	for gIdx, grp := range endpoints {
		for page := 1; page <= grp.maxPages; page++ {
			apiURL := fmt.Sprintf("%s&page=%d", grp.url, page)
			status, bodyBytes, err := c.doATRequest(http.MethodGet, apiURL, token, nil)
			if err == nil && status == 429 {
				time.Sleep(1200 * time.Millisecond)
				status, bodyBytes, err = c.doATRequest(http.MethodGet, apiURL, token, nil)
			}
			if err != nil || status < 200 || status >= 300 {
				break
			}
			offers, rawCount, parseErr := ParseOffersInformationsPage(string(bodyBytes), todayYMD, minDiscountPct)
			if parseErr != nil {
				break
			}
			for _, ov := range offers {
				k := offerKey{
					plat: strings.ToLower(ov.Platform),
					code: strings.ToUpper(ov.CouponCode),
					shop: strings.ToLower(ov.ShopName),
				}
				if _, dup := seen[k]; !dup {
					seen[k] = struct{}{}
					collected = append(collected, ov)
				}
			}
			if rawCount < 50 || len(collected) >= 1500 {
				break
			}
			time.Sleep(350 * time.Millisecond)
		}

		if gIdx == 0 && len(collected) > 0 {
			c.muVoucher.Lock()
			isEmpty := c.voucherCache == nil || len(c.voucherCache.list) == 0
			if isEmpty {
				partial := make([]VoucherOffer, len(collected))
				copy(partial, collected)
				partial = sortVoucherOffersPriority(partial)
				c.voucherCache = &cachedVoucherPool{
					token:     token,
					fetchedAt: time.Now().Unix(),
					minPct:    minDiscountPct,
					list:      partial,
				}
				c.muMeta.Lock()
				c.voucherMeta.LastSyncVN = vnNowStr() + " (Đang quét tiếp...)"
				c.voucherMeta.TotalCount = len(partial)
				c.muMeta.Unlock()
			}
			c.muVoucher.Unlock()
		}
		if len(collected) >= 1500 {
			break
		}
	}

	if !fullScan {
		c.muVoucher.RLock()
		if c.voucherCache != nil && c.voucherCache.token == token {
			for _, oldOv := range c.voucherCache.list {
				exp := strings.ReplaceAll(strings.TrimSpace(oldOv.EndTime), "/", "-")
				if len(exp) == 10 && exp < todayYMD {
					continue
				}
				k := offerKey{
					plat: strings.ToLower(oldOv.Platform),
					code: strings.ToUpper(oldOv.CouponCode),
					shop: strings.ToLower(oldOv.ShopName),
				}
				if _, dup := seen[k]; !dup {
					seen[k] = struct{}{}
					collected = append(collected, oldOv)
				}
			}
		}
		c.muVoucher.RUnlock()
	}

	collected = sortVoucherOffersPriority(collected)
	if len(collected) > 0 {
		c.muVoucher.Lock()
		c.voucherCache = &cachedVoucherPool{
			token:     token,
			fetchedAt: time.Now().Unix(),
			minPct:    minDiscountPct,
			list:      collected,
		}
		c.muVoucher.Unlock()

		c.muMeta.Lock()
		c.voucherMeta.LastSyncVN = vnNowStr()
		c.voucherMeta.SyncCycles++
		c.voucherMeta.TotalCount = len(collected)
		c.muMeta.Unlock()
		slog.Info("✅ Đã cập nhật kho mã ACCESSTRADE", "count", len(collected))
	}

	return collected, nil
}

// IsGenericPlatformVoucherShop kiểm tra shop_name có phải nhãn chung toàn sàn hay không.
func IsGenericPlatformVoucherShop(shopLower, platformLower string) bool {
	t := strings.TrimSpace(shopLower)
	if t == "" || t == platformLower || strings.HasPrefix(t, "toàn sàn ") {
		return true
	}
	switch t {
	case "tiki", "tikinow", "toàn sàn", "toàn sàn tiki", "toàn sàn shopee", "toàn sàn lazada",
		"shopee", "shopee việt nam", "shopee mall", "mã sàn shopee", "voucher xtra",
		"freeship xtra", "shopee video", "shopee live", "lazada", "lazmall", "tiktok",
		"tiktok shop", "khác":
		return true
	default:
		return false
	}
}

// SearchMatchingVouchersForDeal tìm mã trong RAM cache khớp đúng sản phẩm/shop/thương hiệu.
func (c *AccessTradeClient) SearchMatchingVouchersForDeal(q VoucherMatchQuery) []VoucherOffer {
	offers := c.PeekCachedHighDiscountVouchers()
	return MatchVouchersForProduct(offers, q)
}

// MatchVouchersForProduct lọc và chấm điểm danh sách voucher cho 1 sản phẩm.
func MatchVouchersForProduct(offers []VoucherOffer, q VoucherMatchQuery) []VoucherOffer {
	if len(offers) == 0 || q.MaxItems <= 0 {
		return nil
	}
	platLower := strings.ToLower(strings.TrimSpace(q.Platform))
	titleLower := strings.ToLower(strings.TrimSpace(q.ProductTitle))
	urlLower := strings.ToLower(strings.TrimSpace(q.CleanURL))
	prodShopLower := ""
	if q.ShopName != nil {
		prodShopLower = strings.ToLower(strings.TrimSpace(*q.ShopName))
	}
	prodBrandLower := ""
	if q.BrandName != nil {
		prodBrandLower = strings.ToLower(strings.TrimSpace(*q.BrandName))
	}

	type scored struct {
		tier          int
		priceEligible int
		pct           uint32
		amtInt        int64
		offer         VoucherOffer
	}
	var list []scored

	for _, v := range offers {
		if !strings.EqualFold(v.Platform, q.Platform) {
			continue
		}
		vShopLower := strings.ToLower(strings.TrimSpace(v.ShopName))
		vTitleLower := strings.ToLower(strings.TrimSpace(v.Title))
		isGeneric := IsGenericPlatformVoucherShop(vShopLower, platLower)

		exactShopMatch := !isGeneric &&
			prodShopLower != "" &&
			utf8.RuneCountInString(vShopLower) >= 3 &&
			(vShopLower == prodShopLower || strings.Contains(prodShopLower, vShopLower) || strings.Contains(vShopLower, prodShopLower))

		brandMatch := false
		if !isGeneric {
			if prodBrandLower != "" && utf8.RuneCountInString(prodBrandLower) >= 3 &&
				(strings.Contains(vShopLower, prodBrandLower) || strings.Contains(vTitleLower, prodBrandLower)) {
				brandMatch = true
			}
			if !brandMatch && utf8.RuneCountInString(vShopLower) >= 4 {
				compactShop := strings.ReplaceAll(vShopLower, " ", "")
				if strings.Contains(titleLower, vShopLower) || (compactShop != "" && strings.Contains(urlLower, compactShop)) {
					brandMatch = true
				}
			}
		}

		tier := 0
		if exactShopMatch {
			tier = 3
		} else if brandMatch {
			tier = 2
		} else if isGeneric {
			tier = 1
		}
		if tier == 0 {
			continue
		}

		priceElig := 1
		if q.PriceVND != nil && *q.PriceVND > 0 && v.MinSpend > 0 && *q.PriceVND < v.MinSpend {
			priceElig = 0
		}

		list = append(list, scored{
			tier:          tier,
			priceEligible: priceElig,
			pct:           v.DiscountPercent,
			amtInt:        int64(math.Round(v.DiscountAmount)),
			offer:         v,
		})
	}

	sort.SliceStable(list, func(i, j int) bool {
		a, b := list[i], list[j]
		if a.tier != b.tier {
			return a.tier > b.tier
		}
		if a.priceEligible != b.priceEligible {
			return a.priceEligible > b.priceEligible
		}
		if a.pct != b.pct {
			return a.pct > b.pct
		}
		return a.amtInt > b.amtInt
	})

	seenCodes := make(map[string]struct{})
	out := make([]VoucherOffer, 0, q.MaxItems)
	for _, s := range list {
		if len(out) >= q.MaxItems {
			break
		}
		codeKey := strings.ToUpper(strings.TrimSpace(s.offer.CouponCode))
		if codeKey != "" {
			if _, dup := seenCodes[codeKey]; dup {
				continue
			}
			seenCodes[codeKey] = struct{}{}
		}
		out = append(out, s.offer)
	}
	return out
}

// FetchRecentOrders tải danh sách giao dịch / đơn hàng gần đây từ ACCESSTRADE.
func (c *AccessTradeClient) FetchRecentOrders(rawToken string, daysBack int64) ([]db.TrackedOrder, error) {
	token := CleanAPIToken(rawToken)
	if token == "" {
		return nil, errors.New("chưa cấu hình ACCESSTRADE_API_TOKEN")
	}
	clamped := daysBack
	if clamped < 1 {
		clamped = 1
	} else if clamped > 30 {
		clamped = 30
	}
	now := time.Now().UTC()
	since := now.Add(-time.Duration(clamped) * 24 * time.Hour)
	sinceEnc := utils.PercentEncodeRFC3986(since.Format("2006-01-02T15:04:05Z"))
	untilEnc := utils.PercentEncodeRFC3986(now.Format("2006-01-02T15:04:05Z"))

	txURL := fmt.Sprintf("https://api.accesstrade.vn/v1/transactions?since=%s&until=%s&limit=50", sinceEnc, untilEnc)
	status, data, err := c.doATRequest(http.MethodGet, txURL, token, nil)
	if err != nil {
		return nil, fmt.Errorf("lỗi kết nối ACCESSTRADE orders API: %w", err)
	}
	if status == 404 {
		ordURL := fmt.Sprintf("https://api.accesstrade.vn/v1/orders?since=%s&until=%s&limit=50", sinceEnc, untilEnc)
		status, data, err = c.doATRequest(http.MethodGet, ordURL, token, nil)
		if err != nil {
			return nil, fmt.Errorf("lỗi kết nối ACCESSTRADE orders fallback API: %w", err)
		}
	}
	if status < 200 || status >= 300 {
		return nil, fmt.Errorf("ACCESSTRADE orders API trả HTTP %d", status)
	}
	return ParseAccessTradeOrdersJSON(string(data))
}

func normalizeOrderPlatform(raw string) string {
	t := strings.TrimSpace(raw)
	if t == "" {
		return "ACCESSTRADE"
	}
	low := strings.ToLower(t)
	switch {
	case strings.Contains(low, "shopee"):
		return "Shopee"
	case strings.Contains(low, "tiktok"):
		return "TikTok Shop"
	case strings.Contains(low, "lazada"):
		return "Lazada"
	case strings.Contains(low, "tiki"):
		return "Tiki"
	case strings.Contains(low, "alibaba"):
		return "Alibaba"
	default:
		return t
	}
}

func normalizeOrderStatusVal(v any) string {
	switch x := v.(type) {
	case float64:
		switch int64(x) {
		case 0:
			return "PENDING"
		case 1:
			return "APPROVED"
		case 2:
			return "REJECTED"
		default:
			return fmt.Sprintf("STATUS_%d", int64(x))
		}
	case string:
		low := strings.ToLower(strings.TrimSpace(x))
		switch low {
		case "0", "pending", "processing":
			return "PENDING"
		case "1", "approved", "completed":
			return "APPROVED"
		case "2", "rejected", "cancelled", "canceled":
			return "REJECTED"
		case "":
			return "PENDING"
		default:
			return strings.ToUpper(strings.TrimSpace(x))
		}
	default:
		return "PENDING"
	}
}

type atOrdersEnvelope struct {
	Data []atOrderRaw `json:"data"`
}

type atOrderRaw struct {
	TransactionID    any     `json:"transaction_id"`
	OrderID          any     `json:"order_id"`
	ID               any     `json:"id"`
	Merchant         *string `json:"merchant"`
	CampaignName     *string `json:"campaign_name"`
	Status           any     `json:"status"`
	OrderStatus      any     `json:"order_status"`
	IsBrandBonus     *bool   `json:"is_brand_bonus"`
	ProductCategory  *string `json:"product_category"`
	TransactionValue any     `json:"transaction_value"`
	Billing          any     `json:"billing"`
	TotalAmount      any     `json:"total_amount"`
	Amount           any     `json:"amount"`
	ProductPrice     any     `json:"product_price"`
	PubCommission    any     `json:"pub_commission"`
	Commission       any     `json:"commission"`
	UtmSub1          *string `json:"utm_sub1"`
	Sub1             *string `json:"sub1"`
	UtmCampaign      *string `json:"utm_campaign"`
	Extra            *struct {
		SubParams *struct {
			Sub1 *string `json:"sub1"`
		} `json:"sub_params"`
	} `json:"_extra"`
	TransactionTime *string `json:"transaction_time"`
	SalesTime       *string `json:"sales_time"`
	ClickTime       *string `json:"click_time"`
	ConfirmedTime   *string `json:"confirmed_time"`
}

func firstNonEmptyStrPtr(ptrs ...*string) string {
	for _, p := range ptrs {
		if p != nil && strings.TrimSpace(*p) != "" {
			return strings.TrimSpace(*p)
		}
	}
	return ""
}

// ParseAccessTradeOrdersJSON phân tích danh sách đơn hàng từ ACCESSTRADE và gộp trùng theo order_id.
func ParseAccessTradeOrdersJSON(body string) ([]db.TrackedOrder, error) {
	var env atOrdersEnvelope
	if err := json.Unmarshal([]byte(body), &env); err != nil {
		return nil, fmt.Errorf("lỗi giải mã JSON ACCESSTRADE orders: %w", err)
	}
	nowStr := time.Now().UTC().Format(time.RFC3339)

	orders := make([]db.TrackedOrder, 0, len(env.Data))
	hasNonBonus := make([]bool, 0, len(env.Data))
	indexByKey := make(map[string]int, len(env.Data))

	for _, item := range env.Data {
		orderID := anyIDToString(item.TransactionID)
		if orderID == "" {
			orderID = anyIDToString(item.OrderID)
		}
		if orderID == "" {
			orderID = anyIDToString(item.ID)
		}
		if orderID == "" {
			continue
		}

		platRaw := firstNonEmptyStrPtr(item.Merchant, item.CampaignName)
		platform := normalizeOrderPlatform(platRaw)

		statusVal := item.Status
		if statusVal == nil {
			statusVal = item.OrderStatus
		}
		status := normalizeOrderStatusVal(statusVal)

		isBonus := (item.IsBrandBonus != nil && *item.IsBrandBonus) ||
			(item.ProductCategory != nil && strings.EqualFold(strings.TrimSpace(*item.ProductCategory), "bonus"))

		orderAmount := 0.0
		for _, v := range []any{item.TransactionValue, item.Billing, item.TotalAmount, item.Amount, item.ProductPrice} {
			if fPtr := jsonAnyFloat(v); fPtr != nil {
				orderAmount = *fPtr
				break
			}
		}

		commission := 0.0
		for _, v := range []any{item.PubCommission, item.Commission} {
			if fPtr := jsonAnyFloat(v); fPtr != nil {
				commission = *fPtr
				break
			}
		}

		var extraSub1 *string
		if item.Extra != nil && item.Extra.SubParams != nil {
			extraSub1 = item.Extra.SubParams.Sub1
		}
		subID := firstNonEmptyStrPtr(item.UtmSub1, item.Sub1, extraSub1, item.UtmCampaign)
		orderTime := firstNonEmptyStrPtr(item.TransactionTime, item.SalesTime, item.ClickTime, item.ConfirmedTime)

		key := "AT:" + orderID
		if idx, exists := indexByKey[key]; exists {
			orders[idx].Commission += commission
			if !isBonus {
				if hasNonBonus[idx] {
					orders[idx].OrderAmount += orderAmount
				} else {
					orders[idx].OrderAmount = orderAmount
					hasNonBonus[idx] = true
				}
			}
			if orders[idx].SubID == "" && subID != "" {
				orders[idx].SubID = subID
			}
			if orders[idx].OrderTime == "" && orderTime != "" {
				orders[idx].OrderTime = orderTime
			}
			if status == "APPROVED" || (orders[idx].Status == "PENDING" && status != "PENDING") {
				orders[idx].Status = status
			}
		} else {
			indexByKey[key] = len(orders)
			hasNonBonus = append(hasNonBonus, !isBonus)
			orders = append(orders, db.TrackedOrder{
				OrderKey:    key,
				Platform:    platform,
				OrderID:     orderID,
				Status:      status,
				OrderAmount: orderAmount,
				Commission:  commission,
				SubID:       subID,
				OrderTime:   orderTime,
				NotifiedAt:  nowStr,
			})
		}
	}

	return orders, nil
}
