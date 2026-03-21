package main

import (
	"context"
	"fmt"
	"net/http"
	"os/signal"
	"syscall"
	"time"

	"github.com/spf13/cobra"
	"go.uber.org/zap"

	_ "github.com/prettyirrelevant/kassi/internal/docs"

	"github.com/prettyirrelevant/kassi/internal/cache"
	"github.com/prettyirrelevant/kassi/internal/config"
	"github.com/prettyirrelevant/kassi/internal/datastore"
	"github.com/prettyirrelevant/kassi/internal/pricing"
	"github.com/prettyirrelevant/kassi/internal/api"
	"github.com/prettyirrelevant/kassi/internal/signer"
)

func init() {
	rootCmd.AddCommand(serveCmd)
}

var serveCmd = &cobra.Command{
	Use:   "serve",
	Short: "start the HTTP server",
	RunE: func(cmd *cobra.Command, args []string) error {
		cfg, err := config.Load()
		if err != nil {
			return fmt.Errorf("loading config: %w", err)
		}

		logger, err := zap.NewProduction()
		if err != nil {
			return fmt.Errorf("creating logger: %w", err)
		}
		defer func() { _ = logger.Sync() }()

		store, err := datastore.NewStore(cfg.DatabaseURL)
		if err != nil {
			return fmt.Errorf("connecting to database: %w", err)
		}
		defer func() { _ = store.Close() }()

		redis, err := cache.New(cfg.RedisURL)
		if err != nil {
			return fmt.Errorf("connecting to redis: %w", err)
		}
		defer func() { _ = redis.Close() }()

		kms := signer.NewInfisicalKMS(
			cfg.InfisicalClientID,
			cfg.InfisicalClientSecret,
			cfg.InfisicalProjectID,
		)

		srv := api.New(
			store,
			kms,
			pricing.NewLiveOracle(),
			cfg,
			redis,
			logger,
		)

		httpServer := &http.Server{
			Addr:              ":" + cfg.Port,
			Handler:           srv.Routes(),
			ReadHeaderTimeout: 10 * time.Second,
		}

		ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
		defer stop()

		go func() {
			logger.Info("server starting", zap.String("port", cfg.Port))
			if err := httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
				logger.Fatal("server failed", zap.Error(err))
			}
		}()

		<-ctx.Done()
		logger.Info("shutting down")

		shutdownCtx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()

		if err := httpServer.Shutdown(shutdownCtx); err != nil {
			return fmt.Errorf("server shutdown: %w", err)
		}

		logger.Info("shutdown complete")
		return nil
	},
}
