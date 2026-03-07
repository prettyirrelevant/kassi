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
