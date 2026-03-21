package handler

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/mr-tron/base58"
	"github.com/stretchr/testify/require"

	"github.com/prettyirrelevant/kassi/internal/testutil"
)

func TestMain(m *testing.M) {
	testutil.Setup(m, "../../testutil/fixtures")
}

func newAuthHandler() *AuthHandler {
	return &AuthHandler{
		Store:  testutil.Infra.Store,
		Cache:  testutil.Infra.Cache,
		Config: testutil.Infra.Config,
	}
}

func TestGetNonce(t *testing.T) {
	require.NoError(t, testutil.Infra.LoadFixtures())
	h := newAuthHandler()

	t.Run("returns a nonce", func(t *testing.T) {
		rr := httptest.NewRecorder()
		req := httptest.NewRequest(http.MethodGet, "/auth/nonce", nil)

		err := h.GetNonce(rr, req)
		require.NoError(t, err)
		require.Equal(t, http.StatusOK, rr.Code)

		var resp ApiSuccess
		require.NoError(t, json.NewDecoder(rr.Body).Decode(&resp))

		data, ok := resp.Data.(map[string]any)
		require.True(t, ok)
		require.NotEmpty(t, data["nonce"])
	})

	t.Run("nonce is stored in cache", func(t *testing.T) {
		rr := httptest.NewRecorder()
		req := httptest.NewRequest(http.MethodGet, "/auth/nonce", nil)

		err := h.GetNonce(rr, req)
		require.NoError(t, err)

		var resp ApiSuccess
		require.NoError(t, json.NewDecoder(rr.Body).Decode(&resp))

		data := resp.Data.(map[string]any)
		nonce := data["nonce"].(string)

		val, err := testutil.Infra.Cache.Get(req.Context(), "nonce:"+nonce)
		require.NoError(t, err)
		require.Equal(t, "1", val)
	})

	t.Run("each call returns a different nonce", func(t *testing.T) {
		rr1 := httptest.NewRecorder()
		rr2 := httptest.NewRecorder()
		req1 := httptest.NewRequest(http.MethodGet, "/auth/nonce", nil)
		req2 := httptest.NewRequest(http.MethodGet, "/auth/nonce", nil)

		require.NoError(t, h.GetNonce(rr1, req1))
		require.NoError(t, h.GetNonce(rr2, req2))

		var resp1, resp2 ApiSuccess
		require.NoError(t, json.NewDecoder(rr1.Body).Decode(&resp1))
		require.NoError(t, json.NewDecoder(rr2.Body).Decode(&resp2))

		n1 := resp1.Data.(map[string]any)["nonce"].(string)
		n2 := resp2.Data.(map[string]any)["nonce"].(string)
		require.NotEqual(t, n1, n2)
	})
}

