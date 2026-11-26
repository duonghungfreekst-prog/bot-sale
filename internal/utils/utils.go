package utils

import (
	"crypto/rand"
	"fmt"
	"math"
	"math/big"
	"strings"
	"unicode/utf8"
)

const referralCharset = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ"

// FormatVND định dạng số tiền VNĐ kiểu 1,234,567 ₫.
func FormatVND(amount float64) string {
	intPart := int64(math.Round(amount))
	negative := intPart < 0
	var absVal uint64
	if negative {
		absVal = uint64(-intPart)
	} else {
		absVal = uint64(intPart)
	}
	s := fmt.Sprintf("%d", absVal)
	var b strings.Builder
	b.Grow(len(s) + len(s)/3 + 4)
	if negative {
		b.WriteByte('-')
	}
	n := len(s)
	for i := 0; i < n; i++ {
		if i > 0 && (n-i)%3 == 0 {
			b.WriteByte(',')
		}
		b.WriteByte(s[i])
	}
	b.WriteString(" ₫")
	return b.String()
}

// EscapeHTML thoát các ký tự nguy hiểm XSS khỏi chuỗi trước khi nhúng vào HTML.
func EscapeHTML(input string) string {
	r := strings.NewReplacer(
		"&", "&amp;",
		"<", "&lt;",
		">", "&gt;",
		`"`, "&quot;",
		"'", "&#39;",
	)
	return r.Replace(input)
}

// SanitizeCRLF loại bỏ \r và \n chống Log Injection trước khi ghi log.
func SanitizeCRLF(input string) string {
	cleaned := strings.ReplaceAll(input, "\r", "")
	cleaned = strings.ReplaceAll(cleaned, "\n", "")
	return strings.TrimSpace(cleaned)
}

// GenerateReferralCode tạo mã giới thiệu 7 ký tự bắt đầu bằng 'R'.
func GenerateReferralCode() string {
	var b strings.Builder
	b.Grow(7)
	b.WriteByte('R')
	maxIdx := big.NewInt(int64(len(referralCharset)))
	for i := 0; i < 6; i++ {
		idx := 0
		if n, err := rand.Int(rand.Reader, maxIdx); err == nil {
			idx = int(n.Int64())
		}
		b.WriteByte(referralCharset[idx])
	}
	return b.String()
}

// MaskAccount che một phần số tài khoản/nhạy cảm khi hiển thị.
func MaskAccount(acc string) string {
	runes := []rune(acc)
	if len(runes) <= 4 {
		return acc
	}
	return string(runes[:3]) + "***" + string(runes[len(runes)-3:])
}

// MaskToken che API token chỉ chừa 4 ký tự đầu và 4 ký tự cuối.
func MaskToken(tok string) string {
	runes := []rune(strings.TrimSpace(tok))
	if len(runes) <= 8 {
		return "********"
	}
	return string(runes[:4]) + "••••••••" + string(runes[len(runes)-4:])
}

// PercentEncodeRFC3986 mã hóa URL đúng chuẩn RFC 3986 (dùng %20 thay vì +).
func PercentEncodeRFC3986(data string) string {
	var b strings.Builder
	b.Grow(len(data))
	for i := 0; i < len(data); i++ {
		c := data[i]
		if (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') ||
			c == '-' || c == '_' || c == '.' || c == '~' {
			b.WriteByte(c)
		} else {
			fmt.Fprintf(&b, "%%%02X", c)
		}
	}
	return b.String()
}

// TakeRunes cắt an toàn theo số lượng Unicode rune tối đa.
func TakeRunes(s string, maxRunes int) string {
	if maxRunes <= 0 || s == "" {
		return ""
	}
	if utf8.RuneCountInString(s) <= maxRunes {
		return s
	}
	count := 0
	for idx := range s {
		if count == maxRunes {
			return s[:idx]
		}
		count++
	}
	return s
}
