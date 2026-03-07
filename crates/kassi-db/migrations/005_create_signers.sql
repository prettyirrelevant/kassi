CREATE TABLE signers (
    id              TEXT PRIMARY KEY,
    merchant_id     TEXT NOT NULL REFERENCES merchants(id),
    address         TEXT NOT NULL UNIQUE,
    signer_type     TEXT NOT NULL,
    linked_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_signers_merchant ON signers(merchant_id);
