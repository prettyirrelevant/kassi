package server

import (
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"
	"github.com/go-chi/cors"
	httpSwagger "github.com/swaggo/http-swagger/v2"
	"go.uber.org/zap"

	"github.com/prettyirrelevant/kassi/internal/cache"
	"github.com/prettyirrelevant/kassi/internal/config"
	"github.com/prettyirrelevant/kassi/internal/datastore"
	"github.com/prettyirrelevant/kassi/internal/pricing"
	"github.com/prettyirrelevant/kassi/internal/server/handlers"
	"github.com/prettyirrelevant/kassi/internal/signer"
	"github.com/prettyirrelevant/kassi/internal/util"
)

type Server struct {
	store  *datastore.Store
	kms    signer.KMS
	oracle pricing.Oracle
	config *config.Config
	cache  *cache.Cache
	logger *zap.Logger
}

func New(
	store *datastore.Store,
	kms signer.KMS,
	oracle pricing.Oracle,
	cfg *config.Config,
	cache *cache.Cache,
	logger *zap.Logger,
) *Server {
	return &Server{
		store:  store,
		kms:    kms,
		oracle: oracle,
		config: cfg,
		cache:  cache,
		logger: logger,
	}
}

func (s *Server) Routes() http.Handler {
	r := chi.NewRouter()

	r.Use(middleware.RequestID)
	r.Use(middleware.RealIP)
	r.Use(s.wideEventLog)
	r.Use(cors.Handler(cors.Options{
		AllowedOrigins:   []string{"*"},
		AllowedMethods:   []string{"GET", "POST", "PATCH", "DELETE", "OPTIONS"},
		AllowedHeaders:   []string{"Accept", "Authorization", "Content-Type", "X-API-Key"},
		AllowCredentials: true,
		MaxAge:           300,
	}))

	r.NotFound(func(w http.ResponseWriter, r *http.Request) {
		util.WriteJSON(w, util.ErrRouteNotFound.Status, util.ApiError{
			Error: util.ErrorBody{
				Code:    util.ErrRouteNotFound.Code,
				Message: util.ErrRouteNotFound.Message,
			},
		})
	})

	r.Get("/health", util.Wrap(s.Health))
	r.Get("/docs/*", httpSwagger.Handler())

	auth := &handlers.AuthHandler{
		Store:  s.store,
		Cache:  s.cache,
		Config: s.config,
	}

	r.Get("/auth/nonce", util.Wrap(auth.GetNonce))
	r.Post("/auth/verify", util.Wrap(auth.Verify))
	r.With(s.requireSession).Post("/auth/link", util.Wrap(auth.Link))

	return r
}

// Health godoc
// @Summary Liveness probe
// @Tags health
// @Produce json
// @Success 200 {object} util.ApiSuccess{data=map[string]string}
// @Router /health [get]
func (s *Server) Health(w http.ResponseWriter, r *http.Request) error {
	util.WriteJSON(w, http.StatusOK, util.ApiSuccess{Data: map[string]string{"status": "healthy"}})
	return nil
}
