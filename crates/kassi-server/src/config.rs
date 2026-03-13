use serde::Deserialize;

fn default_port() -> u16 {
    3000
}

#[derive(Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub session_jwt_secret: String,
    pub api_key_prefix: String,
    #[serde(default = "default_port")]
    pub port: u16,
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
