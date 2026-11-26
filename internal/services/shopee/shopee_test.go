package shopee

import (
	"strings"
	"testing"
)

func TestSHA256HexNISTVectors(t *testing.T) {
	if got := SHA256Hex([]byte("abc")); got != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" {
		t.Fatalf("unexpected sha256(abc): %q", got)
	}
	if got := SHA256Hex([]byte("")); got != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" {
		t.Fatalf("unexpected sha256(''): %q", got)
	}
}

func TestBuildDirectAnRedirLinkAndValidation(t *testing.T) {
	if !IsValidShopeeAffiliateID("17375520550") {
		t.Fatalf("expected valid shopee affiliate id")
	}
	if IsValidShopeeAffiliateID("12ab") || IsValidShopeeAffiliateID("123") {
		t.Fatalf("expected invalid shopee affiliate id")
	}
	link := BuildDirectAnRedirLink("https://shopee.vn/product/123/456", "17375520550", "u1_r0_abcd")
	if !strings.HasPrefix(link, "https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fproduct%2F123%2F456") {
		t.Fatalf("unexpected an_redir link: %q", link)
	}
	if !strings.Contains(link, "&affiliate_id=17375520550&sub_id=u1_r0_abcd") {
		t.Fatalf("missing params in link: %q", link)
	}
}

func TestExtractAndAppendSocialPassthroughParams(t *testing.T) {
	sample := `https://shopee.vn/universal-link/m/abc?sm_sig_id=SIG123&channel_type=fb&content_source=reels`
	ext, ok := ExtractSocialPassthroughParams(sample)
	if !ok || ext != "sm_sig_id=SIG123&channel_type=fb&content_source=reels" {
		t.Fatalf("unexpected extract result: ok=%v ext=%q", ok, ext)
	}
	if SocialParamStrength(sample) != 3 {
		t.Fatalf("expected strength 3, got %d", SocialParamStrength(sample))
	}
	out := AppendSocialParamsToShopeeLink("https://s.shopee.vn/an_redir?origin_link=abc", sample)
	if !strings.Contains(out, "&sm_sig_id=SIG123") {
		t.Fatalf("expected appended params, got %q", out)
	}
}

func TestParseShopeeConversionReportJSON(t *testing.T) {
	raw := `{
		"data": {
			"conversionReport": {
				"nodes": [{
					"conversionId": 998877,
					"conversionStatus": "completed",
					"totalCommission": "12500.5",
					"purchaseTime": 1700000000,
					"subIds": ["u1", "r0"],
					"orders": [{"orderId": "240101ABCD", "items": [{"actualAmount": 150000}, {"actualAmount": "50000"}]}]
				}]
			}
		}
	}`
	list, err := ParseShopeeConversionReportJSON(raw)
	if err != nil || len(list) != 1 {
		t.Fatalf("parse failed: len=%d err=%v", len(list), err)
	}
	if list[0].OrderKey != "SHOPEE:240101ABCD" || list[0].Status != "COMPLETED" || list[0].OrderAmount != 200000 || list[0].Commission != 12500.5 || list[0].SubID != "u1,r0" {
		t.Fatalf("unexpected parsed order: %#v", list[0])
	}
}
