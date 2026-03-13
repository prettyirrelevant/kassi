use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{Kms, SignerError};

/// Buffer before actual expiry to avoid using a token that's about to die.
const EXPIRY_BUFFER: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum KmsError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("{operation} failed: {body}")]
    Api {
        operation: &'static str,
        body: String,
    },
}

struct TokenState {
    access_token: SecretString,
    expires_at: Instant,
}

const BASE_URL: &str = "https://app.infisical.com";

pub struct InfisicalKms {
    client: Client,
    client_id: SecretString,
    client_secret: SecretString,
    project_id: String,
    token: RwLock<TokenState>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginRequest<'a> {
    client_id: &'a str,
    client_secret: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct KeyResponse {
    key: KeyData,
}

#[derive(Deserialize)]
struct KeyData {
    id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateKeyRequest<'a> {
    project_id: &'a str,
    name: &'a str,
    encryption_algorithm: &'a str,
}

#[derive(Serialize)]
struct EncryptRequest<'a> {
    plaintext: &'a str,
}

#[derive(Deserialize)]
struct EncryptResponse {
    ciphertext: String,
}

#[derive(Serialize)]
struct DecryptRequest<'a> {
    ciphertext: &'a str,
}

#[derive(Deserialize)]
struct DecryptResponse {
    plaintext: String,
}

impl InfisicalKms {
    /// Authenticate with Infisical using universal auth (client ID + secret)
    /// and return an authenticated `InfisicalKms` instance.
    ///
    /// # Panics
    /// Panics if the HTTP client cannot be built (TLS backend unavailable).
    ///
    /// # Errors
    /// Returns `SignerError::Kms` if authentication fails.
    pub async fn login(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        project_id: impl Into<String>,
    ) -> Result<Self, SignerError> {
        let client_id = SecretString::from(client_id.into());
        let client_secret = SecretString::from(client_secret.into());

        let client = Client::builder()
            .user_agent(format!(
                "kassi/{} ({})",
                env!("CARGO_PKG_VERSION"),
                option_env!("KASSI_GIT_SHA").unwrap_or("dev"),
            ))
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(10)
            .build()
            .expect("failed to build HTTP client");

        let token_state = authenticate(
            &client,
            client_id.expose_secret(),
            client_secret.expose_secret(),
        )
        .await?;

        Ok(Self {
            client,
            client_id,
            client_secret,
            project_id: project_id.into(),
            token: RwLock::new(token_state),
        })
    }

    async fn get_key_id(&self, name: &str) -> Result<String, KmsError> {
        let token = self.valid_token().await?;

        let resp = self
            .client
            .get(format!("{BASE_URL}/api/v1/kms/keys/key-name/{name}"))
            .bearer_auth(token.expose_secret())
            .query(&[("projectId", &self.project_id)])
            .send()
            .await
            .map_err(KmsError::from)?;

        let resp = check_response(resp, "get key by name").await?;
        Ok(resp
            .json::<KeyResponse>()
            .await
            .map_err(KmsError::from)?
            .key
            .id)
    }

    /// Return a valid access token, refreshing if expired or about to expire.
    async fn valid_token(&self) -> Result<SecretString, KmsError> {
        {
            let state = self.token.read().await;
            if Instant::now() < state.expires_at {
                return Ok(state.access_token.clone());
            }
        }

        let mut state = self.token.write().await;
        // double-check: another task may have refreshed while we waited for the write lock
        if Instant::now() < state.expires_at {
            return Ok(state.access_token.clone());
        }

        tracing::info!("refreshing infisical access token");
        *state = authenticate(
            &self.client,
            self.client_id.expose_secret(),
            self.client_secret.expose_secret(),
        )
        .await?;

        Ok(state.access_token.clone())
    }
}

