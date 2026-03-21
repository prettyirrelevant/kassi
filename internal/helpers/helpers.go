package helpers

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
)

func RandomString(prefix string, bytes int) string {
	b := make([]byte, bytes)
	_, _ = rand.Read(b)
	return prefix + hex.EncodeToString(b)
}

func HashAPIKey(key string) string {
	h := sha256.Sum256([]byte(key))
	return hex.EncodeToString(h[:])
}
