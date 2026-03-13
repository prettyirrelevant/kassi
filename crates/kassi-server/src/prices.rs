use std::future::Future;
use std::pin::Pin;

use kassi_tokens::{PricingError, TokenPrice};

/// Object-safe price fetching trait for use in `AppState`.
pub trait PriceFetcher: Send + Sync {
    fn fetch_prices(
        &self,
        coingecko_ids: &[String],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<TokenPrice>, PricingError>> + Send + '_>>;
}

/// Production implementation wrapping `kassi_tokens::PriceClient::new()`.
pub struct LivePriceFetcher {
    client: kassi_tokens::PriceClient<kassi_tokens::DefiLlama, kassi_tokens::CoinGecko>,
}

impl LivePriceFetcher {
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: kassi_tokens::PriceClient::new(),
        }
    }
}

impl Default for LivePriceFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl PriceFetcher for LivePriceFetcher {
    fn fetch_prices(
        &self,
        coingecko_ids: &[String],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<TokenPrice>, PricingError>> + Send + '_>> {
        let ids: Vec<String> = coingecko_ids.to_vec();
        Box::pin(async move {
            let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
            self.client.get_prices(&refs).await
        })
    }
}
