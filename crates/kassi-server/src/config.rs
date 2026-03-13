use serde::Deserialize;

fn default_port() -> u16 {
    3000
}

fn default_quote_lock_duration_secs() -> i64 {
    1800 // 30 minutes
}

fn default_price_cache_stale_secs() -> i64 {
    300 // 5 minutes
}

#[derive(Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub session_jwt_secret: String,
    pub api_key_prefix: String,
    pub infisical_client_id: String,
    pub infisical_client_secret: String,
    pub infisical_project_id: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_quote_lock_duration_secs")]
    pub quote_lock_duration_secs: i64,
    #[serde(default = "default_price_cache_stale_secs")]
    pub price_cache_stale_secs: i64,
    pub internal_basic_auth_token: String,
    pub admin_basic_auth_token: String,
}

impl Config {
    /// Loads configuration from environment variables.
    ///
    /// # Panics
    /// Panics if required environment variables are missing.
    #[must_use]
    pub fn from_env() -> Self {
        envy::from_env().expect("failed to load config from environment")
    }
}
