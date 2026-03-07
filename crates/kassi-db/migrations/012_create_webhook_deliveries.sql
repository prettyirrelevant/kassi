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
