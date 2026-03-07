CREATE TABLE deposit_addresses (
    id              TEXT PRIMARY KEY,
    merchant_id     TEXT NOT NULL REFERENCES merchants(id),
    label           TEXT,
    address_type    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_deposit_addresses_merchant ON deposit_addresses(merchant_id);
