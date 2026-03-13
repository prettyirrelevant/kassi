use std::collections::HashSet;
use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

use crate::infisical::KmsError;
use crate::Kms;

/// In-memory KMS for tests. Keys are stored in a `HashSet`; "encryption"
/// is a simple base64 round-trip (prefixed with the key name so decrypting
/// with the wrong key fails).
pub struct MockKms {
    keys: Mutex<HashSet<String>>,
}

impl MockKms {
    #[must_use]
    pub fn new() -> Self {
        Self {
            keys: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for MockKms {
    fn default() -> Self {
        Self::new()
    }
}

impl Kms for MockKms {
    async fn create_key(&self, name: &str) -> Result<(), KmsError> {
        self.keys.lock().unwrap().insert(name.to_string());
        Ok(())
    }

    async fn encrypt(&self, name: &str, plaintext: &[u8]) -> Result<String, KmsError> {
        if !self.keys.lock().unwrap().contains(name) {
            return Err(KmsError::Api {
                operation: "encrypt",
                body: format!("key not found: {name}"),
            });
        }
        // prefix with "name:" so wrong-key decrypt is detected
        let mut buf = name.as_bytes().to_vec();
        buf.push(b':');
        buf.extend_from_slice(plaintext);
        Ok(BASE64.encode(&buf))
    }

    async fn decrypt(&self, name: &str, ciphertext: &str) -> Result<Vec<u8>, KmsError> {
        if !self.keys.lock().unwrap().contains(name) {
            return Err(KmsError::Api {
                operation: "decrypt",
                body: format!("key not found: {name}"),
            });
        }
        let raw = BASE64.decode(ciphertext).map_err(|e| KmsError::Api {
            operation: "decrypt (base64)",
            body: e.to_string(),
        })?;
        let prefix = format!("{name}:");
        let Some(plaintext) = raw.strip_prefix(prefix.as_bytes()) else {
            return Err(KmsError::Api {
                operation: "decrypt",
                body: "wrong key".into(),
            });
        };
        Ok(plaintext.to_vec())
    }
}
