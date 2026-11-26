package shopee

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"salehunter-go/internal/db"
	"salehunter-go/internal/utils"
)

const openAPIEndpoint = "https://open-api.affiliate.shopee.vn/graphql"

// SocialPassthroughKeys thứ tự chuẩn xác các tham số phiên FB/IG Reels của Shopee.
var SocialPassthroughKeys = []string{
	"fb_content_id",
	"encrypted_payload",
	"fbclid",
	"cts",
	"channel_type",
	"content_source",
	"content_type",
	"sm_sig_id",
	"gads_t_sig",
}

// SHA256Hex trả về chuỗi SHA-256 hex viết thường 64 ký tự.
func SHA256Hex(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// IsValidShopeeAffiliateID kiểm tra mã Affiliate ID Shopee (5-20 chữ số ASCII).
func IsValidShopeeAffiliateID(id string) bool {
	t := strings.TrimSpace(id)
	if len(t) < 5 || len(t) > 20 {
		return false
	}
	for i := 0; i < len(t); i++ {
		if t[i] < '0' || t[i] > '9' {
			return false
		}
	}
	return true
}

// BuildDirectAnRedirLink dựng link s.shopee.vn/an_redir trực tiếp không qua gateway trung gian.
func BuildDirectAnRedirLink(cleanURL, affiliateID, subID string) string {
	affTrim := strings.TrimSpace(affiliateID)
	encOrigin := utils.PercentEncodeRFC3986(strings.TrimSpace(cleanURL))
	encSub := utils.PercentEncodeRFC3986(strings.TrimSpace(subID))
	return fmt.Sprintf("https://s.shopee.vn/an_redir?origin_link=%s&affiliate_id=%s&sub_id=%s", encOrigin, affTrim, encSub)
}

type shopeeGraphQLReq struct {
	Query     string               `json:"query"`
	Variables shopeeShortLinkVars  `json:"variables"`
}

type shopeeShortLinkVars struct {
	Input shopeeShortLinkInput `json:"input"`
}

type shopeeShortLinkInput struct {
	OriginURL string   `json:"originUrl"`
	SubIDs    []string `json:"subIds"`
}

type shopeeGraphQLResp struct {
	Data   *shopeeGraphQLData   `json:"data"`
	Errors []shopeeGraphQLError `json:"errors"`
}

type shopeeGraphQLData struct {
	GenerateShortLink *shopeeShortLinkResult `json:"generateShortLink"`
}

type shopeeShortLinkResult struct {
	ShortLink *string `json:"shortLink"`
}

type shopeeGraphQLError struct {
	Message *string `json:"message"`
}

type shopeeReportReq struct {
	Query string `json:"query"`
}

// GenerateOpenAPIShortLink gọi Shopee Affiliate Open API để ký SHA256 và tạo shortLink.
func GenerateOpenAPIShortLink(client *http.Client, appID, appSecret, originURL string, subIDs []string) (string, error) {
	appIDClean := strings.TrimSpace(appID)
	secretClean := strings.TrimSpace(appSecret)
	if appIDClean == "" || secretClean == "" {
		return "", errors.New("thiếu App ID hoặc Secret Key của Shopee Open API")
	}

	reqBody := shopeeGraphQLReq{
		Query: "mutation($input: ShortLinkInput!) { generateShortLink(input: $input) { shortLink } }",
		Variables: shopeeShortLinkVars{
			Input: shopeeShortLinkInput{
				OriginURL: strings.TrimSpace(originURL),
				SubIDs:    subIDs,
			},
		},
	}
	payloadBytes, err := json.Marshal(reqBody)
	if err != nil {
		return "", err
	}
	payloadJSON := string(payloadBytes)

	ts := time.Now().UTC().Unix()
	signPayload := fmt.Sprintf("%s%d%s%s", appIDClean, ts, payloadJSON, secretClean)
	sig := SHA256Hex([]byte(signPayload))
	authHeader := fmt.Sprintf("SHA256 Credential=%s, Timestamp=%d, Signature=%s", appIDClean, ts, sig)

	req, err := http.NewRequest(http.MethodPost, openAPIEndpoint, bytes.NewReader(payloadBytes))
	if err != nil {
		return "", err
	}
	req.Header.Set("Authorization", authHeader)
	req.Header.Set("Content-Type", "application/json")

	resp, err := client.Do(req)
	if err != nil {
		return "", fmt.Errorf("lỗi kết nối Shopee Open API: %w", err)
	}
	defer resp.Body.Close()

	bodyBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 2*1024*1024))
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return "", fmt.Errorf("Shopee Open API trả HTTP %d: %s", resp.StatusCode, string(bodyBytes))
	}

	var parsed shopeeGraphQLResp
	if err := json.Unmarshal(bodyBytes, &parsed); err != nil {
		return "", fmt.Errorf("lỗi phân tích phản hồi Shopee Open API: %w", err)
	}
	for _, e := range parsed.Errors {
		if e.Message != nil && strings.TrimSpace(*e.Message) != "" {
			return "", errors.New(strings.TrimSpace(*e.Message))
		}
	}
	if len(parsed.Errors) > 0 {
		return "", errors.New("Shopee Open API từ chối yêu cầu")
	}
	if parsed.Data != nil && parsed.Data.GenerateShortLink != nil && parsed.Data.GenerateShortLink.ShortLink != nil {
		link := strings.TrimSpace(*parsed.Data.GenerateShortLink.ShortLink)
		if strings.HasPrefix(link, "https://") {
			return link, nil
		}
	}
	return "", errors.New("Shopee Open API không trả về shortLink hợp lệ")
}

