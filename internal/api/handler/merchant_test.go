package handler

import (
	"encoding/json"
	"net/http"
	"testing"

	"github.com/labstack/echo/v5"
	"github.com/labstack/echo/v5/echotest"
	"github.com/stretchr/testify/require"

	"github.com/prettyirrelevant/kassi/internal/helpers"
	"github.com/prettyirrelevant/kassi/internal/testutil"
)

func newMerchantHandler() *MerchantHandler {
	return &MerchantHandler{
		Store:  testutil.Infra.Store,
		Config: testutil.Infra.Config,
	}
}

func TestUpdateMe(t *testing.T) {
	h := newMerchantHandler()

	t.Run("updates name and webhook url atomically", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		body, _ := json.Marshal(updateMerchantRequest{
			Name:       strPtr("updated merchant"),
			WebhookURL: strPtr("https://example.com/hooks"),
		})
		c, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body,
		}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		require.NoError(t, h.UpdateMe(c))

		merchant, err := testutil.Infra.Store.FindMerchantByID(c.Request().Context(), "mer_test_existing")
		require.NoError(t, err)
		require.Equal(t, "updated merchant", *merchant.Name)
		require.Equal(t, "https://example.com/hooks", *merchant.Config.WebhookURL)
	})

	t.Run("null name clears existing name", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		// first set a name
		body1, _ := json.Marshal(updateMerchantRequest{Name: strPtr("has a name")})
		c1, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: body1,
		}.ToContextRecorder(t)
		c1.Set("merchant", mustFindMerchant(t, "mer_test_existing"))
		require.NoError(t, h.UpdateMe(c1))

		// then send null (omitted field = nil pointer)
		c2, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: []byte(`{}`),
		}.ToContextRecorder(t)
		c2.Set("merchant", mustFindMerchant(t, "mer_test_existing"))
		require.NoError(t, h.UpdateMe(c2))

		merchant, err := testutil.Infra.Store.FindMerchantByID(c2.Request().Context(), "mer_test_existing")
		require.NoError(t, err)
		require.Nil(t, merchant.Name)
	})

	t.Run("invalid JSON body returns 400", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		c, _ := echotest.ContextConfig{
			Headers:  map[string][]string{echo.HeaderContentType: {echo.MIMEApplicationJSON}},
			JSONBody: []byte("not json"),
		}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		err := h.UpdateMe(c)
		require.Error(t, err)

		var appErr *AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusBadRequest, appErr.Status)
	})
}

func TestRotateKey(t *testing.T) {
	h := newMerchantHandler()

	t.Run("new key hash matches returned key", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		c, rec := echotest.ContextConfig{}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))

		require.NoError(t, h.RotateKey(c))

		var resp ApiSuccess
		require.NoError(t, json.NewDecoder(rec.Body).Decode(&resp))
		secretKey := resp.Data.(map[string]any)["secret_key"].(string)

		merchant, err := testutil.Infra.Store.FindMerchantByID(c.Request().Context(), "mer_test_existing")
		require.NoError(t, err)
		require.NotNil(t, merchant.Config.SecretKeyHash)
		require.Equal(t, helpers.HashAPIKey(secretKey), *merchant.Config.SecretKeyHash)
	})

	t.Run("old key hash is replaced", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		c1, rec1 := echotest.ContextConfig{}.ToContextRecorder(t)
		c1.Set("merchant", mustFindMerchant(t, "mer_test_existing"))
		require.NoError(t, h.RotateKey(c1))

		var resp1 ApiSuccess
		require.NoError(t, json.NewDecoder(rec1.Body).Decode(&resp1))
		oldKey := resp1.Data.(map[string]any)["secret_key"].(string)

		c2, rec2 := echotest.ContextConfig{}.ToContextRecorder(t)
		c2.Set("merchant", mustFindMerchant(t, "mer_test_existing"))
		require.NoError(t, h.RotateKey(c2))

		var resp2 ApiSuccess
		require.NoError(t, json.NewDecoder(rec2.Body).Decode(&resp2))
		newKey := resp2.Data.(map[string]any)["secret_key"].(string)

		require.NotEqual(t, oldKey, newKey)

		// old key hash should no longer match
		merchant, err := testutil.Infra.Store.FindMerchantByID(c2.Request().Context(), "mer_test_existing")
		require.NoError(t, err)
		require.NotEqual(t, helpers.HashAPIKey(oldKey), *merchant.Config.SecretKeyHash)
		require.Equal(t, helpers.HashAPIKey(newKey), *merchant.Config.SecretKeyHash)
	})
}

func TestRotateWebhookSecret(t *testing.T) {
	h := newMerchantHandler()

	t.Run("new secret is persisted and replaces old", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())

		merchant, err := testutil.Infra.Store.FindMerchantByID(t.Context(), "mer_test_existing")
		require.NoError(t, err)
		oldSecret := merchant.Config.WebhookSecret

		c, rec := echotest.ContextConfig{}.ToContextRecorder(t)
		c.Set("merchant", mustFindMerchant(t, "mer_test_existing"))
		require.NoError(t, h.RotateWebhookSecret(c))

		var resp ApiSuccess
		require.NoError(t, json.NewDecoder(rec.Body).Decode(&resp))
		newSecret := resp.Data.(map[string]any)["webhook_secret"].(string)

		require.NotEqual(t, oldSecret, newSecret)

		merchant, err = testutil.Infra.Store.FindMerchantByID(c.Request().Context(), "mer_test_existing")
		require.NoError(t, err)
		require.Equal(t, newSecret, merchant.Config.WebhookSecret)
	})
}

func strPtr(s string) *string { return &s }
