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
