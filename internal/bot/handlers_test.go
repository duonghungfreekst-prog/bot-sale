package bot

import (
	"fmt"
	"strings"
	"testing"

	"salehunter-go/internal/db"
	"salehunter-go/internal/services/parser"
)

func strPtr(s string) *string { return &s }
func f64Ptr(v float64) *float64 { return &v }

func TestIsExclusiveDealRelevantFiltersOtherShopTags(t *testing.T) {
	parsed := &parser.ParsedDeal{
		Platform:        parser.Platform{Kind: parser.PlatformTiki},
		ProductTitle:    "Sữa bột Enfagrow Enspire 3 850g",
		ProductPriceVND: f64Ptr(822000),
		ProductShop:     strPtr("Tiki Trading"),
		ProductBrand:    strPtr("Enfa"),
		OriginalURL:     "https://tiki.vn/product-p274504811.html",
		ResolvedURL:     "https://tiki.vn/product-p274504811.html",
		CleanURL:        "https://tiki.vn/product-p274504811.html",
		AffiliateURL:    "https://tiki.vn/product-p274504811.html",
		ClickID:         "c1",
		SubID:           "s1",
		PrimaryVoucher: parser.SaleVoucher{
			Name:             "Test",
			DiscountDesc:     "Desc",
			MinSpendDesc:     "0đ",
			RemainingPercent: 80,
			BackHours:        "12H",
		},
	}

	makeDeal := func(title string) db.DealItem {
		return db.DealItem{
			ID:           1,
			Platform:     "Tiki",
			Title:        title,
			DiscountDesc: "Giảm 50K",
			CouponCode:   strPtr("CODE50"),
			DealURL:      "https://tiki.vn",
			IsExclusive:  true,
			IsActive:     true,
		}
	}

	if !IsExclusiveDealRelevant(makeDeal("[Toàn sàn] Giảm 50K"), parsed) {
		t.Fatal("expected [Toàn sàn] deal to be relevant")
	}
	if !IsExclusiveDealRelevant(makeDeal("[Tiki Trading] Giảm 45K"), parsed) {
		t.Fatal("expected [Tiki Trading] deal to be relevant")
	}
	if IsExclusiveDealRelevant(makeDeal("[DONA Thời Trang] Giảm 11%"), parsed) {
		t.Fatal("expected [DONA Thời Trang] deal NOT to be relevant")
	}
}

func TestFormatTelegramLinkDisplayShortVsLongDirect(t *testing.T) {
	shortWrapped := "https://go.example.com/Ab12Cd34"
	if got := FormatTelegramLinkDisplay(shortWrapped, "Shopee", true); got != shortWrapped {
		t.Fatalf("expected %q, got %q", shortWrapped, got)
	}

	shortDirect := "https://s.shopee.vn/2gCo1LuaEU"
	if got := FormatTelegramLinkDisplay(shortDirect, "Shopee", false); got != shortDirect {
		t.Fatalf("expected %q, got %q", shortDirect, got)
	}

	longDirect := "https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fproduct%2F123456%2F987654321&affiliate_id=17375520550&sub_id=u1_r0_abc"
	formatted := FormatTelegramLinkDisplay(longDirect, "Shopee", false)
	if !strings.HasPrefix(formatted, `<a href="https://s.shopee.vn/an_redir?`) {
		t.Fatalf("expected anchor tag for long direct link, got %s", formatted)
	}
	if !strings.Contains(formatted, "Bấm mở Shopee kèm ưu đãi") {
		t.Fatalf("expected Vietnamese CTA text, got %s", formatted)
	}
}

func TestShouldProbeAdminSocialInputAndPlatformMatch(t *testing.T) {
	if ShouldProbeAdminSocialInput("https://vn.shp.ee/oSMPdPRU") {
		t.Fatal("expected vn.shp.ee not to trigger probe")
	}
	if ShouldProbeAdminSocialInput("https://shopee.vn/product/123/456") {
		t.Fatal("expected shopee product URL not to trigger probe")
	}
	if ShouldProbeAdminSocialInput("https://vt.tiktok.com/ZSYabc/") {
		t.Fatal("expected tiktok short URL not to trigger probe")
	}
	if !ShouldProbeAdminSocialInput("https://godeal.online/WdIhcsBB") {
		t.Fatal("expected unknown bridge URL to trigger probe")
	}
	if !ShouldProbeAdminSocialInput("https://s.shopee.vn/6L6Hj9wivc?sm_sig_id=SIG1") {
		t.Fatal("expected sm_sig_id URL to trigger probe")
	}

	if !PlatformMatches("TikTok Shop", "TikTokShop") {
		t.Fatal("expected TikTok Shop == TikTokShop")
	}
	if !PlatformMatches("shopee", "Shopee") {
		t.Fatal("expected shopee == Shopee")
	}
	if PlatformMatches("Shopee", "TikTokShop") {
		t.Fatal("expected Shopee != TikTokShop")
	}
}

