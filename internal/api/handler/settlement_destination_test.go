package handler

import (
	"crypto/ed25519"
	"encoding/json"
	"net/http"
	"net/url"
	"testing"

	"github.com/labstack/echo/v5"
	"github.com/labstack/echo/v5/echotest"
	"github.com/mr-tron/base58"
	"github.com/stretchr/testify/require"

	"github.com/prettyirrelevant/kassi/internal/testutil"
)

func newSettlementDestinationHandler() *SettlementDestinationHandler {
	return &SettlementDestinationHandler{
		Store:  testutil.Infra.Store,
		Config: testutil.Infra.Config,
	}
}

func signSolanaMessage(t *testing.T, ah *AuthHandler) (address, message, signature string, priv ed25519.PrivateKey) {
	t.Helper()
	pub, priv, _ := ed25519.GenerateKey(nil)
	address = base58.Encode(pub)
	nonce := storeNonce(t, ah)
	message = buildSIWSMessage("localhost", address, nonce)
	signature = base58.Encode(ed25519.Sign(priv, []byte(message)))
	return
}

func createDestination(t *testing.T, h *SettlementDestinationHandler, ah *AuthHandler, merchantID string, networkIDs []string) []any {
	t.Helper()
	_, message, signature, _ := signSolanaMessage(t, ah)

	body, _ := json.Marshal(createSettlementDestinationRequest{
		Message:    message,
		Signature:  signature,
		NetworkIDs: networkIDs,
	})
	c, rec := echotest.ContextConfig{
		Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
		JSONBody: body,
	}.ToContextRecorder(t)
	c.Set("merchant", mustFindMerchant(t, merchantID))

	require.NoError(t, h.Create(c))
	require.Equal(t, http.StatusCreated, rec.Code)

	var resp ApiSuccess
	require.NoError(t, json.NewDecoder(rec.Body).Decode(&resp))
	return resp.Data.([]any)
}

func TestCreateSettlementDestination(t *testing.T) {
	h := newSettlementDestinationHandler()
	ah := newAuthHandlerForSD()

	t.Run("solana wallet creates destination on solana network", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		data := createDestination(t, h, ah, "mer_test_existing", []string{"solana-mainnet"})
		require.Len(t, data, 1)

		dest := data[0].(map[string]any)
		require.Equal(t, "solana-mainnet", dest["network_id"])
		require.NotEmpty(t, dest["address"])
		require.NotNil(t, dest["network"])

		network := dest["network"].(map[string]any)
		require.Equal(t, "Solana", network["display_name"])
	})

	t.Run("upsert replaces address for same network", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		data1 := createDestination(t, h, ah, "mer_test_existing", []string{"solana-mainnet"})
		addr1 := data1[0].(map[string]any)["address"].(string)

		data2 := createDestination(t, h, ah, "mer_test_existing", []string{"solana-mainnet"})
		addr2 := data2[0].(map[string]any)["address"].(string)

		require.NotEqual(t, addr1, addr2, "upsert should replace the address")

		// list should show only one destination, not two
		c, rec := echotest.ContextConfig{}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))
		require.NoError(t, h.List(c))

		var resp ApiList
		require.NoError(t, json.NewDecoder(rec.Body).Decode(&resp))
		require.Len(t, resp.Data.([]any), 1)
		require.Equal(t, addr2, resp.Data.([]any)[0].(map[string]any)["address"])
	})

	t.Run("solana wallet rejected for evm networks", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		_, message, signature, _ := signSolanaMessage(t, ah)

		body, _ := json.Marshal(createSettlementDestinationRequest{
			Message:    message,
			Signature:  signature,
			NetworkIDs: []string{"ethereum-mainnet", "base-mainnet"},
		})
		c, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body,
		}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		err := h.Create(c)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, "validation_failed", appErr.Code)
		require.Equal(t, "network_ids", appErr.Details[0].Field)
	})

	t.Run("mixed chain types rejected even if some match", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		_, message, signature, _ := signSolanaMessage(t, ah)

		body, _ := json.Marshal(createSettlementDestinationRequest{
			Message:    message,
			Signature:  signature,
			NetworkIDs: []string{"solana-mainnet", "ethereum-mainnet"},
		})
		c, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body,
		}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		err := h.Create(c)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, "validation_failed", appErr.Code)
	})

	t.Run("nonexistent network ID rejected", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		_, message, signature, _ := signSolanaMessage(t, ah)

		body, _ := json.Marshal(createSettlementDestinationRequest{
			Message:    message,
			Signature:  signature,
			NetworkIDs: []string{"solana-mainnet", "does-not-exist"},
		})
		c, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body,
		}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		err := h.Create(c)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, "validation_failed", appErr.Code)
		require.Equal(t, "network_ids", appErr.Details[0].Field)
	})
}

