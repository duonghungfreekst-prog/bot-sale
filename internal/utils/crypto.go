package utils

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"strings"
)

const (
	EncryptedValuePrefix = "enc:v1:"
	aesGCMContextAAD     = "dealx-settings-v1"
)

// DeriveAES256Key dẫn xuất khóa AES-256 cố định 32 byte từ chuỗi bí mật gốc (có phân tách miền).
func DeriveAES256Key(seed string) [32]byte {
	return sha256.Sum256([]byte("dealx-settings-aes256gcm-v1:" + seed))
}

// EncryptAESGCM mã hóa văn bản nhạy cảm bằng AES-256-GCM với nonce 96-bit ngẫu nhiên và AAD.
func EncryptAESGCM(key [32]byte, plaintext string) (string, error) {
	if plaintext == "" {
		return "", nil
	}
	if strings.HasPrefix(plaintext, EncryptedValuePrefix) {
		return plaintext, nil
	}
	block, err := aes.NewCipher(key[:])
	if err != nil {
		return "", err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", err
	}
	nonce := make([]byte, gcm.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return "", err
	}
	sealed := gcm.Seal(nonce, nonce, []byte(plaintext), []byte(aesGCMContextAAD))
	return EncryptedValuePrefix + hex.EncodeToString(sealed), nil
}

// DecryptAESGCM giải mã chuỗi có tiền tố enc:v1:; nếu dữ liệu cũ chưa mã hóa thì trả nguyên bản để tương thích ngược.
func DecryptAESGCM(key [32]byte, stored string) (string, error) {
	if stored == "" {
		return "", nil
	}
	if !strings.HasPrefix(stored, EncryptedValuePrefix) {
		return stored, nil
	}
	hexPart := strings.TrimPrefix(stored, EncryptedValuePrefix)
	raw, err := hex.DecodeString(hexPart)
	if err != nil {
		return "", err
	}
	block, err := aes.NewCipher(key[:])
	if err != nil {
		return "", err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", err
	}
	nonceSize := gcm.NonceSize()
	if len(raw) < nonceSize+gcm.Overhead() {
		return "", errors.New("ciphertext too short")
	}
	nonce := raw[:nonceSize]
	ciphertext := raw[nonceSize:]
	plain, err := gcm.Open(nil, nonce, ciphertext, []byte(aesGCMContextAAD))
	if err != nil {
		return "", err
	}
	return string(plain), nil
}
