package datastore

import (
	"context"
	"fmt"

	"github.com/uptrace/bun"

	"github.com/prettyirrelevant/kassi/internal/helpers"
)

func (s *Store) FindMerchantByID(ctx context.Context, id string) (*Merchant, error) {
	merchant := new(Merchant)
	if err := s.DB.NewSelect().
		Model(merchant).
		Relation("Config").
		Where("merchant.id = ?", id).
		Scan(ctx); err != nil {
		return nil, fmt.Errorf("finding merchant %s: %w", id, err)
	}
	return merchant, nil
}

func (s *Store) FindMerchantByPublicKeyHash(ctx context.Context, hash string) (*Merchant, error) {
	merchant := new(Merchant)
	if err := s.DB.NewSelect().
		Model(merchant).
		Relation("Config").
		Join("JOIN merchant_configs AS mc ON mc.merchant_id = merchant.id").
		Where("mc.public_key_hash = ?", hash).
		Scan(ctx); err != nil {
		return nil, fmt.Errorf("finding merchant by public key hash: %w", err)
	}
	return merchant, nil
}

func (s *Store) FindMerchantBySecretKeyHash(ctx context.Context, hash string) (*Merchant, error) {
	merchant := new(Merchant)
	if err := s.DB.NewSelect().
		Model(merchant).
		Relation("Config").
		Join("JOIN merchant_configs AS mc ON mc.merchant_id = merchant.id").
		Where("mc.secret_key_hash = ?", hash).
		Scan(ctx); err != nil {
		return nil, fmt.Errorf("finding merchant by secret key hash: %w", err)
	}
	return merchant, nil
}

func (s *Store) FindSignerByAddress(ctx context.Context, address string) (*Signer, error) {
	sgn := new(Signer)
	if err := s.DB.NewSelect().
		Model(sgn).
		Where("address = ?", address).
		Scan(ctx); err != nil {
		return nil, fmt.Errorf("finding signer by address %s: %w", address, err)
	}
	return sgn, nil
}

func (s *Store) CreateMerchantWithConfig(ctx context.Context, address, signerType string) (*Merchant, error) {
	merchant := &Merchant{ID: NewMerchantID()}

	cfg := &MerchantConfig{
		ID:            NewMerchantConfigID(),
		MerchantID:    merchant.ID,
		WebhookSecret: helpers.RandomString("whsec_", 32),
	}

	sgn := &Signer{
		ID:         NewSignerID(),
		MerchantID: merchant.ID,
		Address:    address,
		SignerType: signerType,
	}

	if err := s.DB.RunInTx(ctx, nil, func(ctx context.Context, tx bun.Tx) error {
		if _, err := tx.NewInsert().Model(merchant).Exec(ctx); err != nil {
			return fmt.Errorf("inserting merchant: %w", err)
		}
		if _, err := tx.NewInsert().Model(cfg).Exec(ctx); err != nil {
			return fmt.Errorf("inserting merchant config: %w", err)
		}
		if _, err := tx.NewInsert().Model(sgn).Exec(ctx); err != nil {
			return fmt.Errorf("inserting signer: %w", err)
		}
		return nil
	}); err != nil {
		return nil, fmt.Errorf("creating merchant with config: %w", err)
	}

	merchant.Config = cfg
	return merchant, nil
}

func (s *Store) UpdateMerchant(ctx context.Context, merchantID string, name *string, webhookURL *string) (*Merchant, error) {
	if err := s.DB.RunInTx(ctx, nil, func(ctx context.Context, tx bun.Tx) error {
		_, err := tx.NewUpdate().
			Model((*Merchant)(nil)).
			Set("name = ?", name).
			Set("updated_at = now()").
			Where("id = ?", merchantID).
			Exec(ctx)
		if err != nil {
			return fmt.Errorf("updating merchant: %w", err)
		}

		_, err = tx.NewUpdate().
			Model((*MerchantConfig)(nil)).
			Set("webhook_url = ?", webhookURL).
			Set("updated_at = now()").
			Where("merchant_id = ?", merchantID).
			Exec(ctx)
		if err != nil {
			return fmt.Errorf("updating merchant config: %w", err)
		}

		return nil
	}); err != nil {
		return nil, fmt.Errorf("updating merchant %s: %w", merchantID, err)
	}

	return s.FindMerchantByID(ctx, merchantID)
}

func (s *Store) UpdateSecretKeyHash(ctx context.Context, merchantID, hash string) error {
	_, err := s.DB.NewUpdate().
		Model((*MerchantConfig)(nil)).
		Set("secret_key_hash = ?", hash).
		Set("updated_at = now()").
		Where("merchant_id = ?", merchantID).
		Exec(ctx)
	if err != nil {
		return fmt.Errorf("updating secret key hash for merchant %s: %w", merchantID, err)
	}
	return nil
}

func (s *Store) UpdatePublicKeyHash(ctx context.Context, merchantID, hash string) error {
	_, err := s.DB.NewUpdate().
		Model((*MerchantConfig)(nil)).
		Set("public_key_hash = ?", hash).
		Set("updated_at = now()").
		Where("merchant_id = ?", merchantID).
		Exec(ctx)
	if err != nil {
		return fmt.Errorf("updating public key hash for merchant %s: %w", merchantID, err)
	}
	return nil
}

func (s *Store) UpdateWebhookSecret(ctx context.Context, merchantID, secret string) error {
	_, err := s.DB.NewUpdate().
		Model((*MerchantConfig)(nil)).
		Set("webhook_secret = ?", secret).
		Set("updated_at = now()").
		Where("merchant_id = ?", merchantID).
		Exec(ctx)
	if err != nil {
		return fmt.Errorf("updating webhook secret for merchant %s: %w", merchantID, err)
	}
	return nil
}

func (s *Store) CreateSigner(ctx context.Context, merchantID, address, signerType string) (*Signer, error) {
	sgn := &Signer{
		ID:         NewSignerID(),
		MerchantID: merchantID,
		Address:    address,
		SignerType: signerType,
	}
	if _, err := s.DB.NewInsert().Model(sgn).Exec(ctx); err != nil {
		return nil, fmt.Errorf("inserting signer: %w", err)
	}
	return sgn, nil
}