func TestVerify(t *testing.T) {
	require.NoError(t, testutil.Infra.LoadFixtures())
	h := newAuthHandler()

	t.Run("invalid request body", func(t *testing.T) {
		rr := httptest.NewRecorder()
		req := httptest.NewRequest(http.MethodPost, "/auth/verify", bytes.NewReader([]byte("not json")))

		err := h.Verify(rr, req)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusBadRequest, appErr.Status)
	})

	t.Run("invalid signature", func(t *testing.T) {
		body, _ := json.Marshal(verifyRequest{
			Message:   "not a valid message",
			Signature: "not a valid signature",
		})

		rr := httptest.NewRecorder()
		req := httptest.NewRequest(http.MethodPost, "/auth/verify", bytes.NewReader(body))

		err := h.Verify(rr, req)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusUnauthorized, appErr.Status)
		require.Equal(t, "invalid_signature", appErr.Code)
	})

	t.Run("expired nonce", func(t *testing.T) {
		pub, priv, _ := ed25519.GenerateKey(nil)
		address := base58.Encode(pub)
		nonce := "expired_nonce_value"

		message := buildSIWSMessage("localhost", address, nonce)
		signature := base58.Encode(ed25519.Sign(priv, []byte(message)))

		body, _ := json.Marshal(verifyRequest{
			Message:   message,
			Signature: signature,
		})

		rr := httptest.NewRecorder()
		req := httptest.NewRequest(http.MethodPost, "/auth/verify", bytes.NewReader(body))

		err := h.Verify(rr, req)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, "invalid_nonce", appErr.Code)
	})

	t.Run("solana first login creates merchant and returns 201", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		pub, priv, _ := ed25519.GenerateKey(nil)
		address := base58.Encode(pub)
		nonce := storeNonce(t, h)

		message := buildSIWSMessage("localhost", address, nonce)
		signature := base58.Encode(ed25519.Sign(priv, []byte(message)))

		body, _ := json.Marshal(verifyRequest{
			Message:   message,
			Signature: signature,
		})

		rr := httptest.NewRecorder()
		req := httptest.NewRequest(http.MethodPost, "/auth/verify", bytes.NewReader(body))

		err := h.Verify(rr, req)
		require.NoError(t, err)
		require.Equal(t, http.StatusCreated, rr.Code)

		var resp ApiSuccess
		require.NoError(t, json.NewDecoder(rr.Body).Decode(&resp))
		data := resp.Data.(map[string]any)
		require.NotEmpty(t, data["token"])

		claims := parseTestJWT(t, data["token"].(string), h.Config.JWTSecret)
		require.NotEmpty(t, claims["merchant_id"])
		require.Equal(t, address, claims["signer_address"])
		require.Equal(t, "solana", claims["signer_type"])
	})

	t.Run("returning user gets 200", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		pub, priv, _ := ed25519.GenerateKey(nil)
		address := base58.Encode(pub)

		// first login
		nonce1 := storeNonce(t, h)
		msg1 := buildSIWSMessage("localhost", address, nonce1)
		sig1 := base58.Encode(ed25519.Sign(priv, []byte(msg1)))
		body1, _ := json.Marshal(verifyRequest{Message: msg1, Signature: sig1})

		rr1 := httptest.NewRecorder()
		require.NoError(t, h.Verify(rr1, httptest.NewRequest(http.MethodPost, "/auth/verify", bytes.NewReader(body1))))
		require.Equal(t, http.StatusCreated, rr1.Code)

		// second login
		nonce2 := storeNonce(t, h)
		msg2 := buildSIWSMessage("localhost", address, nonce2)
		sig2 := base58.Encode(ed25519.Sign(priv, []byte(msg2)))
		body2, _ := json.Marshal(verifyRequest{Message: msg2, Signature: sig2})

		rr2 := httptest.NewRecorder()
		require.NoError(t, h.Verify(rr2, httptest.NewRequest(http.MethodPost, "/auth/verify", bytes.NewReader(body2))))
		require.Equal(t, http.StatusOK, rr2.Code)
	})

	t.Run("nonce is consumed after use", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		pub, priv, _ := ed25519.GenerateKey(nil)
		address := base58.Encode(pub)
		nonce := storeNonce(t, h)

		message := buildSIWSMessage("localhost", address, nonce)
		signature := base58.Encode(ed25519.Sign(priv, []byte(message)))

		body, _ := json.Marshal(verifyRequest{Message: message, Signature: signature})

		// first use succeeds
		rr1 := httptest.NewRecorder()
		require.NoError(t, h.Verify(rr1, httptest.NewRequest(http.MethodPost, "/auth/verify", bytes.NewReader(body))))

		// replay with same nonce fails
		rr2 := httptest.NewRecorder()
		body2, _ := json.Marshal(verifyRequest{Message: message, Signature: signature})
		err := h.Verify(rr2, httptest.NewRequest(http.MethodPost, "/auth/verify", bytes.NewReader(body2)))
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, "invalid_nonce", appErr.Code)
	})
}

