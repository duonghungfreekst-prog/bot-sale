package config

import (
	"testing"
)

func TestFallbackAccessTradeToken(t *testing.T) {
	if tok := resolveAccessTradeAPIToken(""); tok != "" {
		t.Fatalf("expected empty token when env unset (no hardcoded secret), got %q", tok)
	}
	if got := resolveAccessTradeAPIToken("  custom-token "); got != "custom-token" {
		t.Fatalf("expected custom-token, got %q", got)
	}
}

func TestWebhookSecretDeterministicAndValid(t *testing.T) {
	s1 := DeriveDeterministicSecret("token-a", "pass-b")
	s2 := DeriveDeterministicSecret("token-a", "pass-b")
	s3 := DeriveDeterministicSecret("token-x", "pass-b")
	if s1 != s2 || s1 == s3 || len(s1) != 32 || !isValidTelegramSecret(s1) {
		t.Fatalf("invalid webhook secret behavior: s1=%q s3=%q", s1, s3)
	}
}

func TestResolveShortlinkBaseURLDefaults(t *testing.T) {
	if got := resolveShortlinkBaseURL(""); got != DefaultShortlinkBaseURL {
		t.Fatalf("expected %s, got %q", DefaultShortlinkBaseURL, got)
	}
	if got := resolveShortlinkBaseURL("https://go.example.com/"); got != "https://go.example.com" {
		t.Fatalf("got %q", got)
	}
	if got := resolveShortlinkBaseURL("off"); got != "" {
		t.Fatalf("expected empty when off, got %q", got)
	}
	if got := ResolvePCRuntimeShortlinkBaseURL("", DefaultShortlinkBaseURL, "", "https://test.trycloudflare.com/", false); got != "https://test.trycloudflare.com" {
		t.Fatalf("expected quick tunnel url, got %q", got)
	}
	if got := ResolvePCRuntimeShortlinkBaseURL("", DefaultShortlinkBaseURL, "", "", false); got != "" {
		t.Fatalf("expected empty when no quick tunnel and no named tunnel, got %q", got)
	}
	if got := ResolvePCRuntimeShortlinkBaseURL("", DefaultShortlinkBaseURL, "", "", true); got != DefaultShortlinkBaseURL {
		t.Fatalf("expected default shortlink when named tunnel enabled, got %q", got)
	}
}

func TestValidateCloudSecurityFailClosed(t *testing.T) {
	// PC Local (PublicBaseURL trống và không có env Cloud) cho phép mật khẩu mặc định
	localCfg := Config{AdminPassword: DefaultAdminPassword}
	if err := localCfg.ValidateCloudSecurity(); err != nil {
		t.Fatalf("local mode should allow default password, got: %v", err)
	}

	// Cloud + mật khẩu mặc định -> PHẢI từ chối khởi động
	cloudDefault := Config{PublicBaseURL: "https://bot.onrender.com", AdminPassword: DefaultAdminPassword}
	if err := cloudDefault.ValidateCloudSecurity(); err == nil {
		t.Fatal("expected Cloud mode to reject default AdminPassword")
	}

	// Cloud + mật khẩu quá ngắn (<12 ký tự) -> PHẢI từ chối khởi động
	cloudShort := Config{PublicBaseURL: "https://bot.onrender.com", AdminPassword: "short123"}
	if err := cloudShort.ValidateCloudSecurity(); err == nil {
		t.Fatal("expected Cloud mode to reject short AdminPassword (<12 chars)")
	}

	// Cloud + mật khẩu mạnh >= 12 ký tự -> Hợp lệ
	cloudStrong := Config{PublicBaseURL: "https://bot.onrender.com", AdminPassword: "VeryStrongAdminPass!2026"}
	if err := cloudStrong.ValidateCloudSecurity(); err != nil {
		t.Fatalf("expected strong Cloud password to pass, got: %v", err)
	}
}
