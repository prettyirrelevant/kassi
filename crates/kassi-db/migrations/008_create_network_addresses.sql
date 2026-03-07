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
