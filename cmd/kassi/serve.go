package main

import (
	"context"
	"fmt"
	"os/signal"
	"syscall"
	"time"

	"github.com/labstack/echo/v5"
	"github.com/spf13/cobra"
	"go.uber.org/zap"

	_ "github.com/prettyirrelevant/kassi/internal/docs"

	"github.com/prettyirrelevant/kassi/internal/api"
	"github.com/prettyirrelevant/kassi/internal/cache"
	"github.com/prettyirrelevant/kassi/internal/config"
	"github.com/prettyirrelevant/kassi/internal/datastore"
	"github.com/prettyirrelevant/kassi/internal/pricing"
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

		ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
		defer stop()

		logger.Info("server starting", zap.String("port", cfg.Port))

		sc := echo.StartConfig{
			Address:         ":" + cfg.Port,
			GracefulTimeout: 30 * time.Second,
		}
		if err := sc.Start(ctx, srv.Echo()); err != nil {
			return fmt.Errorf("server error: %w", err)
		}

		logger.Info("shutdown complete")
		return nil
	},
}
