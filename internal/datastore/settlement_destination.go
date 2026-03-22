package datastore

import (
	"context"
	"fmt"

	"github.com/uptrace/bun"
)

func (s *Store) UpsertSettlementDestinations(ctx context.Context, merchantID, address string, networkIDs []string) ([]*SettlementDestination, error) {
	destinations := make([]*SettlementDestination, len(networkIDs))
	for i, nid := range networkIDs {
		destinations[i] = &SettlementDestination{
			ID:         NewSettlementDestinationID(),
			MerchantID: merchantID,
			NetworkID:  nid,
			Address:    address,
		}
	}

	if _, err := s.DB.NewInsert().
		Model(&destinations).
		On("CONFLICT (merchant_id, network_id) DO UPDATE").
		Set("address = EXCLUDED.address").
		Set("updated_at = now()").
		Exec(ctx); err != nil {
		return nil, fmt.Errorf("upserting settlement destinations: %w", err)
	}

	var result []*SettlementDestination
	if err := s.DB.NewSelect().
		Model(&result).
		Relation("Network").
		Where("settlement_destination.merchant_id = ?", merchantID).
		Where("settlement_destination.network_id IN (?)", bun.In(networkIDs)).
		OrderExpr("settlement_destination.created_at ASC").
		Scan(ctx); err != nil {
		return nil, fmt.Errorf("fetching upserted settlement destinations: %w", err)
	}

	return result, nil
}

func (s *Store) ListSettlementDestinations(ctx context.Context, merchantID string, page, perPage int) ([]*SettlementDestination, int, error) {
	destinations := make([]*SettlementDestination, 0)
	total, err := s.DB.NewSelect().
		Model(&destinations).
		Relation("Network").
		Where("settlement_destination.merchant_id = ?", merchantID).
		OrderExpr("settlement_destination.created_at ASC").
		Limit(perPage).
		Offset((page - 1) * perPage).
		ScanAndCount(ctx)
	if err != nil {
		return nil, 0, fmt.Errorf("listing settlement destinations for merchant %s: %w", merchantID, err)
	}
	return destinations, total, nil
}

func (s *Store) FindSettlementDestinationByID(ctx context.Context, id string) (*SettlementDestination, error) {
	dest := new(SettlementDestination)
	if err := s.DB.NewSelect().
		Model(dest).
		Relation("Network").
		Where("settlement_destination.id = ?", id).
		Scan(ctx); err != nil {
		return nil, fmt.Errorf("finding settlement destination %s: %w", id, err)
	}
	return dest, nil
}

func (s *Store) DeleteSettlementDestination(ctx context.Context, id string) error {
	if _, err := s.DB.NewDelete().
		Model((*SettlementDestination)(nil)).
		Where("id = ?", id).
		Exec(ctx); err != nil {
		return fmt.Errorf("deleting settlement destination %s: %w", id, err)
	}
	return nil
}
