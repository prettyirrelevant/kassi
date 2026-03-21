// @title kassi API
// @version 0.1.0
// @description wallets-as-a-service payment infrastructure API

// @securityDefinitions.apikey BearerAuth
// @in header
// @name Authorization

// @securityDefinitions.apikey APIKeyAuth
// @in header
// @name X-API-Key

package main

import (
	"fmt"
	"os"

	"github.com/spf13/cobra"
)

var (
	version    = "dev"
	commitHash = "unknown"
)

var rootCmd = &cobra.Command{
	Use:     "kassi",
	Short:   "kassi payment infrastructure",
	Version: fmt.Sprintf("%s (%s)", version, commitHash),
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
