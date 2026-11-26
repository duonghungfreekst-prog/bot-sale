package utils

import (
	"strings"
	"testing"
)

func TestAES256GCMEncryptDecryptAndTamperReject(t *testing.T) {
	key := DeriveAES256Key("MasterSecretKey!2026")
	wrongKey := DeriveAES256Key("WrongKey!2026")

	enc1, err := EncryptAESGCM(key, "at_live_secret_token_123456")
	if err != nil {
		t.Fatalf("EncryptAESGCM failed: %v", err)
	}
	enc2, err := EncryptAESGCM(key, "at_live_secret_token_123456")
	if err != nil {
		t.Fatalf("EncryptAESGCM 2 failed: %v", err)
	}
	if !strings.HasPrefix(enc1, EncryptedValuePrefix) || strings.Contains(enc1, "at_live_secret_token") {
		t.Fatalf("ciphertext did not hide plaintext properly: %q", enc1)
	}
	if enc1 == enc2 {
		t.Fatal("expected random nonce to produce distinct ciphertexts")
	}

	dec, err := DecryptAESGCM(key, enc1)
	if err != nil || dec != "at_live_secret_token_123456" {
		t.Fatalf("DecryptAESGCM mismatch: got %q, err=%v", dec, err)
	}

	if _, err := DecryptAESGCM(wrongKey, enc1); err == nil {
		t.Fatal("expected DecryptAESGCM with wrong key to fail authentication")
	}

	// Tương thích ngược với giá trị cũ chưa có tiền tố enc:v1:
	legacy, err := DecryptAESGCM(key, "legacy_plain_token")
	if err != nil || legacy != "legacy_plain_token" {
		t.Fatalf("expected legacy plaintext fallback, got %q, err=%v", legacy, err)
	}
}
