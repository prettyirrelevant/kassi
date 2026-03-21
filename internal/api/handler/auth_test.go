package handler

import (
	"context"
	"crypto/ed25519"
	"encoding/json"
	"fmt"
	"net/http"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/labstack/echo/v5"
	"github.com/labstack/echo/v5/echotest"
	"github.com/mr-tron/base58"
	"github.com/stretchr/testify/require"

	"github.com/prettyirrelevant/kassi/internal/datastore"
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

	t.Run("nonce is stored in cache and consumable", func(t *testing.T) {
		c, rec := echotest.ContextConfig{}.ToContextRecorder(t)

		require.NoError(t, h.GetNonce(c))

		var resp ApiSuccess
		require.NoError(t, json.NewDecoder(rec.Body).Decode(&resp))

		nonce := resp.Data.(map[string]any)["nonce"].(string)
		val, err := testutil.Infra.Cache.GetDel(c.Request().Context(), "nonce:"+nonce)
		require.NoError(t, err)
		require.Equal(t, "1", val)

		// consumed, second get should fail
		_, err = testutil.Infra.Cache.Get(c.Request().Context(), "nonce:"+nonce)
		require.Error(t, err)
	})
}

func TestVerify(t *testing.T) {
	require.NoError(t, testutil.Infra.LoadFixtures())
	h := newAuthHandler()

	t.Run("invalid request body", func(t *testing.T) {
		c, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: []byte("not json"),
		}.ToContextRecorder(t)

		err := h.Verify(c)
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
		c, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body,
		}.ToContextRecorder(t)

		err := h.Verify(c)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusUnauthorized, appErr.Status)
		require.Equal(t, "invalid_signature", appErr.Code)
	})

	t.Run("expired nonce", func(t *testing.T) {
		pub, priv, _ := ed25519.GenerateKey(nil)
		address := base58.Encode(pub)

		message := buildSIWSMessage("localhost", address, "expired_nonce_value")
		signature := base58.Encode(ed25519.Sign(priv, []byte(message)))

		body, _ := json.Marshal(verifyRequest{Message: message, Signature: signature})
		c, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body,
		}.ToContextRecorder(t)

		err := h.Verify(c)
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

		body, _ := json.Marshal(verifyRequest{Message: message, Signature: signature})
		c, rec := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body,
		}.ToContextRecorder(t)

		require.NoError(t, h.Verify(c))
		require.Equal(t, http.StatusCreated, rec.Code)

		var resp ApiSuccess
		require.NoError(t, json.NewDecoder(rec.Body).Decode(&resp))
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

		nonce1 := storeNonce(t, h)
		msg1 := buildSIWSMessage("localhost", address, nonce1)
		sig1 := base58.Encode(ed25519.Sign(priv, []byte(msg1)))
		body1, _ := json.Marshal(verifyRequest{Message: msg1, Signature: sig1})
		c1, rec1 := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body1,
		}.ToContextRecorder(t)
		require.NoError(t, h.Verify(c1))
		require.Equal(t, http.StatusCreated, rec1.Code)

		nonce2 := storeNonce(t, h)
		msg2 := buildSIWSMessage("localhost", address, nonce2)
		sig2 := base58.Encode(ed25519.Sign(priv, []byte(msg2)))
		body2, _ := json.Marshal(verifyRequest{Message: msg2, Signature: sig2})
		c2, rec2 := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body2,
		}.ToContextRecorder(t)
		require.NoError(t, h.Verify(c2))
		require.Equal(t, http.StatusOK, rec2.Code)
	})

	t.Run("nonce is consumed after use", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		pub, priv, _ := ed25519.GenerateKey(nil)
		address := base58.Encode(pub)
		nonce := storeNonce(t, h)

		message := buildSIWSMessage("localhost", address, nonce)
		signature := base58.Encode(ed25519.Sign(priv, []byte(message)))

		body, _ := json.Marshal(verifyRequest{Message: message, Signature: signature})
		c1, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body,
		}.ToContextRecorder(t)
		require.NoError(t, h.Verify(c1))

		body2, _ := json.Marshal(verifyRequest{Message: message, Signature: signature})
		c2, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body2,
		}.ToContextRecorder(t)

		err := h.Verify(c2)
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
		c, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: []byte("bad"),
		}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		err := h.Link(c)
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
		c, rec := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body,
		}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		require.NoError(t, h.Link(c))
		require.Equal(t, http.StatusCreated, rec.Code)

		sgn, err := testutil.Infra.Store.FindSignerByAddress(c.Request().Context(), address)
		require.NoError(t, err)
		require.Equal(t, "mer_test_existing", sgn.MerchantID)
		require.Equal(t, "solana", sgn.SignerType)
	})

	t.Run("cannot link already-linked wallet", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		pub, priv, _ := ed25519.GenerateKey(nil)
		address := base58.Encode(pub)

		nonce1 := storeNonce(t, h)
		msg1 := buildSIWSMessage("localhost", address, nonce1)
		sig1 := base58.Encode(ed25519.Sign(priv, []byte(msg1)))
		body1, _ := json.Marshal(linkRequest{Message: msg1, Signature: sig1})
		c1, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body1,
		}.ToContextRecorder(t)
		c1.Set("merchant", mustFindMerchant(t, "mer_test_existing"))
		require.NoError(t, h.Link(c1))

		nonce2 := storeNonce(t, h)
		msg2 := buildSIWSMessage("localhost", address, nonce2)
		sig2 := base58.Encode(ed25519.Sign(priv, []byte(msg2)))
		body2, _ := json.Marshal(linkRequest{Message: msg2, Signature: sig2})
		c2, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body2,
		}.ToContextRecorder(t)
		c2.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		err := h.Link(c2)
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
	c, rec := echotest.ContextConfig{}.ToContextRecorder(t)
	require.NoError(t, h.GetNonce(c))

	var resp ApiSuccess
	require.NoError(t, json.NewDecoder(rec.Body).Decode(&resp))
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

func mustFindMerchant(t *testing.T, id string) *datastore.Merchant {
	t.Helper()
	m, err := testutil.Infra.Store.FindMerchantByID(context.Background(), id)
	require.NoError(t, err)
	return m
}
