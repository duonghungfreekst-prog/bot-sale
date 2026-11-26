package accesstrade

import (
	"testing"
)

func strP(s string) *string    { return &s }
func f64P(f float64) *float64 { return &f }

func TestCleanAndMaskToken(t *testing.T) {
	if got := CleanAPIToken(` "Token abc123456789" `); got != "abc123456789" {
		t.Fatalf("unexpected clean token: %q", got)
	}
	if got := MaskToken("1234567890abcdef"); got != "1234••••••••cdef" {
		t.Fatalf("unexpected mask token: %q", got)
	}
}

func TestBestTargetURLGuardsBrokenGateway(t *testing.T) {
	approved := &ApprovedCampaigns{
		Tiki: &CampaignPair{ID: TikiPreferredCampID, Name: "Tiki Creator"},
	}
	offer := VoucherOffer{
		Platform: "Tiki",
		RawURL:   "https://tiki.vn/khuyen-mai",
		AffURL:   strP("https://go.isclix.com/deep_link/PUB123/4348614231480407268?url=https%3A%2F%2Ftiki.vn"),
	}
	got := offer.BestTargetURL(approved)
	expected := "https://go.isclix.com/deep_link/PUB123/6985318064449073386?url=https%3A%2F%2Ftiki.vn"
	if got != expected {
		t.Fatalf("expected %q, got %q", expected, got)
	}
}

func TestMatchVouchersStrictAvoidsOtherShops(t *testing.T) {
	offers := []VoucherOffer{
		{Platform: "Shopee", ShopName: "Other Brand", CouponCode: "OTHER30", DiscountPercent: 30, RawURL: "https://shopee.vn"},
		{Platform: "Shopee", ShopName: "Shopee Toàn Sàn", CouponCode: "ALL15", DiscountPercent: 15, RawURL: "https://shopee.vn"},
		{Platform: "Shopee", ShopName: "Coolmate Official", CouponCode: "COOL20", DiscountPercent: 20, MinSpend: 100000, RawURL: "https://shopee.vn"},
	}
	matched := MatchVouchersForProduct(offers, VoucherMatchQuery{
		Platform:     "Shopee",
		ProductTitle: "Áo thun thể thao",
		CleanURL:     "https://shopee.vn/product/1/2",
		ShopName:     strP("Coolmate Official Store"),
		PriceVND:     f64P(250000),
		MaxItems:     2,
	})
	if len(matched) != 1 || matched[0].CouponCode != "COOL20" {
		t.Fatalf("expected only COOL20 (filtering unrelated shop & non-generic mismatch), got %#v", matched)
	}
}

func TestParseOffersAndOrdersJSON(t *testing.T) {
	offersJSON := `{
		"data": [{
			"name": "[Shopee] Giảm 25% đơn từ 150k",
			"merchant": "shopee",
			"domain": "shopee.vn",
			"link": "https://shopee.vn/search?evcode=V25",
			"end_time": "2099-12-31T00:00:00Z",
			"coupons": [{"coupon_code": "S25K", "coupon_desc": "Giảm 25% tối đa 50k"}]
		}]
	}`
	offers, rawCnt, err := ParseOffersInformationsPage(offersJSON, "2026-01-01", 20)
	if err != nil || rawCnt != 1 || len(offers) != 1 || offers[0].CouponCode != "S25K" || offers[0].MinSpend != 150000 {
		t.Fatalf("unexpected offers parse: len=%d err=%v data=%#v", len(offers), err, offers)
	}

	ordersJSON := `{
		"data": [
			{"transaction_id": "ORD1", "merchant": "shopee", "status": 0, "is_brand_bonus": true, "transaction_value": 1000, "pub_commission": 5000},
			{"transaction_id": "ORD1", "merchant": "shopee", "status": 1, "transaction_value": 300000, "pub_commission": 12000, "sub1": "u1_r0_1234"}
		]
	}`
	ords, err := ParseAccessTradeOrdersJSON(ordersJSON)
	if err != nil || len(ords) != 1 {
		t.Fatalf("orders parse failed: len=%d err=%v", len(ords), err)
	}
	if ords[0].OrderKey != "AT:ORD1" || ords[0].Status != "APPROVED" || ords[0].OrderAmount != 300000 || ords[0].Commission != 17000 || ords[0].SubID != "u1_r0_1234" {
		t.Fatalf("unexpected merged order: %#v", ords[0])
	}
}