func TestListSettlementDestinations(t *testing.T) {
	h := newSettlementDestinationHandler()

	t.Run("empty when none exist", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		c, rec := echotest.ContextConfig{}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		require.NoError(t, h.List(c))
		require.Equal(t, http.StatusOK, rec.Code)

		var resp ApiList
		require.NoError(t, json.NewDecoder(rec.Body).Decode(&resp))
		require.Empty(t, resp.Data.([]any))
		require.Equal(t, 1, resp.Meta.Page)
		require.Equal(t, 20, resp.Meta.PerPage)
		require.Equal(t, 0, resp.Meta.Total)
	})

	t.Run("pagination returns correct page and total", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())
		ah := newAuthHandlerForSD()

		// create 3 destinations across both evm and solana
		createDestination(t, h, ah, "mer_test_existing", []string{"solana-mainnet"})

		// page 1 with per_page=2
		c1, rec1 := echotest.ContextConfig{
			QueryValues: url.Values{"page": {"1"}, "per_page": {"2"}},
		}.ToContextRecorder(t)
		c1.Set("merchant", mustFindMerchant(t, "mer_test_existing"))
		require.NoError(t, h.List(c1))

		var resp1 ApiList
		require.NoError(t, json.NewDecoder(rec1.Body).Decode(&resp1))
		require.Len(t, resp1.Data.([]any), 1)
		require.Equal(t, 1, resp1.Meta.Page)
		require.Equal(t, 2, resp1.Meta.PerPage)
		require.Equal(t, 1, resp1.Meta.Total)

		// page 2 returns empty
		c2, rec2 := echotest.ContextConfig{
			QueryValues: url.Values{"page": {"2"}, "per_page": {"2"}},
		}.ToContextRecorder(t)
		c2.Set("merchant", mustFindMerchant(t, "mer_test_existing"))
		require.NoError(t, h.List(c2))

		var resp2 ApiList
		require.NoError(t, json.NewDecoder(rec2.Body).Decode(&resp2))
		require.Empty(t, resp2.Data.([]any))
		require.Equal(t, 2, resp2.Meta.Page)
		require.Equal(t, 1, resp2.Meta.Total)
	})

	t.Run("invalid per_page rejected", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		c, _ := echotest.ContextConfig{
			QueryValues: url.Values{"per_page": {"200"}},
		}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		err := h.List(c)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, "validation_failed", appErr.Code)
	})
}

func TestDeleteSettlementDestination(t *testing.T) {
	h := newSettlementDestinationHandler()
	ah := newAuthHandlerForSD()

	t.Run("create then delete lifecycle", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		data := createDestination(t, h, ah, "mer_test_existing", []string{"solana-mainnet"})
		destID := data[0].(map[string]any)["id"].(string)

		_, message, signature, _ := signSolanaMessage(t, ah)
		deleteBody, _ := json.Marshal(deleteSettlementDestinationRequest{
			Message:   message,
			Signature: signature,
		})
		c, rec := echotest.ContextConfig{
			Headers:    map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody:   deleteBody,
			PathValues: echo.PathValues{{Name: "id", Value: destID}},
		}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		require.NoError(t, h.Delete(c))
		require.Equal(t, http.StatusNoContent, rec.Code)

		// list confirms it's gone
		lc, lrec := echotest.ContextConfig{}.ToContextRecorder(t)
		lc.Set("merchant", mustFindMerchant(t, "mer_test_existing"))
		require.NoError(t, h.List(lc))

		var listResp ApiList
		require.NoError(t, json.NewDecoder(lrec.Body).Decode(&listResp))
		require.Empty(t, listResp.Data.([]any))
	})

	t.Run("cannot delete another merchant's destination", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		// create a second merchant via auth
		pub, priv, _ := ed25519.GenerateKey(nil)
		addr := base58.Encode(pub)
		nonce := storeNonce(t, ah)
		msg := buildSIWSMessage("localhost", addr, nonce)
		sig := base58.Encode(ed25519.Sign(priv, []byte(msg)))

		verifyBody, _ := json.Marshal(verifyRequest{Message: msg, Signature: sig})
		vc, vrec := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: verifyBody,
		}.ToContextRecorder(t)
		require.NoError(t, ah.Verify(vc))
		require.Equal(t, http.StatusCreated, vrec.Code)

		var verifyResp ApiSuccess
		require.NoError(t, json.NewDecoder(vrec.Body).Decode(&verifyResp))
		token := verifyResp.Data.(map[string]any)["token"].(string)
		claims := parseTestJWT(t, token, ah.Config.JWTSecret)
		otherMerchantID := claims["merchant_id"].(string)

		// other merchant creates a destination
		data := createDestination(t, h, ah, otherMerchantID, []string{"solana-mainnet"})
		destID := data[0].(map[string]any)["id"].(string)

		// original merchant tries to delete it
		_, delMsg, delSig, _ := signSolanaMessage(t, ah)
		deleteBody, _ := json.Marshal(deleteSettlementDestinationRequest{
			Message:   delMsg,
			Signature: delSig,
		})
		dc, _ := echotest.ContextConfig{
			Headers:    map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody:   deleteBody,
			PathValues: echo.PathValues{{Name: "id", Value: destID}},
		}.ToContextRecorder(t)
		dc.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		err := h.Delete(dc)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusNotFound, appErr.Status)
	})

	t.Run("404 for nonexistent ID", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		_, message, signature, _ := signSolanaMessage(t, ah)
		body, _ := json.Marshal(deleteSettlementDestinationRequest{
			Message:   message,
			Signature: signature,
		})
		c, _ := echotest.ContextConfig{
			Headers:    map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody:   body,
			PathValues: echo.PathValues{{Name: "id", Value: "sdst_nonexistent"}},
		}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		err := h.Delete(c)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusNotFound, appErr.Status)
	})
}

func newAuthHandlerForSD() *AuthHandler {
	return &AuthHandler{
		Store:  testutil.Infra.Store,
		Cache:  testutil.Infra.Cache,
		Config: testutil.Infra.Config,
	}
}
