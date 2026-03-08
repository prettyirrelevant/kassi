use std::collections::HashMap;

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("no price found for token: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenPrice {
    pub coingecko_id: String,
    pub usd_price: f64,
}

/// A source that can fetch token prices by coingecko ID.
pub trait PriceSource: Send + Sync {
    fn get_prices(
        &self,
        coingecko_ids: &[&str],
    ) -> impl std::future::Future<Output = Result<Vec<TokenPrice>, PricingError>> + Send;
}

/// Fetches prices from `DefiLlama`'s `/prices/current/` endpoint.
pub struct DefiLlama {
    http: Client,
    base_url: String,
}

impl DefiLlama {
    fn new(client: Client) -> Self {
        Self {
            http: client,
            base_url: "https://coins.llama.fi".to_string(),
        }
    }

}

impl PriceSource for DefiLlama {
    async fn get_prices(&self, coingecko_ids: &[&str]) -> Result<Vec<TokenPrice>, PricingError> {
        let coins = coingecko_ids
            .iter()
            .map(|id| format!("coingecko:{id}"))
            .collect::<Vec<_>>()
            .join(",");

        let url = format!("{}/prices/current/{coins}", self.base_url);

        let resp: DefiLlamaResponse = self
            .http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        coingecko_ids
            .iter()
            .map(|id| {
                let key = format!("coingecko:{id}");
                let coin = resp
                    .coins
                    .get(&key)
                    .ok_or_else(|| PricingError::NotFound((*id).to_string()))?;
                Ok(TokenPrice {
                    coingecko_id: (*id).to_string(),
                    usd_price: coin.price,
                })
            })
            .collect()
    }
}

/// Fetches prices from `CoinGecko`'s `/simple/price` endpoint.
pub struct CoinGecko {
    http: Client,
    base_url: String,
}

impl CoinGecko {
    fn new(client: Client) -> Self {
        Self {
            http: client,
            base_url: "https://api.coingecko.com".to_string(),
        }
    }

}

impl PriceSource for CoinGecko {
    async fn get_prices(&self, coingecko_ids: &[&str]) -> Result<Vec<TokenPrice>, PricingError> {
        let ids = coingecko_ids.join(",");
        let url = format!(
            "{}/api/v3/simple/price?ids={ids}&vs_currencies=usd",
            self.base_url
        );

        let resp: HashMap<String, CoinGeckoEntry> = self
            .http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        coingecko_ids
            .iter()
            .map(|id| {
                let entry = resp
                    .get(*id)
                    .ok_or_else(|| PricingError::NotFound((*id).to_string()))?;
                Ok(TokenPrice {
                    coingecko_id: (*id).to_string(),
                    usd_price: entry.usd,
                })
            })
            .collect()
    }
}

/// Orchestrates price fetching with fallback: tries the primary source first,
/// falls back to the secondary on failure.
pub struct PriceClient<P: PriceSource, S: PriceSource> {
    primary: P,
    secondary: S,
}

impl PriceClient<DefiLlama, CoinGecko> {
    /// # Panics
    /// Panics if the HTTP client cannot be built (TLS backend unavailable).
    #[must_use]
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent(format!(
                "kassi/{} ({})",
                env!("CARGO_PKG_VERSION"),
                option_env!("KASSI_GIT_SHA").unwrap_or("dev"),
            ))
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(5)
            .build()
            .expect("failed to build HTTP client");

        Self {
            primary: DefiLlama::new(client.clone()),
            secondary: CoinGecko::new(client),
        }
    }
}

impl Default for PriceClient<DefiLlama, CoinGecko> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: PriceSource, S: PriceSource> PriceClient<P, S> {
    #[must_use]
    pub fn with_sources(primary: P, secondary: S) -> Self {
        Self { primary, secondary }
    }

    /// Fetch USD prices for the given coingecko IDs, falling back to the
    /// secondary source if the primary fails.
    ///
    /// # Errors
    ///
    /// Returns `PricingError` if both sources fail or a requested token is missing.
    pub async fn get_prices(
        &self,
        coingecko_ids: &[&str],
    ) -> Result<Vec<TokenPrice>, PricingError> {
        if coingecko_ids.is_empty() {
            return Ok(vec![]);
        }

        match self.primary.get_prices(coingecko_ids).await {
            Ok(prices) => Ok(prices),
            Err(e) => {
                tracing::warn!(error = %e, "primary price source failed, falling back to secondary");
                self.secondary.get_prices(coingecko_ids).await
            }
        }
    }
}

#[derive(Deserialize)]
struct DefiLlamaResponse {
    coins: HashMap<String, DefiLlamaCoin>,
}

#[derive(Deserialize)]
struct DefiLlamaCoin {
    price: f64,
}

#[derive(Deserialize)]
struct CoinGeckoEntry {
    usd: f64,
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct FakeSource {
        prices: Mutex<Result<Vec<TokenPrice>, String>>,
    }

