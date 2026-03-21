package util

import (
	"crypto/rand"
	"encoding/hex"
)

func RandomString(prefix string, bytes int) string {
	b := make([]byte, bytes)
	rand.Read(b)
	return prefix + hex.EncodeToString(b)
}