// NormalizeEscapedURLStr giải mã các ký tự escape thường gặp trong HTML/JS trước khi parse URL.
func NormalizeEscapedURLStr(raw string) string {
	r := strings.NewReplacer(
		`\/`, `/`,
		`\u0026`, `&`,
		`\u003d`, `=`,
		`\u003D`, `=`,
		`&amp;`, `&`,
	)
	return r.Replace(strings.TrimSpace(raw))
}

// ExtractOriginLinkFromAnRedir trích link đích bên trong tham số origin_link của s.shopee.vn/an_redir.
func ExtractOriginLinkFromAnRedir(rawURL string) (string, bool) {
	norm := NormalizeEscapedURLStr(rawURL)
	u, err := url.Parse(norm)
	if err != nil {
		return "", false
	}
	host := strings.ToLower(u.Hostname())
	if !strings.Contains(host, "shopee.vn") || !strings.HasPrefix(u.Path, "/an_redir") {
		return "", false
	}
	for k, vals := range u.Query() {
		if k == "origin_link" && len(vals) > 0 {
			target := strings.TrimSpace(vals[0])
			if idx := strings.IndexByte(target, '#'); idx >= 0 {
				target = target[:idx]
			}
			if strings.HasPrefix(target, "https://") || strings.HasPrefix(target, "http://") {
				return target, true
			}
		}
	}
	return "", false
}