func TestSelectTopRelevantDealsFiltersGuidesAndDedupsURLs(t *testing.T) {
	parsed := &parser.ParsedDeal{
		Platform:        parser.Platform{Kind: parser.PlatformShopee},
		ProductTitle:    "Dầu bôi trơn Rubik",
		ProductPriceVND: f64Ptr(65000),
		OriginalURL:     "https://s.shopee.vn/9zzaB4uv6M",
		ResolvedURL:     "https://shopee.vn/product/1/2",
		CleanURL:        "https://shopee.vn/product/1/2",
		AffiliateURL:    "https://shopee.vn/product/1/2",
		IsAffiliate:     true,
		ClickID:         "c1",
		SubID:           "s1",
		PrimaryVoucher: parser.SaleVoucher{
			Name:             "Test",
			DiscountDesc:     "Desc",
			MinSpendDesc:     "0đ",
			RemainingPercent: 80,
			BackHours:        "12H",
		},
	}

	makeD := func(id int64, code, desc, dealURL string, minSpend float64) db.DealItem {
		return db.DealItem{
			ID:           id,
			Platform:     "Shopee",
			Title:        "[Toàn sàn] Test",
			DiscountDesc: desc,
			CouponCode:   strPtr(code),
			MinSpend:     minSpend,
			DealURL:      dealURL,
			IsExclusive:  true,
			IsActive:     true,
		}
	}

	deals := []db.DealItem{
		makeD(1, "KOL_FB_22", "📌 Hướng dẫn lấy mã độc quyền Shopee x Facebook 22%", "https://shopee.vn/m/kol", 0),
		makeD(2, "KOL_FB_20", "Voucher 20% toàn sàn", "https://shopee.vn/m/kol", 0),
		makeD(3, "SHP_AUTO_1", "Mã 23% tối đa 500k đơn từ 150K", "https://shopee.vn/search?evcode=ABC1&promotionId=111", 150000),
		makeD(4, "SHP_AUTO_2", "Mã 30% tối đa 100k đơn từ 50K", "https://shopee.vn/search?evcode=ABC2&promotionId=222", 50000),
		makeD(5, "SHP_AUTO_DUP", "Mã 25% trùng URL với SHP_AUTO_2", "https://shopee.vn/search?evcode=ABC2&promotionId=222", 50000),
	}

	top := SelectTopRelevantDeals(deals, parsed, 2)
	if len(top) != 2 {
		t.Fatalf("expected 2 deals, got %d", len(top))
	}
	if top[0].CouponCode == nil || *top[0].CouponCode != "SHP_AUTO_2" {
		t.Fatalf("expected top[0] = SHP_AUTO_2, got %#v", top[0].CouponCode)
	}
	if top[1].CouponCode == nil || *top[1].CouponCode != "KOL_FB_20" {
		t.Fatalf("expected top[1] = KOL_FB_20, got %#v", top[1].CouponCode)
	}
}

func TestRedactTokenInErrorAndParseCommand(t *testing.T) {
	c := NewClient("123456:ABC_SECRET_TOKEN")
	redacted := c.redactTokenErr(fmt.Errorf(`Post "https://api.telegram.org/bot123456:ABC_SECRET_TOKEN/getMe": timeout`))
	if strings.Contains(redacted.Error(), "ABC_SECRET_TOKEN") {
		t.Fatalf("token leaked in error: %s", redacted.Error())
	}

	if cmd, ok := ParseCommand("/start@my_bot ref", "my_bot"); !ok || cmd != CmdStart {
		t.Fatalf("expected CmdStart, got %v %v", cmd, ok)
	}
	if _, ok := ParseCommand("/start@other_bot", "my_bot"); ok {
		t.Fatal("expected other_bot command to be ignored")
	}
}

func TestCanViewAdminStatsFailClosed(t *testing.T) {
	// Khi chưa cấu hình ADMIN_TELEGRAM_ID (adminID == 0), KHÔNG cho phép bất kỳ user nào xem thống kê nội bộ / Origin URL
	if CanViewAdminStats(0, 123456789) {
		t.Fatal("expected CanViewAdminStats(0, user) to be false (fail-closed)")
	}
	if CanViewAdminStats(0, 0) {
		t.Fatal("expected CanViewAdminStats(0, 0) to be false")
	}
	if CanViewAdminStats(999888777, 123456789) {
		t.Fatal("expected non-admin user to be denied")
	}
	if !CanViewAdminStats(999888777, 999888777) {
		t.Fatal("expected matching adminID == userID to be allowed")
	}
}
