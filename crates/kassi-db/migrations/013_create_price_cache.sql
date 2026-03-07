CREATE TABLE price_cache (
    id              TEXT PRIMARY KEY,
    asset_id        TEXT NOT NULL REFERENCES assets(id),
    fiat_currency   TEXT NOT NULL,
    price           TEXT NOT NULL,
    source          TEXT NOT NULL,
    fetched_at      TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_price_cache_lookup ON price_cache(asset_id, fiat_currency, fetched_at DESC);
