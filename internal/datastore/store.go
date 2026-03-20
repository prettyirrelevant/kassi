package datastore

import (
	"context"
	"database/sql"
	"embed"
	"fmt"

	"github.com/pressly/goose/v3"
	"github.com/uptrace/bun"
	"github.com/uptrace/bun/dialect/pgdialect"
	"github.com/uptrace/bun/driver/pgdriver"
)

//go:embed migrations/*.sql
var migrations embed.FS

type Store struct {
	DB *bun.DB
}

func NewStore(dsn string) (*Store, error) {
	db := bun.NewDB(
		sql.OpenDB(pgdriver.NewConnector(pgdriver.WithDSN(dsn))),
		pgdialect.New(),
	)

	if err := db.Ping(); err != nil {
		return nil, fmt.Errorf("pinging database: %w", err)
	}

	return &Store{DB: db}, nil
}

func (s *Store) Migrate(ctx context.Context) error {
	goose.SetBaseFS(migrations)
	if err := goose.SetDialect("postgres"); err != nil {
		return fmt.Errorf("setting goose dialect: %w", err)
	}
	return goose.UpContext(ctx, s.DB.DB, "migrations")
}

func (s *Store) Close() error {
	return s.DB.Close()
}
