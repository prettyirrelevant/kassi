use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use vaultrs::client::VaultClient;

use crate::SignerError;

pub struct VaultTransit {
    client: VaultClient,
    mount: String,
}

impl VaultTransit {
    pub fn new(client: VaultClient, mount: impl Into<String>) -> Self {
        Self {
            client,
            mount: mount.into(),
        }
    }

    #[must_use]
    pub fn key_name(merchant_id: &str) -> String {
        format!("kassi/merchant/{merchant_id}")
    }

    /// Create a new transit encryption key in Vault.
    ///
    /// # Errors
    /// Returns `SignerError::Vault` if the Vault API call fails.
    pub async fn create_key(&self, name: &str) -> Result<(), SignerError> {
        Ok(vaultrs::transit::key::create(&self.client, &self.mount, name, None).await?)
    }

    /// Encrypt plaintext bytes using the named transit key.
    ///
    /// # Errors
    /// Returns `SignerError::Vault` if the Vault API call fails.
    pub async fn encrypt(&self, name: &str, plaintext: &[u8]) -> Result<String, SignerError> {
        let b64 = BASE64.encode(plaintext);
        let resp = vaultrs::transit::data::encrypt(
            &self.client,
            &self.mount,
            name,
            &b64,
            None,
        )
        .await?;
        Ok(resp.ciphertext)
    }

    /// Decrypt ciphertext using the named transit key, returning raw bytes.
    ///
    /// # Errors
    /// Returns `SignerError::Vault` if the Vault API call fails,
    /// or `SignerError::Base64` if the decrypted payload is not valid base64.
    pub async fn decrypt(&self, name: &str, ciphertext: &str) -> Result<Vec<u8>, SignerError> {
        let resp = vaultrs::transit::data::decrypt(
            &self.client,
            &self.mount,
            name,
            ciphertext,
            None,
        )
        .await?;
        Ok(BASE64.decode(&resp.plaintext)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vaultrs::client::VaultClientSettingsBuilder;

    fn dev_client() -> VaultClient {
        VaultClient::new(
            VaultClientSettingsBuilder::default()
                .address("http://127.0.0.1:8200")
                .token("test-token")
                .build()
                .unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    #[ignore = "requires vault dev server"]
    async fn encrypt_decrypt_round_trip() {
        let client = dev_client();
        let vault = VaultTransit::new(client, "transit");
        let key_name = "test-round-trip";

        vault.create_key(key_name).await.unwrap();

        let plaintext = b"super secret merchant seed data!";
        let ciphertext = vault.encrypt(key_name, plaintext).await.unwrap();
        assert!(ciphertext.starts_with("vault:v1:"));

        let decrypted = vault.decrypt(key_name, &ciphertext).await.unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[tokio::test]
    #[ignore = "requires vault dev server"]
    async fn decrypt_with_wrong_key_fails() {
        let client = dev_client();
        let vault = VaultTransit::new(client, "transit");

        vault.create_key("key-a").await.unwrap();
        vault.create_key("key-b").await.unwrap();

        let ciphertext = vault.encrypt("key-a", b"secret").await.unwrap();
        let result = vault.decrypt("key-b", &ciphertext).await;
        assert!(result.is_err());
    }
}
