CREATE TABLE networks (
    id              TEXT PRIMARY KEY,
    display_name    TEXT NOT NULL,
    block_time_ms   INTEGER NOT NULL,
    confirmations   INTEGER NOT NULL,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
