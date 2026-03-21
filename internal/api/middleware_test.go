package api

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/labstack/echo/v5"
	"github.com/stretchr/testify/require"
	"go.uber.org/zap"
	"go.uber.org/zap/zaptest/observer"

	"github.com/prettyirrelevant/kassi/internal/api/handler"
	"github.com/prettyirrelevant/kassi/internal/config"
	"github.com/prettyirrelevant/kassi/internal/datastore"
	"github.com/prettyirrelevant/kassi/internal/testutil"
)

func TestMain(m *testing.M) {
	testutil.Setup(m, "../testutil/fixtures")
}

func testServer(logger *zap.Logger) *Server {
	return &Server{
		store:  testutil.Infra.Store,
		cache:  testutil.Infra.Cache,
		config: testutil.Infra.Config,
		logger: logger,
	}
}

func issueTestJWT(cfg *config.Config, merchantID, address, signerType string) string {
	now := time.Now()
	token := jwt.NewWithClaims(jwt.SigningMethodHS256, jwt.MapClaims{
		"merchant_id":    merchantID,
		"signer_address": address,
		"signer_type":    signerType,
		"iat":            now.Unix(),
		"exp":            now.Add(cfg.JWTExpiry).Unix(),
	})
	s, _ := token.SignedString([]byte(cfg.JWTSecret))
	return s
}

func TestWideEventLog(t *testing.T) {
	t.Run("logs method, path, status, and duration on success", func(t *testing.T) {
		core, logs := observer.New(zap.InfoLevel)
		s := testServer(zap.New(core))

		e := echo.New()
		c := e.NewContext(
			httptest.NewRequest(http.MethodGet, "/health", nil),
			httptest.NewRecorder(),
		)

		mw := s.wideEventLog(func(c *echo.Context) error {
			return c.JSON(http.StatusOK, "ok")
		})
		require.NoError(t, mw(c))
		require.Equal(t, 1, logs.Len())

		entry := logs.All()[0]
		require.Equal(t, zap.InfoLevel, entry.Level)
		require.Equal(t, "request", entry.Message)

		fields := entry.ContextMap()
		require.Equal(t, "GET", fields["method"])
		require.Equal(t, "/health", fields["path"])
		require.Equal(t, int64(http.StatusOK), fields["status_code"])
		require.Contains(t, fields, "duration_ms")
		_, hasMerchant := fields["merchant_id"]
		require.False(t, hasMerchant)
	})

	t.Run("logs error level and error field on handler error", func(t *testing.T) {
		core, logs := observer.New(zap.InfoLevel)
		s := testServer(zap.New(core))

		e := echo.New()
		c := e.NewContext(
			httptest.NewRequest(http.MethodGet, "/fail", nil),
			httptest.NewRecorder(),
		)

		mw := s.wideEventLog(func(c *echo.Context) error {
			return handler.ErrNotFound
		})

		err := mw(c)
		require.Error(t, err)
		require.Equal(t, 1, logs.Len())

		entry := logs.All()[0]
		require.Equal(t, zap.ErrorLevel, entry.Level)
		require.Equal(t, int64(http.StatusNotFound), entry.ContextMap()["status_code"])
	})

	t.Run("includes merchant_id when merchant is in context", func(t *testing.T) {
		core, logs := observer.New(zap.InfoLevel)
		s := testServer(zap.New(core))

		e := echo.New()
		c := e.NewContext(
			httptest.NewRequest(http.MethodGet, "/me", nil),
			httptest.NewRecorder(),
		)
		c.Set("merchant", &datastore.Merchant{ID: "mer_test123"})

		mw := s.wideEventLog(func(c *echo.Context) error {
			return c.JSON(http.StatusOK, "ok")
		})
		require.NoError(t, mw(c))

		entry := logs.All()[0]
		require.Equal(t, "mer_test123", entry.ContextMap()["merchant_id"])
	})
}

