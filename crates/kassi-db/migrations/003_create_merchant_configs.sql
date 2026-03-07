CREATE TABLE merchant_configs (
    id              TEXT PRIMARY KEY,
    merchant_id     TEXT NOT NULL UNIQUE REFERENCES merchants(id),
    api_key_hash    TEXT UNIQUE,
    webhook_secret  TEXT NOT NULL,
    webhook_url     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
