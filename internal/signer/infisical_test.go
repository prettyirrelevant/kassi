package signer

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/jarcoal/httpmock"
	"github.com/stretchr/testify/require"
)

func newTestKMS() *InfisicalKMS {
	kms := NewInfisicalKMS("test-client-id", "test-client-secret", "test-project-id")
	httpmock.ActivateNonDefault(kms.client.GetClient())
	return kms
}

func registerAuthResponder(calls *atomic.Int64) {
	httpmock.RegisterResponder("POST", "https://app.infisical.com/api/v1/auth/universal-auth/login",
		func(req *http.Request) (*http.Response, error) {
			if calls != nil {
				calls.Add(1)
			}
			return httpmock.NewJsonResponse(http.StatusOK, map[string]any{
				"accessToken": "test-token",
				"expiresIn":   3600,
			})
		},
	)
}

func registerCreateKeyResponder() {
	httpmock.RegisterResponder("POST", "https://app.infisical.com/api/v1/kms/keys",
		httpmock.NewJsonResponderOrPanic(http.StatusOK, map[string]any{
			"key": map[string]string{"id": "key-uuid-123", "name": "test-key"},
		}),
	)
}

func TestEnsureAuth_DoesNotRecurse(t *testing.T) {
	kms := newTestKMS()
	defer httpmock.DeactivateAndReset()

	var authCalls atomic.Int64
	registerAuthResponder(&authCalls)
	registerCreateKeyResponder()

	require.NoError(t, kms.CreateKey(context.Background(), "test-key"))
	require.Equal(t, int64(1), authCalls.Load())
}

func TestEnsureAuth_TokenReusedAcrossRequests(t *testing.T) {
	kms := newTestKMS()
	defer httpmock.DeactivateAndReset()

	var authCalls atomic.Int64
	registerAuthResponder(&authCalls)
	registerCreateKeyResponder()

	for range 5 {
		require.NoError(t, kms.CreateKey(context.Background(), "test-key"))
	}

	require.Equal(t, int64(1), authCalls.Load())
}

func TestEnsureAuth_NoConcurrentRace(t *testing.T) {
	kms := newTestKMS()
	defer httpmock.DeactivateAndReset()

	var authCalls atomic.Int64
	registerAuthResponder(&authCalls)
	registerCreateKeyResponder()

	var wg sync.WaitGroup
	errs := make([]error, 50)

	for i := range 50 {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			errs[idx] = kms.CreateKey(context.Background(), "test-key")
		}(i)
	}
	wg.Wait()

	for i, err := range errs {
		require.NoError(t, err, "goroutine %d", i)
	}

	// singleflight should deduplicate: exactly 1 auth call
	require.Equal(t, int64(1), authCalls.Load())
}

func TestEncrypt_SendsBase64Plaintext(t *testing.T) {
	kms := newTestKMS()
	defer httpmock.DeactivateAndReset()

	registerAuthResponder(nil)

	plaintext := []byte("secret seed bytes")
	expectedB64 := base64.StdEncoding.EncodeToString(plaintext)

	// pre-populate key cache so we skip resolveKeyID
	kms.keys.Store("my-key", "key-uuid")

	httpmock.RegisterResponder("POST", "https://app.infisical.com/api/v1/kms/keys/key-uuid/encrypt",
		func(req *http.Request) (*http.Response, error) {
			body, _ := io.ReadAll(req.Body)
			var payload map[string]string
			require.NoError(t, json.Unmarshal(body, &payload))
			require.Equal(t, expectedB64, payload["plaintext"])

			return httpmock.NewJsonResponse(http.StatusOK, map[string]string{
				"ciphertext": "encrypted-data",
			})
		},
	)

	ct, err := kms.Encrypt(context.Background(), "my-key", plaintext)
	require.NoError(t, err)
	require.Equal(t, "encrypted-data", ct)
}

func TestDecrypt_DecodesBase64Response(t *testing.T) {
	kms := newTestKMS()
	defer httpmock.DeactivateAndReset()

	registerAuthResponder(nil)

	original := []byte("secret seed bytes")
	kms.keys.Store("my-key", "key-uuid")

	httpmock.RegisterResponder("POST", "https://app.infisical.com/api/v1/kms/keys/key-uuid/decrypt",
		httpmock.NewJsonResponderOrPanic(http.StatusOK, map[string]string{
			"plaintext": base64.StdEncoding.EncodeToString(original),
		}),
	)

	result, err := kms.Decrypt(context.Background(), "my-key", "some-ciphertext")
	require.NoError(t, err)
	require.Equal(t, original, result)
}

func TestCreateKey_PopulatesKeyCache(t *testing.T) {
	kms := newTestKMS()
	defer httpmock.DeactivateAndReset()

	registerAuthResponder(nil)
	registerCreateKeyResponder()

	require.NoError(t, kms.CreateKey(context.Background(), "test-key"))

	id, ok := kms.keys.Load("test-key")
	require.True(t, ok)
	require.Equal(t, "key-uuid-123", id)
}

func TestResolveKeyID_CachesAfterFirstLookup(t *testing.T) {
	kms := newTestKMS()
	defer httpmock.DeactivateAndReset()

	registerAuthResponder(nil)

	var lookupCalls atomic.Int64
	httpmock.RegisterResponder("GET", "https://app.infisical.com/api/v1/kms/keys/key-name/my-key",
		func(req *http.Request) (*http.Response, error) {
			lookupCalls.Add(1)
			return httpmock.NewJsonResponse(http.StatusOK, map[string]any{
				"key": map[string]string{"id": "resolved-uuid", "name": "my-key"},
			})
		},
	)

	ctx := context.Background()

	id1, err := kms.resolveKeyID(ctx, "my-key")
	require.NoError(t, err)
	require.Equal(t, "resolved-uuid", id1)

	id2, err := kms.resolveKeyID(ctx, "my-key")
	require.NoError(t, err)
	require.Equal(t, "resolved-uuid", id2)

	require.Equal(t, int64(1), lookupCalls.Load())
}
