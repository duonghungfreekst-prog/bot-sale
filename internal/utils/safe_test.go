package utils

import (
	"sync"
	"testing"
)

func TestSafeGoRecoversPanicWithoutCrash(t *testing.T) {
	var wg sync.WaitGroup
	SafeGo(&wg, "unit_test_panic", func() {
		panic("simulated unexpected panic")
	})
	wg.Wait()
}

func TestConstantTimeEqualHash(t *testing.T) {
	if !ConstantTimeEqualHash("StrongPass!2026", "StrongPass!2026") {
		t.Fatal("expected equal strings to match")
	}
	if ConstantTimeEqualHash("short", "much_longer_secret_password") {
		t.Fatal("expected unequal strings to fail")
	}
	if ConstantTimeEqualHash("StrongPass!2026", "StrongPass!2027") {
		t.Fatal("expected single-char difference to fail")
	}
}