impl Kms for InfisicalKms {
    async fn create_key(&self, name: &str) -> Result<(), KmsError> {
        let token = self.valid_token().await?;

        let resp = self
            .client
            .post(format!("{BASE_URL}/api/v1/kms/keys"))
            .bearer_auth(token.expose_secret())
            .json(&CreateKeyRequest {
                project_id: &self.project_id,
                name,
                encryption_algorithm: "aes-256-gcm",
            })
            .send()
            .await
            .map_err(KmsError::from)?;

        check_response(resp, "create key").await?;
        Ok(())
    }

    async fn encrypt(&self, name: &str, plaintext: &[u8]) -> Result<String, KmsError> {
        let key_id = self.get_key_id(name).await?;
        let token = self.valid_token().await?;
        let b64 = BASE64.encode(plaintext);

        let resp = self
            .client
            .post(format!("{BASE_URL}/api/v1/kms/keys/{key_id}/encrypt"))
            .bearer_auth(token.expose_secret())
            .json(&EncryptRequest { plaintext: &b64 })
            .send()
            .await
            .map_err(KmsError::from)?;

        let resp = check_response(resp, "encrypt").await?;
        Ok(resp
            .json::<EncryptResponse>()
            .await
            .map_err(KmsError::from)?
            .ciphertext)
    }

    async fn decrypt(&self, name: &str, ciphertext: &str) -> Result<Vec<u8>, KmsError> {
        let key_id = self.get_key_id(name).await?;
        let token = self.valid_token().await?;

        let resp = self
            .client
            .post(format!("{BASE_URL}/api/v1/kms/keys/{key_id}/decrypt"))
            .bearer_auth(token.expose_secret())
            .json(&DecryptRequest { ciphertext })
            .send()
            .await
            .map_err(KmsError::from)?;

        let resp = check_response(resp, "decrypt").await?;
        let plaintext_b64 = resp
            .json::<DecryptResponse>()
            .await
            .map_err(KmsError::from)?
            .plaintext;

        BASE64.decode(&plaintext_b64).map_err(|e| KmsError::Api {
            operation: "decrypt (base64 decode)",
            body: e.to_string(),
        })
    }
}

async fn authenticate(
    client: &Client,
    client_id: &str,
    client_secret: &str,
) -> Result<TokenState, KmsError> {
    let resp = client
        .post(format!("{BASE_URL}/api/v1/auth/universal-auth/login"))
        .json(&LoginRequest {
            client_id,
            client_secret,
        })
        .send()
        .await?;

    let resp = check_response(resp, "login").await?;
    let login = resp.json::<LoginResponse>().await?;

    Ok(TokenState {
        access_token: SecretString::from(login.access_token),
        expires_at: (Instant::now() + Duration::from_secs(login.expires_in))
            .checked_sub(EXPIRY_BUFFER)
            .unwrap_or_else(Instant::now),
    })
}

async fn check_response(
    resp: reqwest::Response,
    operation: &'static str,
) -> Result<reqwest::Response, KmsError> {
    if !resp.status().is_success() {
        return Err(KmsError::Api {
            operation,
            body: resp.text().await.unwrap_or_default(),
        });
    }
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn dev_client() -> InfisicalKms {
        InfisicalKms::login("test-client-id", "test-client-secret", "test-project-id")
            .await
            .unwrap()
    }

    #[tokio::test]
    #[ignore = "requires infisical dev server"]
    async fn encrypt_decrypt_round_trip() {
        let kms = dev_client().await;
        let key_name = "test-round-trip";

        kms.create_key(key_name).await.unwrap();

        let plaintext = b"super secret merchant seed data!";
        let ciphertext = kms.encrypt(key_name, plaintext).await.unwrap();

        let decrypted = kms.decrypt(key_name, &ciphertext).await.unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[tokio::test]
    #[ignore = "requires infisical dev server"]
    async fn decrypt_with_wrong_key_fails() {
        let kms = dev_client().await;

        kms.create_key("key-a").await.unwrap();
        kms.create_key("key-b").await.unwrap();

        let ciphertext = kms.encrypt("key-a", b"secret").await.unwrap();
        let result = kms.decrypt("key-b", &ciphertext).await;
        assert!(result.is_err());
    }
}
