--- This file should undo anything in `up.sql`
DROP TABLE IF EXISTS nonces;
DROP TABLE IF EXISTS jobs;
DROP TABLE IF EXISTS price_cache;
DROP TABLE IF EXISTS webhook_deliveries;
DROP TABLE IF EXISTS ledger_entries;
DROP TABLE IF EXISTS quotes;
DROP TABLE IF EXISTS payment_intents;
DROP TABLE IF EXISTS network_addresses;
DROP TABLE IF EXISTS deposit_addresses;
DROP TABLE IF EXISTS assets;
DROP TABLE IF EXISTS signers;
DROP TABLE IF EXISTS settlement_destinations;
DROP TABLE IF EXISTS merchant_configs;
DROP TABLE IF EXISTS merchants;
DROP TABLE IF EXISTS networks;
