package testutil

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/go-testfixtures/testfixtures/v3"
	"github.com/testcontainers/testcontainers-go"
	"github.com/testcontainers/testcontainers-go/modules/postgres"
	tcredis "github.com/testcontainers/testcontainers-go/modules/redis"
	"github.com/testcontainers/testcontainers-go/wait"

	"github.com/prettyirrelevant/kassi/internal/cache"
	"github.com/prettyirrelevant/kassi/internal/config"
	"github.com/prettyirrelevant/kassi/internal/datastore"
)

var Infra *TestInfra

type TestInfra struct {
	Store    *datastore.Store
	Cache    *cache.Cache
	Config   *config.Config
	Fixtures *testfixtures.Loader
}

func (ti *TestInfra) LoadFixtures() error {
	return ti.Fixtures.Load()
}

func Setup(m *testing.M, fixturesDir string) {
	ctx := context.Background()

	pg, err := postgres.Run(ctx, "postgres:17-alpine",
		postgres.WithDatabase("kassi_test"),
		postgres.WithUsername("kassi"),
		postgres.WithPassword("kassi"),
		testcontainers.WithWaitStrategy(wait.ForListeningPort("5432/tcp")),
	)
	if err != nil {
		fmt.Fprintf(os.Stderr, "starting postgres container: %v\n", err)
		os.Exit(1)
	}

	pgURL, err := pg.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		fmt.Fprintf(os.Stderr, "getting postgres URL: %v\n", err)
		os.Exit(1)
	}

	store, err := datastore.NewStore(pgURL)
	if err != nil {
		fmt.Fprintf(os.Stderr, "connecting to postgres: %v\n", err)
		os.Exit(1)
	}

	if err := store.Migrate(ctx); err != nil {
		fmt.Fprintf(os.Stderr, "running migrations: %v\n", err)
		os.Exit(1)
	}

	rd, err := tcredis.Run(ctx, "redis:7-alpine",
		testcontainers.WithWaitStrategy(wait.ForListeningPort("6379/tcp")),
	)
	if err != nil {
		fmt.Fprintf(os.Stderr, "starting redis container: %v\n", err)
		os.Exit(1)
	}

	redisURL, err := rd.ConnectionString(ctx)
	if err != nil {
		fmt.Fprintf(os.Stderr, "getting redis URL: %v\n", err)
		os.Exit(1)
	}

	c, err := cache.New(redisURL)
	if err != nil {
		fmt.Fprintf(os.Stderr, "connecting to redis: %v\n", err)
		os.Exit(1)
	}

	fixtures, err := testfixtures.New(
		testfixtures.Database(store.DB.DB),
		testfixtures.Dialect("postgres"),
		testfixtures.Directory(fixturesDir),
	)
	if err != nil {
		fmt.Fprintf(os.Stderr, "creating fixture loader: %v\n", err)
		os.Exit(1)
	}

	Infra = &TestInfra{
		Store: store,
		Cache: c,
		Config: &config.Config{
			Deployment:            "test",
			JWTSecret:             "test-jwt-secret-key-for-signing",
			JWTExpiry:             7 * 24 * time.Hour,
			DatabaseURL:           pgURL,
			RedisURL:              redisURL,
			Port:                  "3000",
			InfisicalClientID:     "test",
			InfisicalClientSecret: "test",
			InfisicalProjectID:    "test",
			QuoteLockDuration:     30 * time.Minute,
			PriceCacheTTL:         5 * time.Minute,
		},
		Fixtures: fixtures,
	}

	code := m.Run()

	store.Close()
	c.Close()
	_ = pg.Terminate(ctx)
	_ = rd.Terminate(ctx)

	os.Exit(code)
}
