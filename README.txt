kassi
=====

wallets-as-a-service platform. merchants integrate once, get deposit addresses
across EVM and Solana chains. deposits are detected, confirmed, swept (with fees),
and settled to merchant-configured destinations. the whole lifecycle is managed
through a REST API and a react dashboard.

stack: rust (axum, sqlx, postgres), react + vite


data flow
---------

    merchant
      |
      v
    POST /deposit-addresses
      |
      v
    kassi-signer (vault + HD derivation)
      |
      v
    deposit address created per active network
      |
      v
    on-chain indexers (ponder for EVM, carbon for Solana)
      |
      v
    POST /internal/deposits (deposit notification)
      |
      v
    deposit worker -> confirmation worker -> sweep worker
      |                   |                      |
      v                   v                      v
    ledger entry      status update         fee split + transfer
    (pending)         (confirmed)           (fee recipient + settlement destination)
      |                                         |
      v                                         v
    webhook fired                           ledger entry (sweep)
    (deposit.pending)                       webhook fired (deposit.swept)


payment intents
---------------

    merchant creates payment intent with fiat amount (e.g. "10.00 USD")
         |
         v
    kassi-tokens fetches current price (defillama, coingecko fallback)
         |
         v
    quote created with locked exchange rate + crypto amount
         |
         v
    customer sends crypto to one-off deposit address
         |
         v
    same deposit -> confirmation -> sweep pipeline


fee calculation
---------------

    fees taken at sweep time, not deposit time.

    fee_before_cap = floor(amount * fee_bps / 10000)
    cap_in_tokens  = floor(fee_cap_usd / exchange_rate * 10^decimals)
    fee            = min(fee_before_cap, cap_in_tokens)

    all math in token's smallest unit. exchange rates use rust_decimal
    for exact arithmetic (no floats).

    fee config via env vars: FEE_BPS, FEE_CAP_USD, FEE_RECIPIENT_EVM,
    FEE_RECIPIENT_SOLANA.


sweep mechanics
---------------

    EVM:    Multicall3 batch from deposit EOA
              1. transfer(feeRecipient, feeAmount)
              2. transfer(settlementDestination, remainder)

    Solana: single transaction, two signers (deposit keypair + relayer fee payer)
              instructions:
              1. transfer fee to fee recipient ATA
              2. transfer remainder to settlement ATA
              3. close source ATA (one-off addresses only)


project structure
-----------------

    crates/
      kassi-types/      CAIP identifiers (CAIP-2, CAIP-10, CAIP-19), prefixed entity IDs
      kassi-db/         schema migrations + connection pool (no queries)
      kassi-tokens/     price fetching (defillama + coingecko), fee calculation
      kassi-jobs/       job worker framework (poll, retry, dead-letter)
      kassi-signer/     vault client, HD derivation, transaction signing
      kassi-server/     axum routes, business logic, db queries, row structs

    indexers/
      evm/              ponder project (ERC-20 + native ETH transfers)
      solana/           carbon project (SPL + native SOL transfers)

    dashboard/          react + vite


environment variables
---------------------

    DATABASE_URL            postgres connection string
    VAULT_ADDR              hashicorp vault address
    VAULT_TOKEN             vault token
    FEE_BPS                 fee rate in basis points (100 = 1%)
    FEE_CAP_USD             max fee in USD (omit to disable cap)
    FEE_RECIPIENT_EVM       EVM address receiving fees
    FEE_RECIPIENT_SOLANA    solana pubkey receiving fees
    QUOTE_LOCK_DURATION     quote expiry duration (default 15-30 min)
    JWT_SECRET              JWT signing secret
    INTERNAL_BEARER_TOKEN   bearer token for indexer endpoints
    ADMIN_BASIC_AUTH        basic auth for admin endpoints
