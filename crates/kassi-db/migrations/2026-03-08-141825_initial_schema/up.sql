-- Your SQL goes here
CREATE TABLE networks (
    id              TEXT PRIMARY KEY,
    display_name    TEXT NOT NULL,
    block_time_ms   INTEGER NOT NULL,
    confirmations   INTEGER NOT NULL,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
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
    api_key_hash    TEXT UNIQUE,
    webhook_secret  TEXT NOT NULL,
    webhook_url     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

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
    caip19           TEXT NOT NULL UNIQUE,
    contract_address TEXT,
    symbol           TEXT NOT NULL,
    name             TEXT NOT NULL,
    decimals         INTEGER NOT NULL,
    coingecko_id     TEXT,
    is_active        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
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
    fiat_amount        TEXT NOT NULL,
    fiat_currency      TEXT NOT NULL,
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

CREATE TABLE quotes (
    id                TEXT PRIMARY KEY,
    payment_intent_id TEXT NOT NULL REFERENCES payment_intents(id),
    asset_id          TEXT NOT NULL REFERENCES assets(id),
    exchange_rate     TEXT NOT NULL,
    crypto_amount     TEXT NOT NULL,
    expires_at        TIMESTAMPTZ NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_quotes_payment_intent ON quotes(payment_intent_id);

CREATE TABLE ledger_entries (
    id                 TEXT PRIMARY KEY,
    deposit_address_id TEXT NOT NULL REFERENCES deposit_addresses(id),
    payment_intent_id  TEXT REFERENCES payment_intents(id),
    asset_id           TEXT NOT NULL REFERENCES assets(id),
    network_id         TEXT NOT NULL REFERENCES networks(id),
    entry_type         TEXT NOT NULL,
    status             TEXT NOT NULL DEFAULT 'pending',
    amount             TEXT NOT NULL,
    fee_amount         TEXT,
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

CREATE TABLE price_cache (
    id              TEXT PRIMARY KEY,
    asset_id        TEXT NOT NULL REFERENCES assets(id),
    fiat_currency   TEXT NOT NULL,
    price           TEXT NOT NULL,
    source          TEXT NOT NULL,
    fetched_at      TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_price_cache_lookup ON price_cache(asset_id, fiat_currency, fetched_at DESC);

CREATE TABLE jobs (
    id              BIGSERIAL PRIMARY KEY,
    queue           TEXT NOT NULL,
    payload         JSONB NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    attempts        INTEGER NOT NULL DEFAULT 0,
    max_attempts    INTEGER NOT NULL DEFAULT 5,
    scheduled_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    failed_at       TIMESTAMPTZ,
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_job_status CHECK (status IN ('pending', 'running', 'completed', 'failed', 'dead'))
);

CREATE INDEX idx_jobs_poll ON jobs(queue, scheduled_at) WHERE status = 'pending';

CREATE TABLE nonces (
    id          BIGSERIAL PRIMARY KEY,
    nonce       TEXT NOT NULL UNIQUE,
    expires_at  TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_nonces_expires ON nonces(expires_at);
