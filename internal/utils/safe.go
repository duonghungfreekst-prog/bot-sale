package utils

import (
	"crypto/sha256"
	"crypto/subtle"
	"log/slog"
	"runtime/debug"
	"sync"
)

// RecoverPanic chặn panic trên các goroutine nền, ghi đầy đủ stack trace vào log mà không làm crash server.
func RecoverPanic(scope string) {
	if r := recover(); r != nil {
		slog.Error("🔥 [RECOVER] Đã chặn panic an toàn trên goroutine nền",
			"scope", scope,
			"panic", r,
			"stack", string(debug.Stack()),
		)
	}
}

// SafeGo khởi chạy goroutine nền có gắn WaitGroup (tùy chọn) và tự động recover panic.
func SafeGo(wg *sync.WaitGroup, scope string, fn func()) {
	if wg != nil {
		wg.Add(1)
	}
	go func() {
		if wg != nil {
			defer wg.Done()
		}
		defer RecoverPanic(scope)
		fn()
	}()
}

// ConstantTimeEqualHash so sánh 2 chuỗi bí mật ở thời gian hằng cố định thông qua băm SHA-256
// nhằm ẩn hoàn toàn cả giá trị lẫn độ dài chuỗi trước Timing Attack.
func ConstantTimeEqualHash(a, b string) bool {
	ha := sha256.Sum256([]byte(a))
	hb := sha256.Sum256([]byte(b))
	return subtle.ConstantTimeCompare(ha[:], hb[:]) == 1
}
