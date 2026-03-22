package datastore

import (
	"context"
	"fmt"

	"github.com/uptrace/bun"
)

func (s *Store) FindNetworksByIDs(ctx context.Context, ids []string) ([]*Network, error) {
	var networks []*Network
	if err := s.DB.NewSelect().
		Model(&networks).
		Where("id IN (?)", bun.List(ids)).
		Scan(ctx); err != nil {
		return nil, fmt.Errorf("finding networks by ids: %w", err)
	}
	return networks, nil
}
