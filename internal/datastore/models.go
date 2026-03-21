package datastore

import (
	"encoding/json"
	"time"

	"github.com/shopspring/decimal"
	"github.com/uptrace/bun"
)

type Network struct {
	bun.BaseModel `bun:"table:networks"`

	ID            string    `bun:"id,pk"`
	ChainType     string    `bun:"chain_type,notnull"`
	DisplayName   string    `bun:"display_name,notnull"`
	Confirmations int       `bun:"confirmations,notnull"`
	IsActive      bool      `bun:"is_active,notnull,default:true"`
	CreatedAt     time.Time `bun:"created_at,notnull,default:current_timestamp"`
}

type Merchant struct {
	bun.BaseModel `bun:"table:merchants"`

	ID        string    `bun:"id,pk"`
	Name      *string   `bun:"name"`
	CreatedAt time.Time `bun:"created_at,notnull,default:current_timestamp"`
	UpdatedAt time.Time `bun:"updated_at,notnull,default:current_timestamp"`

	Config *MerchantConfig `bun:"rel:has-one,join:id=merchant_id"`
}

type MerchantConfig struct {
	bun.BaseModel `bun:"table:merchant_configs"`

	ID            string    `bun:"id,pk"`
	MerchantID    string    `bun:"merchant_id,notnull"`
	PublicKeyHash *string   `bun:"public_key_hash"`
	SecretKeyHash *string   `bun:"secret_key_hash"`
	EncryptedSeed *string   `bun:"encrypted_seed"`
	WebhookSecret string    `bun:"webhook_secret,notnull"`
	WebhookURL    *string   `bun:"webhook_url"`
	CreatedAt     time.Time `bun:"created_at,notnull,default:current_timestamp"`
	UpdatedAt     time.Time `bun:"updated_at,notnull,default:current_timestamp"`
}

type SettlementDestination struct {
	bun.BaseModel `bun:"table:settlement_destinations"`

	ID         string    `bun:"id,pk"`
	MerchantID string    `bun:"merchant_id,notnull"`
	NetworkID  string    `bun:"network_id,notnull"`
	Address    string    `bun:"address,notnull"`
	CreatedAt  time.Time `bun:"created_at,notnull,default:current_timestamp"`
	UpdatedAt  time.Time `bun:"updated_at,notnull,default:current_timestamp"`

	Network *Network `bun:"rel:belongs-to,join:network_id=id"`
}

type Signer struct {
	bun.BaseModel `bun:"table:signers"`

	ID         string    `bun:"id,pk"`
	MerchantID string    `bun:"merchant_id,notnull"`
	Address    string    `bun:"address,notnull"`
	SignerType string    `bun:"signer_type,notnull"`
	LinkedAt   time.Time `bun:"linked_at,notnull,default:current_timestamp"`
}

type Asset struct {
	bun.BaseModel `bun:"table:assets"`

	ID              string    `bun:"id,pk"`
	NetworkID       string    `bun:"network_id,notnull"`
	ContractAddress *string   `bun:"contract_address"`
	Symbol          string    `bun:"symbol,notnull"`
	Name            string    `bun:"name,notnull"`
	Decimals        int       `bun:"decimals,notnull"`
	CoingeckoID     *string   `bun:"coingecko_id"`
	IsActive        bool      `bun:"is_active,notnull,default:true"`
	CreatedAt       time.Time `bun:"created_at,notnull,default:current_timestamp"`

	Network *Network `bun:"rel:belongs-to,join:network_id=id"`
}

type DepositAddress struct {
	bun.BaseModel `bun:"table:deposit_addresses"`

	ID          string    `bun:"id,pk"`
	MerchantID  string    `bun:"merchant_id,notnull"`
	Label       *string   `bun:"label"`
	AddressType string    `bun:"address_type,notnull"`
	CreatedAt   time.Time `bun:"created_at,notnull,default:current_timestamp"`

	NetworkAddresses []NetworkAddress `bun:"rel:has-many,join:id=deposit_address_id"`
}

type NetworkAddress struct {
	bun.BaseModel `bun:"table:network_addresses"`

	ID               string `bun:"id,pk"`
	DepositAddressID string `bun:"deposit_address_id,notnull"`
	NetworkID        string `bun:"network_id,notnull"`
	Address          string `bun:"address,notnull"`
	DerivationIndex  int    `bun:"derivation_index,notnull"`

	Network *Network `bun:"rel:belongs-to,join:network_id=id"`
}

type LockedRate struct {
	ExchangeRate decimal.Decimal `json:"exchange_rate"`
	CryptoAmount decimal.Decimal `json:"crypto_amount"`
}

type PaymentIntent struct {
	bun.BaseModel `bun:"table:payment_intents"`

	ID               string                `bun:"id,pk"`
	DepositAddressID string                `bun:"deposit_address_id,notnull"`
	MerchantID       string                `bun:"merchant_id,notnull"`
	FiatAmount       decimal.Decimal       `bun:"fiat_amount,notnull,type:numeric"`
	FiatCurrency     string                `bun:"fiat_currency,notnull"`
	LockedRates      map[string]LockedRate `bun:"locked_rates,notnull,type:jsonb"`
	Status           string                `bun:"status,notnull,default:'pending'"`
	ConfirmedAt      *time.Time            `bun:"confirmed_at"`
	ExpiresAt        time.Time             `bun:"expires_at,notnull"`
	CreatedAt        time.Time             `bun:"created_at,notnull,default:current_timestamp"`
	UpdatedAt        time.Time             `bun:"updated_at,notnull,default:current_timestamp"`

	DepositAddress *DepositAddress `bun:"rel:belongs-to,join:deposit_address_id=id"`
}

type LedgerEntry struct {
	bun.BaseModel `bun:"table:ledger_entries"`

	ID               string           `bun:"id,pk"`
	DepositAddressID string           `bun:"deposit_address_id,notnull"`
	PaymentIntentID  *string          `bun:"payment_intent_id"`
	AssetID          string           `bun:"asset_id,notnull"`
	NetworkID        string           `bun:"network_id,notnull"`
	EntryType        string           `bun:"entry_type,notnull"`
	Status           string           `bun:"status,notnull,default:'pending'"`
	Amount           decimal.Decimal  `bun:"amount,notnull,type:numeric"`
	FeeAmount        *decimal.Decimal `bun:"fee_amount,type:numeric"`
	Sender           *string          `bun:"sender"`
	Destination      *string          `bun:"destination"`
	OnchainRef       string           `bun:"onchain_ref,notnull"`
	Reason           *string          `bun:"reason"`
	CreatedAt        time.Time        `bun:"created_at,notnull,default:current_timestamp"`

	Asset *Asset `bun:"rel:belongs-to,join:asset_id=id"`
}

type WebhookDelivery struct {
	bun.BaseModel `bun:"table:webhook_deliveries"`

	ID            string          `bun:"id,pk"`
	MerchantID    string          `bun:"merchant_id,notnull"`
	EventType     string          `bun:"event_type,notnull"`
	ReferenceID   string          `bun:"reference_id,notnull"`
	URL           string          `bun:"url,notnull"`
	Payload       json.RawMessage `bun:"payload,notnull,type:jsonb"`
	Status        string          `bun:"status,notnull,default:'pending'"`
	Attempts      int             `bun:"attempts,notnull,default:0"`
	LastAttemptAt *time.Time      `bun:"last_attempt_at"`
	ResponseCode  *int16          `bun:"response_code"`
	CreatedAt     time.Time       `bun:"created_at,notnull,default:current_timestamp"`
	UpdatedAt     time.Time       `bun:"updated_at,notnull,default:current_timestamp"`
}
