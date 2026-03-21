-- +goose Up

CREATE TABLE networks (
    id              TEXT PRIMARY KEY,
    chain_type      TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    confirmations   INTEGER NOT NULL,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_network_chain_type CHECK (chain_type IN ('evm', 'solana'))
);

CREATE TABLE merchants (
    id              TEXT PRIMARY KEY,
    name            TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE merchant_configs (
    id              TEXT PRIMARY KEY,
    merchant_id     TEXT NOT NULL UNIQUE REFERENCES merchants(id),
    public_key_hash TEXT,
    secret_key_hash TEXT,
    encrypted_seed  TEXT,
    webhook_secret  TEXT NOT NULL,
    webhook_url     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_merchant_configs_public_key ON merchant_configs(public_key_hash);
CREATE INDEX idx_merchant_configs_secret_key ON merchant_configs(secret_key_hash);

CREATE TABLE settlement_destinations (
    id              TEXT PRIMARY KEY,
    merchant_id     TEXT NOT NULL REFERENCES merchants(id),
    network_id      TEXT NOT NULL REFERENCES networks(id),
    address         TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (merchant_id, network_id)
);

CREATE TABLE signers (
    id              TEXT PRIMARY KEY,
    merchant_id     TEXT NOT NULL REFERENCES merchants(id),
    address         TEXT NOT NULL UNIQUE,
    signer_type     TEXT NOT NULL,
    linked_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_signers_merchant ON signers(merchant_id);

CREATE TABLE assets (
    id               TEXT PRIMARY KEY,
    network_id       TEXT NOT NULL REFERENCES networks(id),
    contract_address TEXT,
    symbol           TEXT NOT NULL,
    name             TEXT NOT NULL,
    decimals         INTEGER NOT NULL,
    coingecko_id     TEXT,
    is_active        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (network_id, contract_address)
);
CREATE INDEX idx_assets_network ON assets(network_id);
CREATE INDEX idx_assets_symbol ON assets(symbol);

CREATE TABLE deposit_addresses (
    id              TEXT PRIMARY KEY,
    merchant_id     TEXT NOT NULL REFERENCES merchants(id),
    label           TEXT,
    address_type    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_deposit_addresses_merchant ON deposit_addresses(merchant_id);

CREATE TABLE network_addresses (
    id                 TEXT PRIMARY KEY,
    deposit_address_id TEXT NOT NULL REFERENCES deposit_addresses(id),
    network_id         TEXT NOT NULL REFERENCES networks(id),
    address            TEXT NOT NULL,
    derivation_index   INTEGER NOT NULL,
    UNIQUE (deposit_address_id, network_id),
    UNIQUE (address, network_id)
);
CREATE INDEX idx_network_addresses_address ON network_addresses(address);

CREATE TABLE payment_intents (
    id                 TEXT PRIMARY KEY,
    deposit_address_id TEXT NOT NULL REFERENCES deposit_addresses(id),
    merchant_id        TEXT NOT NULL REFERENCES merchants(id),
    fiat_amount        NUMERIC NOT NULL,
    fiat_currency      TEXT NOT NULL,
    locked_rates       JSONB NOT NULL,
    status             TEXT NOT NULL DEFAULT 'pending',
    confirmed_at       TIMESTAMPTZ,
    expires_at         TIMESTAMPTZ NOT NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_payment_intent_status
        CHECK (status IN ('pending', 'partial', 'confirmed', 'expired'))
);
CREATE INDEX idx_payment_intents_merchant_status ON payment_intents(merchant_id, status);
CREATE INDEX idx_payment_intents_merchant_created ON payment_intents(merchant_id, created_at, id);

CREATE TABLE ledger_entries (
    id                 TEXT PRIMARY KEY,
    deposit_address_id TEXT NOT NULL REFERENCES deposit_addresses(id),
    payment_intent_id  TEXT REFERENCES payment_intents(id),
    asset_id           TEXT NOT NULL REFERENCES assets(id),
    network_id         TEXT NOT NULL REFERENCES networks(id),
    entry_type         TEXT NOT NULL,
    status             TEXT NOT NULL DEFAULT 'pending',
    amount             NUMERIC NOT NULL,
    fee_amount         NUMERIC,
    sender             TEXT,
    destination        TEXT,
    onchain_ref        TEXT NOT NULL UNIQUE,
    reason             TEXT,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_ledger_entry_type
        CHECK (entry_type IN ('deposit', 'sweep', 'refund')),
    CONSTRAINT chk_ledger_entry_status
        CHECK (status IN ('pending', 'confirmed', 'reverted'))
);
CREATE INDEX idx_ledger_entries_deposit_address ON ledger_entries(deposit_address_id);
CREATE INDEX idx_ledger_entries_payment_intent ON ledger_entries(payment_intent_id);
CREATE INDEX idx_ledger_entries_type ON ledger_entries(entry_type);
CREATE INDEX idx_ledger_entries_deposit_address_created ON ledger_entries(deposit_address_id, created_at, id);

CREATE TABLE webhook_deliveries (
    id              TEXT PRIMARY KEY,
    merchant_id     TEXT NOT NULL REFERENCES merchants(id),
    event_type      TEXT NOT NULL,
    reference_id    TEXT NOT NULL,
    url             TEXT NOT NULL,
    payload         JSONB NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    attempts        INTEGER NOT NULL DEFAULT 0,
    last_attempt_at TIMESTAMPTZ,
    response_code   SMALLINT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_webhook_status CHECK (status IN ('pending', 'sent', 'failed'))
);
CREATE INDEX idx_webhook_deliveries_merchant ON webhook_deliveries(merchant_id);
CREATE INDEX idx_webhook_deliveries_pending ON webhook_deliveries(status) WHERE status = 'pending';

-- +goose Down

DROP TABLE IF EXISTS webhook_deliveries;
DROP TABLE IF EXISTS ledger_entries;
DROP TABLE IF EXISTS payment_intents;
DROP TABLE IF EXISTS network_addresses;
DROP TABLE IF EXISTS deposit_addresses;
DROP TABLE IF EXISTS assets;
DROP TABLE IF EXISTS signers;
DROP TABLE IF EXISTS settlement_destinations;
DROP TABLE IF EXISTS merchant_configs;
DROP TABLE IF EXISTS merchants;
DROP TABLE IF EXISTS networks;