func isSafeSocialParamValue(val string) bool {
	if val == "" {
		return false
	}
	for i := 0; i < len(val); i++ {
		c := val[i]
		ok := (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') ||
			c == '-' || c == '_' || c == '.' || c == '~' || c == '%'
		if !ok {
			return false
		}
	}
	return true
}

func isSocialPassthroughKey(k string) bool {
	for _, allowed := range SocialPassthroughKeys {
		if k == allowed {
			return true
		}
	}
	return false
}

// ExtractSocialPassthroughParams trích bộ tham số phiên FB/IG Reels hợp lệ theo đúng thứ tự.
func ExtractSocialPassthroughParams(sampleInput string) (string, bool) {
	cleaned := NormalizeEscapedURLStr(strings.TrimSpace(sampleInput))
	if cleaned == "" {
		return "", false
	}
	queryPart := cleaned
	if idx := strings.IndexByte(cleaned, '?'); idx >= 0 {
		queryPart = cleaned[idx+1:]
	} else {
		queryPart = strings.TrimLeft(cleaned, "&")
	}
	if frag := strings.IndexByte(queryPart, '#'); frag >= 0 {
		queryPart = queryPart[:frag]
	}

	var (
		kept             []string
		hasSigOrPayload  bool
		hasChannelType   bool
		hasSocialContext bool
		hasLongGadsSig   bool
	)

	for _, pair := range strings.Split(queryPart, "&") {
		kRaw, vRaw, ok := strings.Cut(pair, "=")
		if !ok {
			continue
		}
		key := strings.TrimSpace(kRaw)
		val := strings.TrimSpace(vRaw)
		if !isSocialPassthroughKey(key) || !isSafeSocialParamValue(val) {
			continue
		}
		switch key {
		case "sm_sig_id", "encrypted_payload":
			hasSigOrPayload = true
		case "channel_type":
			low := strings.ToLower(val)
			if low == "fb" || low == "ig" || low == "facebook" || low == "instagram" {
				hasChannelType = true
			}
		case "content_source", "content_type", "fb_content_id":
			hasSocialContext = true
		case "gads_t_sig":
			if len(val) >= 40 {
				hasLongGadsSig = true
			}
		}
		kept = append(kept, key+"="+val)
	}

	if (hasSigOrPayload || (hasChannelType && hasSocialContext) || hasLongGadsSig) && len(kept) > 0 {
		return strings.Join(kept, "&"), true
	}
	return "", false
}

// SocialParamStrength đánh giá độ mạnh tham số phiên FB/IG Reels (0..3).
func SocialParamStrength(sampleInput string) uint8 {
	extracted, ok := ExtractSocialPassthroughParams(sampleInput)
	if !ok {
		return 0
	}
	if strings.Contains(extracted, "sm_sig_id=") || strings.Contains(extracted, "encrypted_payload=") {
		return 3
	}
	if strings.Contains(extracted, "channel_type=") &&
		(strings.Contains(extracted, "content_source=") || strings.Contains(extracted, "content_type=")) {
		return 2
	}
	return 1
}

// AppendSocialParamsToShopeeLink gắn tham số FB/IG Reels vào link Shopee nếu chưa có.
func AppendSocialParamsToShopeeLink(shopeeURL, rawSocialSample string) string {
	extra, ok := ExtractSocialPassthroughParams(rawSocialSample)
	if !ok {
		return shopeeURL
	}
	if !strings.Contains(shopeeURL, "shopee.vn") ||
		strings.Contains(shopeeURL, "sm_sig_id=") ||
		strings.Contains(shopeeURL, "channel_type=") {
		return shopeeURL
	}
	sep := "?"
	if strings.Contains(shopeeURL, "?") {
		sep = "&"
	}
	return shopeeURL + sep + extra
}

// FetchOpenAPIRecentOrders lấy đơn hàng gần nhất qua Shopee Open API GraphQL conversionReport.
func FetchOpenAPIRecentOrders(client *http.Client, appID, appSecret string, daysBack int64) ([]db.TrackedOrder, error) {
	appIDClean := strings.TrimSpace(appID)
	secretClean := strings.TrimSpace(appSecret)
	if appIDClean == "" || secretClean == "" {
		return nil, errors.New("thiếu App ID hoặc Secret Key của Shopee Open API")
	}

	clamped := daysBack
	if clamped < 1 {
		clamped = 1
	} else if clamped > 30 {
		clamped = 30
	}
	nowTS := time.Now().UTC().Unix()
	startTS := nowTS - clamped*86400

	gqlQuery := fmt.Sprintf(
		`{ conversionReport(purchaseTimeStart: %d, purchaseTimeEnd: %d, limit: 50) { nodes { conversionId conversionStatus totalCommission grossCommission purchaseTime utmContent subIds orders { orderId orderStatus items { actualAmount itemCommission } } } } }`,
		startTS, nowTS,
	)
	payloadBytes, err := json.Marshal(shopeeReportReq{Query: gqlQuery})
	if err != nil {
		return nil, err
	}
	ts := time.Now().UTC().Unix()
	signPayload := fmt.Sprintf("%s%d%s%s", appIDClean, ts, string(payloadBytes), secretClean)
	sig := SHA256Hex([]byte(signPayload))
	authHeader := fmt.Sprintf("SHA256 Credential=%s, Timestamp=%d, Signature=%s", appIDClean, ts, sig)

	req, err := http.NewRequest(http.MethodPost, openAPIEndpoint, bytes.NewReader(payloadBytes))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Authorization", authHeader)
	req.Header.Set("Content-Type", "application/json")

	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("lỗi kết nối Shopee Open API: %w", err)
	}
	defer resp.Body.Close()

	bodyBytes, _ := io.ReadAll(io.LimitReader(resp.Body, 4*1024*1024))
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("Shopee Open API trả HTTP %d", resp.StatusCode)
	}
	return ParseShopeeConversionReportJSON(string(bodyBytes))
}

