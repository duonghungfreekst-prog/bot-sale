package social

import (
	"testing"
)

func TestExtractShopeeConfigHTTPURLParsesFBReels(t *testing.T) {
	html := `<script>window.__CONF__ = { httpUrl: "https:\/\/shopee.vn\/m\/abc?sm_sig_id=SIG99\u0026channel_type=fb" };</script>`
	u, ok := ExtractShopeeConfigHTTPURL(html)
	if !ok || u != "https://shopee.vn/m/abc?sm_sig_id=SIG99&channel_type=fb" {
		t.Fatalf("unexpected decoded url: ok=%v url=%q", ok, u)
	}
}

func TestParseMaGiamGiaShopeeDealsExtractsKOLAndFiltersGuides(t *testing.T) {
	html := `
		<p>📌 Hướng dẫn lấy mã độc quyền Shopee x Facebook 22% <a href="https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fm%2Fkol&amp;affiliate_id=12345">Link</a></p>
		<p>► Mã Instagram giảm 50% tối đa 50k đơn từ 50K! <a href="https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fsearch%3Fevcode%3DIG50&amp;affiliate_id=12345">Lấy ngay</a></p>
		<p>Voucher Shopee 23% tối đa 500k đơn từ 150k <a href="https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fsearch%3Fevcode%3DV23%26promotionId%3D1981279232&amp;affiliate_id=12345">Áp</a></p>
	`
	deals := ParseMaGiamGiaShopeeDeals(html)
	if len(deals) != 2 {
		t.Fatalf("expected 2 valid deals (guide filtered), got %d: %#v", len(deals), deals)
	}
	if deals[0].CouponCode == nil || *deals[0].CouponCode != "KOL_IG_50" {
		t.Fatalf("expected KOL_IG_50 first, got %#v", deals[0])
	}
	if deals[1].CouponCode == nil || *deals[1].CouponCode != "SHP_AUTO_1981279232" {
		t.Fatalf("expected SHP_AUTO_1981279232 second, got %#v", deals[1])
	}
}

func TestCleanHarvestedDescAndChooseEffective(t *testing.T) {
	desc := CleanHarvestedDesc("► List áp dụng >: Giảm 30% đơn từ 100k https://s.shopee.vn/xyz [c link=abc]")
	if desc != "Giảm 30% đơn từ 100k" {
		t.Fatalf("unexpected cleaned desc: %q", desc)
	}
	saved := "https://shopee.vn/x?sm_sig_id=OLD3&channel_type=fb&content_source=reels"
	weak := "https://shopee.vn/y?channel_type=ig&content_source=reels"
	eff := ChooseEffectiveSocialParams(&weak, saved)
	if eff == nil || *eff == weak {
		t.Fatalf("must preserve strength-3 saved signature over weak candidate: %#v", eff)
	}
}
