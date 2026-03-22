package datastore

import (
	"encoding/json"
	"time"

	"github.com/shopspring/decimal"
	"github.com/uptrace/bun"
)

type Network struct {
	bun.BaseModel `bun:"table:networks" json:"-"`

	ID            string    `bun:"id,pk" json:"id"`
	ChainType     string    `bun:"chain_type,notnull" json:"chain_type"`
	DisplayName   string    `bun:"display_name,notnull" json:"display_name"`
	Confirmations int       `bun:"confirmations,notnull" json:"confirmations"`
	IsActive      bool      `bun:"is_active,notnull,default:true" json:"is_active"`
	CreatedAt     time.Time `bun:"created_at,notnull,default:current_timestamp" json:"created_at"`
}

type Merchant struct {
	bun.BaseModel `bun:"table:merchants" json:"-"`

	ID        string    `bun:"id,pk" json:"id"`
	Name      *string   `bun:"name" json:"name"`
	CreatedAt time.Time `bun:"created_at,notnull,default:current_timestamp" json:"created_at"`
	UpdatedAt time.Time `bun:"updated_at,notnull,default:current_timestamp" json:"updated_at"`

	Config *MerchantConfig `bun:"rel:has-one,join:id=merchant_id" json:"config,omitempty"`
}

type MerchantConfig struct {
	bun.BaseModel `bun:"table:merchant_configs" json:"-"`

	ID            string    `bun:"id,pk" json:"id"`
	MerchantID    string    `bun:"merchant_id,notnull" json:"merchant_id"`
	PublicKeyHash *string   `bun:"public_key_hash" json:"public_key_hash"`
	SecretKeyHash *string   `bun:"secret_key_hash" json:"secret_key_hash"`
	EncryptedSeed *string   `bun:"encrypted_seed" json:"-"`
	WebhookSecret string    `bun:"webhook_secret,notnull" json:"webhook_secret"`
	WebhookURL    *string   `bun:"webhook_url" json:"webhook_url"`
	CreatedAt     time.Time `bun:"created_at,notnull,default:current_timestamp" json:"created_at"`
	UpdatedAt     time.Time `bun:"updated_at,notnull,default:current_timestamp" json:"updated_at"`
}

type SettlementDestination struct {
	bun.BaseModel `bun:"table:settlement_destinations" json:"-"`

	ID         string    `bun:"id,pk" json:"id"`
	MerchantID string    `bun:"merchant_id,notnull" json:"merchant_id"`
	NetworkID  string    `bun:"network_id,notnull" json:"network_id"`
	Address    string    `bun:"address,notnull" json:"address"`
	CreatedAt  time.Time `bun:"created_at,notnull,default:current_timestamp" json:"created_at"`
	UpdatedAt  time.Time `bun:"updated_at,notnull,default:current_timestamp" json:"updated_at"`

	Network *Network `bun:"rel:belongs-to,join:network_id=id" json:"network,omitempty"`
}

type Signer struct {
	bun.BaseModel `bun:"table:signers" json:"-"`

	ID         string    `bun:"id,pk" json:"id"`
	MerchantID string    `bun:"merchant_id,notnull" json:"merchant_id"`
	Address    string    `bun:"address,notnull" json:"address"`
	SignerType string    `bun:"signer_type,notnull" json:"signer_type"`
	LinkedAt   time.Time `bun:"linked_at,notnull,default:current_timestamp" json:"linked_at"`
}

type Asset struct {
	bun.BaseModel `bun:"table:assets" json:"-"`

	ID              string    `bun:"id,pk" json:"id"`
	NetworkID       string    `bun:"network_id,notnull" json:"network_id"`
	ContractAddress *string   `bun:"contract_address" json:"contract_address"`
	Symbol          string    `bun:"symbol,notnull" json:"symbol"`
	Name            string    `bun:"name,notnull" json:"name"`
	Decimals        int       `bun:"decimals,notnull" json:"decimals"`
	CoingeckoID     *string   `bun:"coingecko_id" json:"coingecko_id"`
	IsActive        bool      `bun:"is_active,notnull,default:true" json:"is_active"`
	CreatedAt       time.Time `bun:"created_at,notnull,default:current_timestamp" json:"created_at"`

	Network *Network `bun:"rel:belongs-to,join:network_id=id" json:"network,omitempty"`
}

type DepositAddress struct {
	bun.BaseModel `bun:"table:deposit_addresses" json:"-"`

	ID          string    `bun:"id,pk" json:"id"`
	MerchantID  string    `bun:"merchant_id,notnull" json:"merchant_id"`
	Label       *string   `bun:"label" json:"label"`
	AddressType string    `bun:"address_type,notnull" json:"address_type"`
	CreatedAt   time.Time `bun:"created_at,notnull,default:current_timestamp" json:"created_at"`

	NetworkAddresses []NetworkAddress `bun:"rel:has-many,join:id=deposit_address_id" json:"network_addresses,omitempty"`
}

