CREATE TABLE ledger_entries (
    id                 TEXT PRIMARY KEY,
    deposit_address_id TEXT NOT NULL REFERENCES deposit_addresses(id),
    payment_intent_id  TEXT REFERENCES payment_intents(id),
    asset_id           TEXT NOT NULL REFERENCES assets(id),
    network_id         TEXT NOT NULL REFERENCES networks(id),
    entry_type         TEXT NOT NULL,
    status             TEXT NOT NULL DEFAULT 'pending',
    amount             TEXT NOT NULL,
    fee_amount         TEXT,
    sender             TEXT,
    destination        TEXT,
    onchain_ref        TEXT NOT NULL UNIQUE,
    reason             TEXT,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_ledger_entry_type
        CHECK (entry_type IN ('deposit', 'sweep', 'refund')),
    CONSTRAINT chk_ledger_entry_status
        CHECK (status IN ('pending', 'confirmed', 'reverted'))
);

CREATE INDEX idx_ledger_entries_deposit_address ON ledger_entries(deposit_address_id);
CREATE INDEX idx_ledger_entries_payment_intent ON ledger_entries(payment_intent_id);
CREATE INDEX idx_ledger_entries_type ON ledger_entries(entry_type);
CREATE INDEX idx_ledger_entries_deposit_address_created ON ledger_entries(deposit_address_id, created_at, id);
