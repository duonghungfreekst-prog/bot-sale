package config

import (
	"strings"
	"testing"
)

func TestFallbackAccessTradeToken(t *testing.T) {
	tok := resolveAccessTradeAPIToken("")
	if !strings.HasPrefix(tok, "XcsOD") || !strings.HasSuffix(tok, "9N_6") {
		t.Fatalf("unexpected fallback token: %q", tok)
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
