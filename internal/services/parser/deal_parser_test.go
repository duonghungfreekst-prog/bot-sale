package parser

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"salehunter-go/internal/services/accesstrade"
)

func TestExtractURLAndCleanURL(t *testing.T) {
	p := New()
	u, ok := p.ExtractFirstURL("Xem ngay https://shopee.vn/ao-thun-nam-i.123456.789012?sp_atk=abc&fbclid=xyz.")
	if !ok || strings.HasSuffix(u, ".") {
		t.Fatalf("unexpected url extract: ok=%v u=%q", ok, u)
	}
	cleaned := CleanURL(u, DetectPlatform(u))
	if cleaned != "https://shopee.vn/product/123456/789012" {
		t.Fatalf("unexpected cleaned shopee url: %q", cleaned)
	}

	lzd := CleanURL("https://www.lazada.vn/products/san-pham-abc-i987654-s111222.html?spm=a2o4n", Platform{Kind: PlatformLazada})
	if lzd != "https://www.lazada.vn/products/-i987654.html" {
		t.Fatalf("unexpected cleaned lazada url: %q", lzd)
	}
}

func TestExtractTitleFromHTMLIgnoresAntibot(t *testing.T) {
	antibot := `<title>Just a moment...</title>`
	if _, ok := ExtractTitleFromHTML(antibot); ok {
		t.Fatalf("expected antibot title to be ignored")
	}
	valid := `<meta name="description" content="Mua Áo Thun Thể Thao Nam Cao Cấp giá tốt tại Shopee"><title>Áo Thun Thể Thao | Shopee Việt Nam</title>`
	title, ok := ExtractTitleFromHTML(valid)
	if !ok || title != "Áo Thun Thể Thao Nam Cao Cấp" {
		t.Fatalf("unexpected extracted meta title: ok=%v title=%q", ok, title)
	}
}

func TestExtractLazadaBridgeAndProcessLinkShopeeDirect(t *testing.T) {
	html := `<script>window.location.href = 'https://www.lazada.vn/products/giay-chay-bo-i112233.html?utm_source=fb';</script>`
	target, ok := ExtractLazadaTargetFromHTML(html)
	if !ok || target != "https://www.lazada.vn/products/giay-chay-bo-i112233.html" {
		t.Fatalf("unexpected lazada js extract: ok=%v target=%q", ok, target)
	}

	p := New()
	deal, err := p.ProcessLink(
		"https://shopee.vn/ao-so-mi-trang-i.1000.2000",
		555,
		nil,
		AffiliateSettings{
			ShopeeAffiliateID:  "17375520550",
			ShopeeSocialParams: "https://shopee.vn/m/x?sm_sig_id=SIGWEB&channel_type=fb",
		},
	)
	if err != nil {
		t.Fatalf("process link failed: %v", err)
	}
	if !deal.IsAffiliate || !strings.Contains(deal.AffiliateURL, "affiliate_id=17375520550") || !strings.Contains(deal.AffiliateURL, "sm_sig_id=SIGWEB") {
		t.Fatalf("unexpected processed shopee link: %#v", deal)
	}
}

func TestParseTikiProductJSON(t *testing.T) {
	raw := `{"name":"Nồi Cơm Điện","price":890000,"current_seller":{"name":"Tiki Trading"},"brand":{"name":"Sharp"}}`
	title, price, shop, brand, ok := ParseTikiProductJSON(raw)
	if !ok || title != "Nồi Cơm Điện" || price == nil || *price != 890000 || shop == nil || *shop != "Tiki Trading" || brand == nil || *brand != "Sharp" {
		t.Fatalf("unexpected tiki parse: ok=%v title=%q", ok, title)
	}
}

func TestIsShortLinkStrictAndSSRFBlocked(t *testing.T) {
	if !isShortLink("https://s.shopee.vn/abcXYZ") || !isShortLink("https://www.lazada.vn/s.12345") {
		t.Fatal("expected legitimate shortlinks to be detected")
	}
	spoofs := []string{
		"http://127.0.0.1/?q=s.shopee.vn",
		"https://evil.example/shp.ee",
		"javascript:alert('shope.ee')",
		"https://shopee.vn/product/1/2?ref=vt.tiktok.com",
	}
	for _, s := range spoofs {
		if isShortLink(s) {
			t.Errorf("isShortLink(%q) should return false for non-shortlink host", s)
		}
	}
	if p := DetectPlatform("http://127.0.0.1:18091/health?q=s.shopee.vn"); p.Kind != PlatformUnknown {
		t.Fatalf("expected PlatformUnknown for spoofed query url, got %v", p.Kind)
	}
	if p := DetectPlatform("https://vn.shp.ee/abc"); p.Kind != PlatformShopee {
		t.Fatalf("expected PlatformShopee for vn.shp.ee, got %v", p.Kind)
	}

	calledLocal := false
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calledLocal = true
		w.WriteHeader(http.StatusOK)
	}))
	defer srv.Close()

	p := WithAccessTrade(accesstrade.NewClient())
	if _, err := p.httpClient.Get(srv.URL); err == nil {
		t.Fatal("expected parser httpClient to reject loopback SSRF request")
	}
	if _, err := p.socialProbeClient.Get(srv.URL); err == nil {
		t.Fatal("expected parser socialProbeClient to reject loopback SSRF request")
	}
	if calledLocal {
		t.Fatal("local SSRF server should never receive HTTP traffic from DealParser clients")
	}
	if _, err := p.ProcessLink("http://127.0.0.1:18091/health?q=s.shopee.vn", 1, nil, AffiliateSettings{}); err == nil {
		t.Fatal("expected ProcessLink to reject private/loopback target URL")
	}

	// Kiểm tra BuildAffiliateURL hỗ trợ {{CLEAN_URL}} và chặn template có scheme độc hại (javascript:)
	doubleBrace, _, isAff := BuildAffiliateURL("https://shopee.vn/product/1/2", "https://go.isclix.com/deep_link/1/2?url={{CLEAN_URL}}", 7, nil, "abcdef123456")
	if !isAff || strings.Contains(doubleBrace, "{{") || !strings.Contains(doubleBrace, "shopee.vn") {
		t.Fatalf("unexpected double-brace BuildAffiliateURL output: %q (isAff=%v)", doubleBrace, isAff)
	}
	evilOut, _, evilAff := BuildAffiliateURL("https://shopee.vn/product/1/2", "javascript:alert('{{CLEAN_URL}}')", 7, nil, "abcdef123456")
	if evilAff || evilOut != "https://shopee.vn/product/1/2" {
		t.Fatalf("expected unsafe template scheme to fall back to cleanURL, got %q (isAff=%v)", evilOut, evilAff)
	}
}
