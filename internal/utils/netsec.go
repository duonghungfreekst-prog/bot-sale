package utils

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// IsPublicIP kiểm tra một địa chỉ IP có thuộc không gian Internet công cộng an toàn hay không.
// Chặn loopback, RFC1918 private, link-local/cloud metadata (169.254.0.0/16), CGNAT (100.64.0.0/10),
// IPv4-mapped IPv6, multicast và các dải đặc biệt để chống SSRF.
func IsPublicIP(ip net.IP) bool {
	if ip == nil {
		return false
	}
	if ip4 := ip.To4(); ip4 != nil {
		if ip4.IsLoopback() ||
			ip4.IsPrivate() ||
			ip4.IsLinkLocalUnicast() ||
			ip4.IsLinkLocalMulticast() ||
			ip4.IsMulticast() ||
			ip4.IsUnspecified() {
			return false
		}
		// 0.0.0.0/8 ("this" network)
		if ip4[0] == 0 {
			return false
		}
		// 100.64.0.0/10 (Carrier-Grade NAT RFC 6598 - nội bộ hạ tầng Cloud/Tailscale)
		if ip4[0] == 100 && ip4[1] >= 64 && ip4[1] <= 127 {
			return false
		}
		// 192.0.0.0/24 (IETF Protocol Assignments) & 192.0.2.0/24 (TEST-NET-1)
		if ip4[0] == 192 && ip4[1] == 0 && (ip4[2] == 0 || ip4[2] == 2) {
			return false
		}
		// 198.18.0.0/15 (Benchmarking)
		if ip4[0] == 198 && (ip4[1] == 18 || ip4[1] == 19) {
			return false
		}
		// 198.51.100.0/24 (TEST-NET-2) & 203.0.113.0/24 (TEST-NET-3)
		if (ip4[0] == 198 && ip4[1] == 51 && ip4[2] == 100) ||
			(ip4[0] == 203 && ip4[1] == 0 && ip4[2] == 113) {
			return false
		}
		// 240.0.0.0/4 (Reserved / Broadcast 255.255.255.255)
		if ip4[0] >= 240 {
			return false
		}
		return true
	}

	if ip.IsLoopback() ||
		ip.IsPrivate() ||
		ip.IsLinkLocalUnicast() ||
		ip.IsLinkLocalMulticast() ||
		ip.IsInterfaceLocalMulticast() ||
		ip.IsMulticast() ||
		ip.IsUnspecified() {
		return false
	}
	// IPv6 Discard Prefix 100::/64
	if len(ip) == net.IPv6len && ip[0] == 0x01 && ip[1] == 0x00 &&
		ip[2] == 0 && ip[3] == 0 && ip[4] == 0 && ip[5] == 0 && ip[6] == 0 && ip[7] == 0 {
		return false
	}
	// IPv6 Documentation Prefix 2001:db8::/32
	if len(ip) == net.IPv6len && ip[0] == 0x20 && ip[1] == 0x01 && ip[2] == 0x0d && ip[3] == 0xb8 {
		return false
	}
	return true
}

// IsSafeHTTPURL chỉ cho phép URL chuẩn http:// hoặc https:// có host hợp lệ và không chứa ký tự điều khiển.
func IsSafeHTTPURL(raw string) bool {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" || strings.ContainsAny(trimmed, "\r\n\x00") {
		return false
	}
	u, err := url.Parse(trimmed)
	if err != nil || u == nil {
		return false
	}
	scheme := strings.ToLower(u.Scheme)
	if scheme != "http" && scheme != "https" {
		return false
	}
	return strings.TrimSpace(u.Hostname()) != ""
}

// ValidatePublicTargetURL xác thực scheme http/https và từ chối literal IP private/loopback/metadata.
func ValidatePublicTargetURL(raw string) error {
	if !IsSafeHTTPURL(raw) {
		return errors.New("unsafe or invalid http/https URL")
	}
	u, _ := url.Parse(strings.TrimSpace(raw))
	host := strings.TrimSpace(u.Hostname())
	if ip := net.ParseIP(host); ip != nil && !IsPublicIP(ip) {
		return fmt.Errorf("ssrf blocked: non-public target IP %s", ip.String())
	}
	return nil
}

// NewSafePublicHTTPClient tạo *http.Client chống SSRF ở cả bước phân giải DNS, Dial trực tiếp tới IP đã kiểm tra
// (chống DNS Rebinding) và kiểm tra scheme/host trên mọi hop Redirect.
func NewSafePublicHTTPClient(
	timeout time.Duration,
	checkRedirect func(req *http.Request, via []*http.Request) error,
) *http.Client {
	dialTimeout := 5 * time.Second
	if timeout > 0 && timeout < dialTimeout {
		dialTimeout = timeout
	}
	dialer := &net.Dialer{
		Timeout:   dialTimeout,
		KeepAlive: 30 * time.Second,
	}

	transport := &http.Transport{
		Proxy:                 http.ProxyFromEnvironment,
		ForceAttemptHTTP2:     true,
		MaxIdleConns:          32,
		IdleConnTimeout:       60 * time.Second,
		TLSHandshakeTimeout:   5 * time.Second,
		ExpectContinueTimeout: 1 * time.Second,
		DialContext: func(ctx context.Context, network, addr string) (net.Conn, error) {
			host, port, err := net.SplitHostPort(addr)
			if err != nil {
				return nil, err
			}
			cleanHost := strings.Trim(strings.TrimSpace(host), "[]")
			if parsed := net.ParseIP(cleanHost); parsed != nil {
				if !IsPublicIP(parsed) {
					return nil, fmt.Errorf("ssrf blocked: non-public IP %s", parsed.String())
				}
				return dialer.DialContext(ctx, network, net.JoinHostPort(parsed.String(), port))
			}

			ips, err := net.DefaultResolver.LookupIPAddr(ctx, cleanHost)
			if err != nil {
				return nil, err
			}
			if len(ips) == 0 {
				return nil, fmt.Errorf("dns resolution returned no address for host %s", SanitizeCRLF(cleanHost))
			}
			allowed := make([]net.IP, 0, len(ips))
			for _, ipAddr := range ips {
				if !IsPublicIP(ipAddr.IP) {
					return nil, fmt.Errorf("ssrf blocked: host %s resolved to non-public IP %s", SanitizeCRLF(cleanHost), ipAddr.IP.String())
				}
				allowed = append(allowed, ipAddr.IP)
			}

			// Dial trực tiếp tới IP công cộng vừa xác thực để ngăn tấn công DNS Rebinding (TOCTOU).
			var lastErr error
			for _, pubIP := range allowed {
				conn, dErr := dialer.DialContext(ctx, network, net.JoinHostPort(pubIP.String(), port))
				if dErr == nil {
					return conn, nil
				}
				lastErr = dErr
			}
			if lastErr != nil {
				return nil, lastErr
			}
			return nil, errors.New("failed to connect to resolved public IP")
		},
	}

	wrappedRedirect := func(req *http.Request, via []*http.Request) error {
		if req == nil || req.URL == nil || !IsSafeHTTPURL(req.URL.String()) {
			return errors.New("ssrf blocked: redirect target has unsafe scheme or host")
		}
		if err := ValidatePublicTargetURL(req.URL.String()); err != nil {
			return err
		}
		if checkRedirect != nil {
			return checkRedirect(req, via)
		}
		if len(via) >= 10 {
			return errors.New("stopped after 10 redirects")
		}
		return nil
	}

	return &http.Client{
		Timeout:       timeout,
		Transport:     transport,
		CheckRedirect: wrappedRedirect,
	}
}