func TestRequireSession(t *testing.T) {
	t.Run("rejects missing authorization header", func(t *testing.T) {
		s := testServer(zap.NewNop())

		e := echo.New()
		c := e.NewContext(
			httptest.NewRequest(http.MethodGet, "/", nil),
			httptest.NewRecorder(),
		)

		called := false
		mw := s.requireSession(func(c *echo.Context) error {
			called = true
			return nil
		})

		err := mw(c)
		require.Error(t, err)
		require.False(t, called)

		var appErr *handler.AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusUnauthorized, appErr.Status)
	})

	t.Run("rejects invalid JWT", func(t *testing.T) {
		s := testServer(zap.NewNop())

		e := echo.New()
		req := httptest.NewRequest(http.MethodGet, "/", nil)
		req.Header.Set("Authorization", "Bearer invalid.token.here")
		c := e.NewContext(req, httptest.NewRecorder())

		err := s.requireSession(func(c *echo.Context) error { return nil })(c)
		require.Error(t, err)

		var appErr *handler.AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusUnauthorized, appErr.Status)
	})

	t.Run("rejects valid JWT for nonexistent merchant", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())
		s := testServer(zap.NewNop())

		token := issueTestJWT(s.config, "mer_doesnotexist", "0xabc", "evm")

		e := echo.New()
		req := httptest.NewRequest(http.MethodGet, "/", nil)
		req.Header.Set("Authorization", "Bearer "+token)
		c := e.NewContext(req, httptest.NewRecorder())

		err := s.requireSession(func(c *echo.Context) error { return nil })(c)
		require.Error(t, err)

		var appErr *handler.AppError
		require.ErrorAs(t, err, &appErr)
		require.Equal(t, http.StatusUnauthorized, appErr.Status)
	})

	t.Run("sets merchant in context for valid JWT", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())
		s := testServer(zap.NewNop())

		token := issueTestJWT(s.config, "mer_test_existing", "0x742d35Cc6634C0532925a3b844Bc9e7595f2bD18", "evm")

		e := echo.New()
		req := httptest.NewRequest(http.MethodGet, "/", nil)
		req.Header.Set("Authorization", "Bearer "+token)
		c := e.NewContext(req, httptest.NewRecorder())

		var got *datastore.Merchant
		err := s.requireSession(func(c *echo.Context) error {
			got = handler.MerchantFromCtx(c)
			return nil
		})(c)

		require.NoError(t, err)
		require.NotNil(t, got)
		require.Equal(t, "mer_test_existing", got.ID)
	})
}

func TestRequireAny(t *testing.T) {
	t.Run("passes if first middleware succeeds", func(t *testing.T) {
		require.NoError(t, testutil.Infra.LoadFixtures())
		s := testServer(zap.NewNop())

		token := issueTestJWT(s.config, "mer_test_existing", "0x742d35Cc6634C0532925a3b844Bc9e7595f2bD18", "evm")

		e := echo.New()
		req := httptest.NewRequest(http.MethodGet, "/", nil)
		req.Header.Set("Authorization", "Bearer "+token)
		c := e.NewContext(req, httptest.NewRecorder())

		called := false
		mw := s.requireAny(s.requireSession, s.requireSecretKey)
		err := mw(func(c *echo.Context) error {
			called = true
			return nil
		})(c)

		require.NoError(t, err)
		require.True(t, called)
	})

	t.Run("rejects if all middlewares fail", func(t *testing.T) {
		s := testServer(zap.NewNop())

		e := echo.New()
		c := e.NewContext(
			httptest.NewRequest(http.MethodGet, "/", nil),
			httptest.NewRecorder(),
		)

		called := false
		mw := s.requireAny(s.requireSession, s.requireSecretKey)
		err := mw(func(c *echo.Context) error {
			called = true
			return nil
		})(c)

		require.Error(t, err)
		require.False(t, called)
	})
}
