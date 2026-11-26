package utils

import (
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestIsPublicIP(t *testing.T) {
	cases := []struct {
		ip   string
		want bool
	}{
		{"8.8.8.8", true},
		{"1.1.1.1", true},
		{"2001:4860:4860::8888", true},
		{"127.0.0.1", false},
		{"10.0.0.5", false},
		{"172.16.0.1", false},
		{"192.168.1.10", false},
		{"169.254.169.254", false},
		{"100.64.0.1", false},
		{"0.0.0.0", false},
		{"::1", false},
		{"::ffff:127.0.0.1", false},
		{"::ffff:169.254.169.254", false},
		{"fe80::1", false},
		{"fc00::1", false},
		{"fd12:3456::1", false},
	}
	for _, tc := range cases {
		parsed := net.ParseIP(tc.ip)
		if parsed == nil {
			t.Fatalf("failed to parse IP %s", tc.ip)
		}
		if got := IsPublicIP(parsed); got != tc.want {
			t.Errorf("IsPublicIP(%s) = %v, want %v", tc.ip, got, tc.want)
		}
	}
}

func TestIsSafeHTTPURL(t *testing.T) {
	if !IsSafeHTTPURL("https://shopee.vn/product/1/2") {
		t.Fatal("expected valid https url to pass")
	}
	if !IsSafeHTTPURL("http://example.com") {
		t.Fatal("expected valid http url to pass")
	}
	badURLs := []string{
		"",
		"javascript:alert(1)",
		"data:text/html,<script>alert(1)</script>",
		"file:///etc/passwd",
		"https:///missing-host",
		"https://shopee.vn\r\nSet-Cookie: a=1",
	}
	for _, u := range badURLs {
		if IsSafeHTTPURL(u) {
			t.Errorf("expected IsSafeHTTPURL(%q) to return false", u)
		}
	}
}

func TestSafePublicHTTPClientBlocksLoopbackAndMetadata(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("SECRET_LOCAL_DATA"))
	}))
	defer srv.Close()

	client := NewSafePublicHTTPClient(2*time.Second, nil)

	resp, err := client.Get(srv.URL)
	if err == nil {
		_ = resp.Body.Close()
		t.Fatalf("expected SSRF client to block loopback server %s", srv.URL)
	}
	if !strings.Contains(err.Error(), "ssrf blocked") {
		t.Fatalf("expected ssrf blocked error, got: %v", err)
	}

	respMeta, errMeta := client.Get("http://169.254.169.254/latest/meta-data/")
	if errMeta == nil {
		_ = respMeta.Body.Close()
		t.Fatal("expected SSRF client to block AWS/GCP metadata IP 169.254.169.254")
	}
	if !strings.Contains(errMeta.Error(), "ssrf blocked") {
		t.Fatalf("expected ssrf blocked error for metadata IP, got: %v", errMeta)
	}
}