type convReportResp struct {
	Data   *convReportData      `json:"data"`
	Errors []shopeeGraphQLError `json:"errors"`
}

type convReportData struct {
	ConversionReport *convReportNodes `json:"conversionReport"`
}

type convReportNodes struct {
	Nodes []convNodeRaw `json:"nodes"`
}

type convNodeRaw struct {
	ConversionID     any            `json:"conversionId"`
	ConversionStatus *string        `json:"conversionStatus"`
	TotalCommission  any            `json:"totalCommission"`
	GrossCommission  any            `json:"grossCommission"`
	PurchaseTime     any            `json:"purchaseTime"`
	UtmContent       *string        `json:"utmContent"`
	SubIDs           []string       `json:"subIds"`
	Orders           []convOrderRaw `json:"orders"`
}

type convOrderRaw struct {
	OrderID     *string       `json:"orderId"`
	OrderStatus *string       `json:"orderStatus"`
	Items       []convItemRaw `json:"items"`
}

type convItemRaw struct {
	ActualAmount   any `json:"actualAmount"`
	ItemCommission any `json:"itemCommission"`
}

func jsonAnyToString(v any) string {
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

func jsonAnyToFloat(v any) (float64, bool) {
	switch x := v.(type) {
	case float64:
		return x, true
	case int64:
		return float64(x), true
	case string:
		f, err := strconv.ParseFloat(strings.TrimSpace(x), 64)
		return f, err == nil
	default:
		return 0, false
	}
}

// ParseShopeeConversionReportJSON phân tích dữ liệu JSON đơn hàng conversionReport từ Shopee Open API.
func ParseShopeeConversionReportJSON(body string) ([]db.TrackedOrder, error) {
	var parsed convReportResp
	if err := json.Unmarshal([]byte(body), &parsed); err != nil {
		return nil, fmt.Errorf("lỗi giải mã JSON Shopee conversionReport: %w", err)
	}
	for _, e := range parsed.Errors {
		if e.Message != nil && strings.TrimSpace(*e.Message) != "" {
			return nil, errors.New(strings.TrimSpace(*e.Message))
		}
	}
	if len(parsed.Errors) > 0 {
		return nil, errors.New("Shopee Open API từ chối truy vấn đơn")
	}

	var result []db.TrackedOrder
	if parsed.Data == nil || parsed.Data.ConversionReport == nil {
		return result, nil
	}

	nowStr := time.Now().UTC().Format(time.RFC3339)
	for _, node := range parsed.Data.ConversionReport.Nodes {
		convID := jsonAnyToString(node.ConversionID)
		nestedOrderID := ""
		if len(node.Orders) > 0 && node.Orders[0].OrderID != nil {
			nestedOrderID = strings.TrimSpace(*node.Orders[0].OrderID)
		}
		orderID := nestedOrderID
		if orderID == "" {
			orderID = convID
		}
		if orderID == "" {
			continue
		}

		status := "PENDING"
		if node.ConversionStatus != nil && strings.TrimSpace(*node.ConversionStatus) != "" {
			status = strings.ToUpper(strings.TrimSpace(*node.ConversionStatus))
		}

		commission := 0.0
		if f, ok := jsonAnyToFloat(node.TotalCommission); ok {
			commission = f
		} else if f, ok := jsonAnyToFloat(node.GrossCommission); ok {
			commission = f
		}

		orderAmount := 0.0
		for _, ord := range node.Orders {
			for _, it := range ord.Items {
				if amt, ok := jsonAnyToFloat(it.ActualAmount); ok {
					orderAmount += amt
				}
			}
		}

		var subParts []string
		for _, s := range node.SubIDs {
			if t := strings.TrimSpace(s); t != "" {
				subParts = append(subParts, t)
			}
		}
		subID := strings.Join(subParts, ",")
		if subID == "" && node.UtmContent != nil {
			subID = strings.TrimSpace(*node.UtmContent)
		}

		orderTime := jsonAnyToString(node.PurchaseTime)

		result = append(result, db.TrackedOrder{
			OrderKey:    "SHOPEE:" + orderID,
			Platform:    "Shopee",
			OrderID:     orderID,
			Status:      status,
			OrderAmount: orderAmount,
			Commission:  commission,
			SubID:       subID,
			OrderTime:   orderTime,
			NotifiedAt:  nowStr,
		})
	}
	return result, nil
}