type NetworkAddress struct {
	bun.BaseModel `bun:"table:network_addresses" json:"-"`

	ID               string `bun:"id,pk" json:"id"`
	DepositAddressID string `bun:"deposit_address_id,notnull" json:"deposit_address_id"`
	NetworkID        string `bun:"network_id,notnull" json:"network_id"`
	Address          string `bun:"address,notnull" json:"address"`
	DerivationIndex  int    `bun:"derivation_index,notnull" json:"-"`

	Network *Network `bun:"rel:belongs-to,join:network_id=id" json:"network,omitempty"`
}

type LockedRate struct {
	ExchangeRate decimal.Decimal `json:"exchange_rate"`
	CryptoAmount decimal.Decimal `json:"crypto_amount"`
}

type PaymentIntent struct {
	bun.BaseModel `bun:"table:payment_intents" json:"-"`

	ID               string                `bun:"id,pk" json:"id"`
	DepositAddressID string                `bun:"deposit_address_id,notnull" json:"deposit_address_id"`
	MerchantID       string                `bun:"merchant_id,notnull" json:"merchant_id"`
	FiatAmount       decimal.Decimal       `bun:"fiat_amount,notnull,type:numeric" json:"fiat_amount"`
	FiatCurrency     string                `bun:"fiat_currency,notnull" json:"fiat_currency"`
	LockedRates      map[string]LockedRate `bun:"locked_rates,notnull,type:jsonb" json:"locked_rates"`
	Status           string                `bun:"status,notnull,default:'pending'" json:"status"`
	ConfirmedAt      *time.Time            `bun:"confirmed_at" json:"confirmed_at"`
	ExpiresAt        time.Time             `bun:"expires_at,notnull" json:"expires_at"`
	CreatedAt        time.Time             `bun:"created_at,notnull,default:current_timestamp" json:"created_at"`
	UpdatedAt        time.Time             `bun:"updated_at,notnull,default:current_timestamp" json:"updated_at"`

	DepositAddress *DepositAddress `bun:"rel:belongs-to,join:deposit_address_id=id" json:"deposit_address,omitempty"`
}

type LedgerEntry struct {
	bun.BaseModel `bun:"table:ledger_entries" json:"-"`

	ID               string           `bun:"id,pk" json:"id"`
	DepositAddressID string           `bun:"deposit_address_id,notnull" json:"deposit_address_id"`
	PaymentIntentID  *string          `bun:"payment_intent_id" json:"payment_intent_id"`
	AssetID          string           `bun:"asset_id,notnull" json:"asset_id"`
	NetworkID        string           `bun:"network_id,notnull" json:"network_id"`
	EntryType        string           `bun:"entry_type,notnull" json:"entry_type"`
	Status           string           `bun:"status,notnull,default:'pending'" json:"status"`
	Amount           decimal.Decimal  `bun:"amount,notnull,type:numeric" json:"amount"`
	FeeAmount        *decimal.Decimal `bun:"fee_amount,type:numeric" json:"fee_amount"`
	Sender           *string          `bun:"sender" json:"sender"`
	Destination      *string          `bun:"destination" json:"destination"`
	OnchainRef       string           `bun:"onchain_ref,notnull" json:"onchain_ref"`
	Reason           *string          `bun:"reason" json:"reason"`
	CreatedAt        time.Time        `bun:"created_at,notnull,default:current_timestamp" json:"created_at"`

	Asset *Asset `bun:"rel:belongs-to,join:asset_id=id" json:"asset,omitempty"`
}

type WebhookDelivery struct {
	bun.BaseModel `bun:"table:webhook_deliveries" json:"-"`

	ID            string          `bun:"id,pk" json:"id"`
	MerchantID    string          `bun:"merchant_id,notnull" json:"merchant_id"`
	EventType     string          `bun:"event_type,notnull" json:"event_type"`
	ReferenceID   string          `bun:"reference_id,notnull" json:"reference_id"`
	URL           string          `bun:"url,notnull" json:"url"`
	Payload       json.RawMessage `bun:"payload,notnull,type:jsonb" json:"payload"`
	Status        string          `bun:"status,notnull,default:'pending'" json:"status"`
	Attempts      int             `bun:"attempts,notnull,default:0" json:"attempts"`
	LastAttemptAt *time.Time      `bun:"last_attempt_at" json:"last_attempt_at"`
	ResponseCode  *int16          `bun:"response_code" json:"response_code"`
	CreatedAt     time.Time       `bun:"created_at,notnull,default:current_timestamp" json:"created_at"`
	UpdatedAt     time.Time       `bun:"updated_at,notnull,default:current_timestamp" json:"updated_at"`
}
