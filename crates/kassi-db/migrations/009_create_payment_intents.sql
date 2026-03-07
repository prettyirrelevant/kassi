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
