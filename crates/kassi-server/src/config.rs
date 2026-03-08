use serde::Deserialize;

fn default_port() -> u16 {
    3000
}

#[derive(Deserialize)]
pub struct Config {
    pub database_url: String,
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
