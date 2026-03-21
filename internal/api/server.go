package api

import (
	"errors"
	"net/http"

	"github.com/labstack/echo/v5"
	"github.com/labstack/echo/v5/middleware"
	"go.uber.org/zap"

	"github.com/prettyirrelevant/kassi/internal/api/handler"
	"github.com/prettyirrelevant/kassi/internal/cache"
	"github.com/prettyirrelevant/kassi/internal/config"
	"github.com/prettyirrelevant/kassi/internal/datastore"
	"github.com/prettyirrelevant/kassi/internal/pricing"
	"github.com/prettyirrelevant/kassi/internal/signer"
)

type Server struct {
	store  *datastore.Store
	kms    signer.KMS
	oracle pricing.Oracle
	config *config.Config
	cache  *cache.Cache
	logger *zap.Logger
	echo   *echo.Echo
}

func New(
	store *datastore.Store,
	kms signer.KMS,
	oracle pricing.Oracle,
	cfg *config.Config,
	cache *cache.Cache,
	logger *zap.Logger,
) *Server {
	s := &Server{
		store:  store,
		kms:    kms,
		oracle: oracle,
		config: cfg,
		cache:  cache,
		logger: logger,
		echo:   echo.New(),
	}

	s.echo.HTTPErrorHandler = s.errorHandler
	s.setupRoutes()

	return s
}

func (s *Server) Echo() *echo.Echo {
	return s.echo
}

func (s *Server) setupRoutes() {
	e := s.echo

	e.Use(middleware.RequestID())
	e.Use(middleware.Recover())
	e.Use(s.wideEventLog)
	e.Use(middleware.CORSWithConfig(middleware.CORSConfig{
		AllowOrigins:     []string{"*"},
		AllowMethods:     []string{http.MethodGet, http.MethodPost, http.MethodPatch, http.MethodDelete, http.MethodOptions},
		AllowHeaders:     []string{"Accept", "Authorization", "Content-Type", "X-API-Key"},
		AllowCredentials: true,
		MaxAge:           300,
	}))

	e.GET("/health", s.health)

	auth := &handler.AuthHandler{
		Store:  s.store,
		Cache:  s.cache,
		Config: s.config,
	}

	e.GET("/auth/nonce", auth.GetNonce)
	e.POST("/auth/verify", auth.Verify)

	sessionOnly := e.Group("", s.requireSession)
	sessionOnly.POST("/auth/link", auth.Link)

	merchant := &handler.MerchantHandler{
		Store:  s.store,
		Config: s.config,
	}

	sessionOnly.PATCH("/merchants/me", merchant.UpdateMe)
	sessionOnly.POST("/merchants/me/rotate-key", merchant.RotateKey)
	sessionOnly.POST("/merchants/me/rotate-webhook-secret", merchant.RotateWebhookSecret)

	sessionOrSecretKey := e.Group("", s.requireAny(s.requireSession, s.requireSecretKey))
	sessionOrSecretKey.GET("/merchants/me", merchant.GetMe)
}

func (s *Server) health(c *echo.Context) error {
	return c.JSON(http.StatusOK, handler.ApiSuccess{Data: map[string]string{"status": "healthy"}})
}

func (s *Server) errorHandler(c *echo.Context, err error) {
	if resp, err := echo.UnwrapResponse(c.Response()); err == nil && resp.Committed {
		return
	}

	var appErr *handler.AppError
	if errors.As(err, &appErr) {
		_ = c.JSON(appErr.Status, handler.ApiError{
			Error: handler.ErrorBody{
				Code:    appErr.Code,
				Message: appErr.Message,
				Details: appErr.Details,
			},
		})
		return
	}

	var he *echo.HTTPError
	if errors.As(err, &he) {
		_ = c.JSON(he.Code, handler.ApiError{
			Error: handler.ErrorBody{
				Code:    "http_error",
				Message: http.StatusText(he.Code),
			},
		})
		return
	}

	s.logger.Error("unhandled error", zap.Error(err))
	_ = c.JSON(http.StatusInternalServerError, handler.ApiError{
		Error: handler.ErrorBody{
			Code:    "internal_error",
			Message: "an unexpected error occurred",
		},
	})
}