    impl FakeSource {
        fn ok(prices: Vec<TokenPrice>) -> Self {
            Self {
                prices: Mutex::new(Ok(prices)),
            }
        }

        fn err(msg: &str) -> Self {
            Self {
                prices: Mutex::new(Err(msg.to_string())),
            }
        }
    }

    impl PriceSource for FakeSource {
        async fn get_prices(
            &self,
            coingecko_ids: &[&str],
        ) -> Result<Vec<TokenPrice>, PricingError> {
            match &*self.prices.lock().unwrap() {
                Ok(prices) => {
                    let result: Result<Vec<_>, _> = coingecko_ids
                        .iter()
                        .map(|id| {
                            prices
                                .iter()
                                .find(|p| p.coingecko_id == *id)
                                .cloned()
                                .ok_or_else(|| PricingError::NotFound((*id).to_string()))
                        })
                        .collect();
                    result
                }
                Err(msg) => Err(PricingError::NotFound(msg.clone())),
            }
        }
    }

    fn eth_price(usd: f64) -> TokenPrice {
        TokenPrice {
            coingecko_id: "ethereum".to_string(),
            usd_price: usd,
        }
    }

    fn usdc_price(usd: f64) -> TokenPrice {
        TokenPrice {
            coingecko_id: "usd-coin".to_string(),
            usd_price: usd,
        }
    }

    #[tokio::test]
    async fn returns_prices_from_primary() {
        let client = PriceClient::with_sources(
            FakeSource::ok(vec![eth_price(2000.0), usdc_price(1.0)]),
            FakeSource::err("should not be called"),
        );

        let prices = client.get_prices(&["ethereum", "usd-coin"]).await.unwrap();

        assert_eq!(prices.len(), 2);
        assert_eq!(prices[0], eth_price(2000.0));
        assert_eq!(prices[1], usdc_price(1.0));
    }

    #[tokio::test]
    async fn falls_back_to_secondary_on_primary_failure() {
        let client = PriceClient::with_sources(
            FakeSource::err("primary down"),
            FakeSource::ok(vec![eth_price(1950.0)]),
        );

        let prices = client.get_prices(&["ethereum"]).await.unwrap();

        assert_eq!(prices.len(), 1);
        assert_eq!(prices[0], eth_price(1950.0));
    }

    #[tokio::test]
    async fn returns_error_when_both_sources_fail() {
        let client = PriceClient::with_sources(
            FakeSource::err("primary down"),
            FakeSource::err("secondary down"),
        );

        let result = client.get_prices(&["ethereum"]).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("secondary down"));
    }

    #[tokio::test]
    async fn returns_error_when_token_not_in_source() {
        let client = PriceClient::with_sources(
            FakeSource::ok(vec![eth_price(2000.0)]),
            FakeSource::ok(vec![eth_price(2000.0)]),
        );

        let result = client.get_prices(&["nonexistent"]).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent"));
    }

    #[tokio::test]
    async fn empty_ids_returns_empty_vec() {
        let client = PriceClient::with_sources(
            FakeSource::err("should not be called"),
            FakeSource::err("should not be called"),
        );

        let prices = client.get_prices(&[]).await.unwrap();

        assert!(prices.is_empty());
    }

    #[tokio::test]
    #[ignore = "hits real DefiLlama API"]
    async fn defillama_live() {
        let source = DefiLlama::new(Client::new());
        let prices = source.get_prices(&["ethereum", "usd-coin"]).await.unwrap();

        assert_eq!(prices.len(), 2);
        assert_eq!(prices[0].coingecko_id, "ethereum");
        assert!(prices[0].usd_price > 0.0);
        assert_eq!(prices[1].coingecko_id, "usd-coin");
        assert!(prices[1].usd_price > 0.0);
    }

    #[tokio::test]
    #[ignore = "hits real CoinGecko API"]
    async fn coingecko_live() {
        let source = CoinGecko::new(Client::new());
        let prices = source.get_prices(&["ethereum", "usd-coin"]).await.unwrap();

        assert_eq!(prices.len(), 2);
        assert_eq!(prices[0].coingecko_id, "ethereum");
        assert!(prices[0].usd_price > 0.0);
        assert_eq!(prices[1].coingecko_id, "usd-coin");
        assert!(prices[1].usd_price > 0.0);
    }

    #[tokio::test]
    async fn primary_missing_token_falls_back_to_secondary() {
        let client = PriceClient::with_sources(
            FakeSource::ok(vec![eth_price(2000.0)]),
            FakeSource::ok(vec![eth_price(2000.0), usdc_price(1.0)]),
        );

        // primary has eth but not usdc, so it errors, fallback has both
        let prices = client.get_prices(&["ethereum", "usd-coin"]).await.unwrap();

        assert_eq!(prices.len(), 2);
        assert_eq!(prices[1], usdc_price(1.0));
    }
}
