package parser

import (
	"strings"
	"testing"
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
