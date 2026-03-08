use std::fmt;

use serde::{Deserialize, Serialize};

const BASE58_ALPHABET: &[char] = &[
    '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K',
    'L', 'M', 'N', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b', 'c', 'd', 'e',
    'f', 'g', 'h', 'i', 'j', 'k', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y',
    'z',
];

const NANOID_LENGTH: usize = 21;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityPrefix {
    Merchant,
    MerchantConfig,
    SettlementDestination,
    Signer,
    Asset,
    DepositAddress,
    NetworkAddress,
    PaymentIntent,
    Quote,
    LedgerEntry,
    WebhookDelivery,
    PriceCache,
}

impl EntityPrefix {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Merchant => "mer_",
            Self::MerchantConfig => "mcfg_",
            Self::SettlementDestination => "sdst_",
            Self::Signer => "sig_",
            Self::Asset => "ast_",
            Self::DepositAddress => "dep_",
            Self::NetworkAddress => "nadr_",
            Self::PaymentIntent => "pi_",
            Self::Quote => "quo_",
            Self::LedgerEntry => "le_",
            Self::WebhookDelivery => "whd_",
            Self::PriceCache => "prc_",
        }
    }
}

/// A prefixed entity identifier using nanoid with base58 alphabet.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(String);

impl EntityId {
    #[must_use]
    pub fn new(prefix: EntityPrefix) -> Self {
        let suffix = nanoid::nanoid!(NANOID_LENGTH, BASE58_ALPHABET);
        Self(format!("{}{}", prefix.as_str(), suffix))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Wrap an existing string as an `EntityId` without validation.
    #[must_use]
    pub fn from_raw(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for EntityId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const ALL_PREFIXES: &[EntityPrefix] = &[
        EntityPrefix::Merchant,
        EntityPrefix::MerchantConfig,
        EntityPrefix::SettlementDestination,
        EntityPrefix::Signer,
        EntityPrefix::Asset,
        EntityPrefix::DepositAddress,
        EntityPrefix::NetworkAddress,
        EntityPrefix::PaymentIntent,
        EntityPrefix::Quote,
        EntityPrefix::LedgerEntry,
        EntityPrefix::WebhookDelivery,
        EntityPrefix::PriceCache,
    ];

    #[test]
    fn each_prefix_produces_correctly_formatted_ids() {
        for prefix in ALL_PREFIXES {
            let id = EntityId::new(*prefix);
            let s = id.as_str();
            assert!(
                s.starts_with(prefix.as_str()),
                "{s} should start with {}",
                prefix.as_str()
            );
            let suffix = &s[prefix.as_str().len()..];
            assert_eq!(suffix.len(), NANOID_LENGTH);
        }
    }

    #[test]
    fn generated_ids_contain_only_base58_characters() {
        for prefix in ALL_PREFIXES {
            let id = EntityId::new(*prefix);
            let suffix = &id.as_str()[prefix.as_str().len()..];
            for ch in suffix.chars() {
                assert!(
                    BASE58_ALPHABET.contains(&ch),
                    "character '{ch}' is not in base58 alphabet"
                );
            }
        }
    }

    #[test]
    fn uniqueness_across_1000_ids() {
        let ids: HashSet<String> = (0..1000)
            .map(|_| EntityId::new(EntityPrefix::Merchant).to_string())
            .collect();
        assert_eq!(ids.len(), 1000);
    }

    #[test]
    fn serde_roundtrip() {
        let id = EntityId::new(EntityPrefix::Asset);
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: EntityId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn display_matches_as_str() {
        let id = EntityId::new(EntityPrefix::Quote);
        assert_eq!(id.to_string(), id.as_str());
    }

    #[test]
    fn from_raw_preserves_value() {
        let id = EntityId::from_raw("mer_abc123".to_string());
        assert_eq!(id.as_str(), "mer_abc123");
    }

    #[test]
    fn as_ref_returns_inner_str() {
        let id = EntityId::new(EntityPrefix::Merchant);
        let s: &str = id.as_ref();
        assert_eq!(s, id.as_str());
    }
}
