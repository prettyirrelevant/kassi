// @generated automatically by Diesel CLI.

diesel::table! {
    assets (id) {
        id -> Text,
        network_id -> Text,
        caip19 -> Text,
        contract_address -> Nullable<Text>,
        symbol -> Text,
        name -> Text,
        decimals -> Int4,
        coingecko_id -> Nullable<Text>,
        is_active -> Bool,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    deposit_addresses (id) {
        id -> Text,
        merchant_id -> Text,
        label -> Nullable<Text>,
        address_type -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    jobs (id) {
        id -> Int8,
        queue -> Text,
        payload -> Jsonb,
        status -> Text,
        attempts -> Int4,
        max_attempts -> Int4,
        scheduled_at -> Timestamptz,
        started_at -> Nullable<Timestamptz>,
        completed_at -> Nullable<Timestamptz>,
        failed_at -> Nullable<Timestamptz>,
        last_error -> Nullable<Text>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    ledger_entries (id) {
        id -> Text,
        deposit_address_id -> Text,
        payment_intent_id -> Nullable<Text>,
        asset_id -> Text,
        network_id -> Text,
        entry_type -> Text,
        status -> Text,
        amount -> Text,
        fee_amount -> Nullable<Text>,
        sender -> Nullable<Text>,
        destination -> Nullable<Text>,
        onchain_ref -> Text,
        reason -> Nullable<Text>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    merchant_configs (id) {
        id -> Text,
        merchant_id -> Text,
        api_key_hash -> Nullable<Text>,
        webhook_secret -> Text,
        webhook_url -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    merchants (id) {
        id -> Text,
        name -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    network_addresses (id) {
        id -> Text,
        deposit_address_id -> Text,
        network_id -> Text,
        address -> Text,
        derivation_index -> Int4,
    }
}

diesel::table! {
    networks (id) {
        id -> Text,
        display_name -> Text,
        block_time_ms -> Int4,
        confirmations -> Int4,
        is_active -> Bool,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    nonces (id) {
        id -> Int8,
        nonce -> Text,
        expires_at -> Timestamptz,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    payment_intents (id) {
        id -> Text,
        deposit_address_id -> Text,
        merchant_id -> Text,
        fiat_amount -> Text,
        fiat_currency -> Text,
        status -> Text,
        confirmed_at -> Nullable<Timestamptz>,
        expires_at -> Timestamptz,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    price_cache (id) {
        id -> Text,
        asset_id -> Text,
        fiat_currency -> Text,
        price -> Text,
        source -> Text,
        fetched_at -> Timestamptz,
    }
}

diesel::table! {
    quotes (id) {
        id -> Text,
        payment_intent_id -> Text,
        asset_id -> Text,
        exchange_rate -> Text,
        crypto_amount -> Text,
        expires_at -> Timestamptz,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    settlement_destinations (id) {
        id -> Text,
        merchant_id -> Text,
        network_id -> Text,
        address -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    signers (id) {
        id -> Text,
        merchant_id -> Text,
        address -> Text,
        signer_type -> Text,
        linked_at -> Timestamptz,
    }
}

diesel::table! {
    webhook_deliveries (id) {
        id -> Text,
        merchant_id -> Text,
        event_type -> Text,
        reference_id -> Text,
        url -> Text,
        payload -> Jsonb,
        status -> Text,
        attempts -> Int4,
        last_attempt_at -> Nullable<Timestamptz>,
        response_code -> Nullable<Int2>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::joinable!(assets -> networks (network_id));
diesel::joinable!(deposit_addresses -> merchants (merchant_id));
diesel::joinable!(ledger_entries -> assets (asset_id));
diesel::joinable!(ledger_entries -> deposit_addresses (deposit_address_id));
diesel::joinable!(ledger_entries -> networks (network_id));
diesel::joinable!(ledger_entries -> payment_intents (payment_intent_id));
diesel::joinable!(merchant_configs -> merchants (merchant_id));
diesel::joinable!(network_addresses -> deposit_addresses (deposit_address_id));
diesel::joinable!(network_addresses -> networks (network_id));
diesel::joinable!(payment_intents -> deposit_addresses (deposit_address_id));
diesel::joinable!(payment_intents -> merchants (merchant_id));
diesel::joinable!(price_cache -> assets (asset_id));
diesel::joinable!(quotes -> assets (asset_id));
diesel::joinable!(quotes -> payment_intents (payment_intent_id));
diesel::joinable!(settlement_destinations -> merchants (merchant_id));
diesel::joinable!(settlement_destinations -> networks (network_id));
diesel::joinable!(signers -> merchants (merchant_id));
diesel::joinable!(webhook_deliveries -> merchants (merchant_id));

diesel::allow_tables_to_appear_in_same_query!(
    assets,
    deposit_addresses,
    jobs,
    ledger_entries,
    merchant_configs,
    merchants,
    network_addresses,
    networks,
    nonces,
    payment_intents,
    price_cache,
    quotes,
    settlement_destinations,
    signers,
    webhook_deliveries,
);
