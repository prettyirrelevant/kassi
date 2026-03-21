package main

import (
	"fmt"

	"github.com/spf13/cobra"

	"github.com/prettyirrelevant/kassi/internal/config"
	"github.com/prettyirrelevant/kassi/internal/datastore"
)

func init() {
	rootCmd.AddCommand(migrateCmd)
}

var migrateCmd = &cobra.Command{
	Use:   "migrate",
	Short: "run database migrations",
	RunE: func(cmd *cobra.Command, args []string) error {
		cfg, err := config.Load()
		if err != nil {
			return fmt.Errorf("loading config: %w", err)
		}

		store, err := datastore.NewStore(cfg.DatabaseURL)
		if err != nil {
			return fmt.Errorf("connecting to database: %w", err)
		}
		defer store.Close()

		if err := store.Migrate(cmd.Context()); err != nil {
			return fmt.Errorf("running migrations: %w", err)
		}

		fmt.Println("migrations complete")
		return nil
	},
}
