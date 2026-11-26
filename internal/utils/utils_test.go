package utils

import "testing"

func TestFormatVND(t *testing.T) {
	if got := FormatVND(1234567); got != "1,234,567 ₫" {
		t.Fatalf("expected 1,234,567 ₫, got %q", got)
	}
	if got := FormatVND(-50000); got != "-50,000 ₫" {
		t.Fatalf("expected -50,000 ₫, got %q", got)
	}
	if got := FormatVND(0); got != "0 ₫" {
		t.Fatalf("expected 0 ₫, got %q", got)
	}
}

func TestEscapeHTMLAndSanitizeCRLF(t *testing.T) {
	if got := EscapeHTML(`<script>"a"&'b'</script>`); got != `&lt;script&gt;&quot;a&quot;&amp;&#39;b&#39;&lt;/script&gt;` {
		t.Fatalf("unexpected escape result: %q", got)
	}
	if got := SanitizeCRLF("  hello\r\nworld\n "); got != "helloworld" {
		t.Fatalf("unexpected sanitize result: %q", got)
	}
}

func TestGenerateReferralCode(t *testing.T) {
	code := GenerateReferralCode()
	if len(code) != 7 || code[0] != 'R' {
		t.Fatalf("unexpected referral code: %q", code)
	}
}