func TestLink(t *testing.T) {
	require.NoError(t, testutil.Infra.LoadFixtures())
	h := newAuthHandler()

	t.Run("invalid request body", func(t *testing.T) {
		rr := httptest.NewRecorder()
		req := httptest.NewRequest(http.MethodPost, "/auth/link", bytes.NewReader([]byte("bad")))
		req = reqWithMerchant(req, "mer_test_existing")

		err := h.Link(rr, req)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusBadRequest, appErr.Status)
	})

	t.Run("links new solana wallet to existing merchant", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		pub, priv, _ := ed25519.GenerateKey(nil)
		address := base58.Encode(pub)
		nonce := storeNonce(t, h)

		message := buildSIWSMessage("localhost", address, nonce)
		signature := base58.Encode(ed25519.Sign(priv, []byte(message)))

		body, _ := json.Marshal(linkRequest{Message: message, Signature: signature})

		rr := httptest.NewRecorder()
		req := httptest.NewRequest(http.MethodPost, "/auth/link", bytes.NewReader(body))
		req = reqWithMerchant(req, "mer_test_existing")

		err := h.Link(rr, req)
		require.NoError(t, err)
		require.Equal(t, http.StatusCreated, rr.Code)

		// verify the signer was created
		sgn, err := testutil.Infra.Store.FindSignerByAddress(req.Context(), address)
		require.NoError(t, err)
		require.Equal(t, "mer_test_existing", sgn.MerchantID)
		require.Equal(t, "solana", sgn.SignerType)
	})

	t.Run("cannot link already-linked wallet", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		pub, priv, _ := ed25519.GenerateKey(nil)
		address := base58.Encode(pub)

		// link first time
		nonce1 := storeNonce(t, h)
		msg1 := buildSIWSMessage("localhost", address, nonce1)
		sig1 := base58.Encode(ed25519.Sign(priv, []byte(msg1)))
		body1, _ := json.Marshal(linkRequest{Message: msg1, Signature: sig1})

		rr1 := httptest.NewRecorder()
		req1 := httptest.NewRequest(http.MethodPost, "/auth/link", bytes.NewReader(body1))
		req1 = reqWithMerchant(req1, "mer_test_existing")
		require.NoError(t, h.Link(rr1, req1))

		// link same wallet again
		nonce2 := storeNonce(t, h)
		msg2 := buildSIWSMessage("localhost", address, nonce2)
		sig2 := base58.Encode(ed25519.Sign(priv, []byte(msg2)))
		body2, _ := json.Marshal(linkRequest{Message: msg2, Signature: sig2})

		rr2 := httptest.NewRecorder()
		req2 := httptest.NewRequest(http.MethodPost, "/auth/link", bytes.NewReader(body2))
		req2 = reqWithMerchant(req2, "mer_test_existing")

		err := h.Link(rr2, req2)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusConflict, appErr.Status)
		require.Equal(t, "signer_already_linked", appErr.Code)
	})
}

// helpers

func buildSIWSMessage(domain, address, nonce string) string {
	return fmt.Sprintf(`%s wants you to sign in with your Solana account:
%s

Sign in to %s

URI: https://%s
Version: 1
Nonce: %s
Issued At: %s`, domain, address, domain, domain, nonce, time.Now().UTC().Format(time.RFC3339))
}

func storeNonce(t *testing.T, h *AuthHandler) string {
	t.Helper()
	rr := httptest.NewRecorder()
	req := httptest.NewRequest(http.MethodGet, "/auth/nonce", nil)
	require.NoError(t, h.GetNonce(rr, req))

	var resp ApiSuccess
	require.NoError(t, json.NewDecoder(rr.Body).Decode(&resp))
	return resp.Data.(map[string]any)["nonce"].(string)
}

func parseTestJWT(t *testing.T, tokenStr, secret string) jwt.MapClaims {
	t.Helper()
	token, err := jwt.Parse(tokenStr, func(token *jwt.Token) (any, error) {
		return []byte(secret), nil
	})
	require.NoError(t, err)
	claims, ok := token.Claims.(jwt.MapClaims)
	require.True(t, ok)
	return claims
}

func reqWithMerchant(r *http.Request, merchantID string) *http.Request {
	merchant, _ := testutil.Infra.Store.FindMerchantByID(r.Context(), merchantID)
	return r.WithContext(
		context.WithValue(r.Context(), CtxMerchant, merchant),
	)
}
