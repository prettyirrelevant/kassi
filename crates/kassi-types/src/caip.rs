use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum CaipError {
    #[error("invalid CAIP-2 identifier: {0}")]
    InvalidCaip2(String),

    #[error("invalid CAIP-10 identifier: {0}")]
    InvalidCaip10(String),

    #[error("invalid CAIP-19 identifier: {0}")]
    InvalidCaip19(String),
}

/// CAIP-2 chain identifier: `namespace:reference`
/// e.g. `eip155:1`, `solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Caip2 {
    raw: String,
    colon: usize,
}

impl Caip2 {
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.raw[..self.colon]
    }

    #[must_use]
    pub fn reference(&self) -> &str {
        &self.raw[self.colon + 1..]
    }
}

impl FromStr for Caip2 {
    type Err = CaipError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let colon = s
            .find(':')
            .ok_or_else(|| CaipError::InvalidCaip2(s.to_string()))?;

        if colon == 0 || colon == s.len() - 1 || s[colon + 1..].contains(':') {
            return Err(CaipError::InvalidCaip2(s.to_string()));
        }

        Ok(Self {
            raw: s.to_string(),
            colon,
        })
    }
}

impl TryFrom<String> for Caip2 {
    type Error = CaipError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Caip2> for String {
    fn from(c: Caip2) -> Self {
        c.raw
    }
}

impl fmt::Display for Caip2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// CAIP-10 account identifier: `namespace:reference:address`
/// e.g. `eip155:1:0xab16a96D359eC26a11e2C2b3d8f8B8942d5Bfcdb`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Caip10 {
    chain_id: Caip2,
    address: String,
}

impl Caip10 {
    #[must_use]
    pub fn chain_id(&self) -> &Caip2 {
        &self.chain_id
    }

    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        self.chain_id.namespace()
    }
}

impl FromStr for Caip10 {
    type Err = CaipError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let make_err = || CaipError::InvalidCaip10(s.to_string());

        let first_colon = s.find(':').ok_or_else(make_err)?;
        let rest = &s[first_colon + 1..];
        let second_colon = rest.find(':').ok_or_else(make_err)?;

        let chain_part = &s[..first_colon + 1 + second_colon];
        let address = &rest[second_colon + 1..];

        if address.is_empty() {
            return Err(make_err());
        }

        let chain_id: Caip2 = chain_part.parse().map_err(|_| make_err())?;

        Ok(Self {
            chain_id,
            address: address.to_string(),
        })
    }
}

impl TryFrom<String> for Caip10 {
    type Error = CaipError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Caip10> for String {
    fn from(c: Caip10) -> Self {
        c.to_string()
    }
}

impl fmt::Display for Caip10 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.chain_id, self.address)
    }
}

/// CAIP-19 asset identifier: `namespace:reference/asset_namespace:asset_reference`
/// e.g. `eip155:1/erc20:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Caip19 {
    chain_id: Caip2,
    asset_namespace: String,
    asset_reference: String,
}

impl Caip19 {
    #[must_use]
    pub fn chain_id(&self) -> &Caip2 {
        &self.chain_id
    }

    #[must_use]
    pub fn asset_namespace(&self) -> &str {
        &self.asset_namespace
    }

    #[must_use]
    pub fn asset_reference(&self) -> &str {
        &self.asset_reference
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        self.chain_id.namespace()
    }
}

impl FromStr for Caip19 {
    type Err = CaipError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // format: namespace:reference/asset_namespace:asset_reference
        let (chain_part, asset_part) = s
            .split_once('/')
            .ok_or_else(|| CaipError::InvalidCaip19(s.to_string()))?;

        let chain_id: Caip2 = chain_part
            .parse()
            .map_err(|_| CaipError::InvalidCaip19(s.to_string()))?;

        let (asset_namespace, asset_reference) = asset_part
            .split_once(':')
            .ok_or_else(|| CaipError::InvalidCaip19(s.to_string()))?;

        if asset_namespace.is_empty() || asset_reference.is_empty() {
            return Err(CaipError::InvalidCaip19(s.to_string()));
        }

        Ok(Self {
            chain_id,
            asset_namespace: asset_namespace.to_string(),
            asset_reference: asset_reference.to_string(),
        })
    }
}

impl TryFrom<String> for Caip19 {
    type Error = CaipError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Caip19> for String {
    fn from(c: Caip19) -> Self {
        c.to_string()
    }
}

impl fmt::Display for Caip19 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}:{}",
            self.chain_id, self.asset_namespace, self.asset_reference
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- CAIP-2 tests --

    #[test]
    fn caip2_valid_evm() {
        let id: Caip2 = "eip155:1".parse().unwrap();
        assert_eq!(id.namespace(), "eip155");
        assert_eq!(id.reference(), "1");
    }

    #[test]
    fn caip2_valid_solana() {
        let id: Caip2 = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp".parse().unwrap();
        assert_eq!(id.namespace(), "solana");
        assert_eq!(id.reference(), "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
    }

    #[test]
    fn caip2_invalid_no_colon() {
        assert!("eip155".parse::<Caip2>().is_err());
    }

    #[test]
    fn caip2_rejects_empty_namespace() {
        assert!(":1".parse::<Caip2>().is_err());
    }

    #[test]
    fn caip2_rejects_empty_reference() {
        assert!("eip155:".parse::<Caip2>().is_err());
    }

    #[test]
    fn caip2_rejects_both_empty() {
        assert!(":".parse::<Caip2>().is_err());
    }

    #[test]
    fn caip2_roundtrip() {
        let original = "eip155:137";
        let parsed: Caip2 = original.parse().unwrap();
        let displayed = parsed.to_string();
        let reparsed: Caip2 = displayed.parse().unwrap();
        assert_eq!(parsed, reparsed);
        assert_eq!(original, displayed);
    }

    #[test]
    fn caip2_serde_roundtrip() {
        let id: Caip2 = "eip155:1".parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"eip155:1\"");
        let deserialized: Caip2 = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    // -- CAIP-10 tests --

    #[test]
    fn caip10_valid_evm() {
        let id: Caip10 = "eip155:1:0xab16a96D359eC26a11e2C2b3d8f8B8942d5Bfcdb"
            .parse()
            .unwrap();
        assert_eq!(id.namespace(), "eip155");
        assert_eq!(id.chain_id().reference(), "1");
        assert_eq!(id.address(), "0xab16a96D359eC26a11e2C2b3d8f8B8942d5Bfcdb");
    }

    #[test]
    fn caip10_valid_solana() {
        let id: Caip10 =
            "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp:7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv"
                .parse()
                .unwrap();
        assert_eq!(id.namespace(), "solana");
        assert_eq!(id.address(), "7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv");
    }

    #[test]
    fn caip10_invalid_missing_address() {
        assert!("eip155:1".parse::<Caip10>().is_err());
    }

    #[test]
    fn caip10_rejects_empty_address() {
        assert!("eip155:1:".parse::<Caip10>().is_err());
    }

    #[test]
    fn caip10_rejects_empty_reference() {
        assert!("eip155::0xabc".parse::<Caip10>().is_err());
    }

    #[test]
    fn caip10_rejects_empty_namespace() {
        assert!(":1:0xabc".parse::<Caip10>().is_err());
    }

    #[test]
    fn caip10_roundtrip() {
        let original = "eip155:1:0xab16a96D359eC26a11e2C2b3d8f8B8942d5Bfcdb";
        let parsed: Caip10 = original.parse().unwrap();
        let displayed = parsed.to_string();
        let reparsed: Caip10 = displayed.parse().unwrap();
        assert_eq!(parsed, reparsed);
        assert_eq!(original, displayed);
    }

    #[test]
    fn caip10_serde_roundtrip() {
        let id: Caip10 = "eip155:1:0xabc".parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"eip155:1:0xabc\"");
        let deserialized: Caip10 = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    // -- CAIP-19 tests --

    #[test]
    fn caip19_valid_erc20() {
        let id: Caip19 = "eip155:1/erc20:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
            .parse()
            .unwrap();
        assert_eq!(id.namespace(), "eip155");
        assert_eq!(id.chain_id().reference(), "1");
        assert_eq!(id.asset_namespace(), "erc20");
        assert_eq!(
            id.asset_reference(),
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
        );
    }

    #[test]
    fn caip19_valid_native() {
        let id: Caip19 = "eip155:1/slip44:60".parse().unwrap();
        assert_eq!(id.asset_namespace(), "slip44");
        assert_eq!(id.asset_reference(), "60");
    }

    #[test]
    fn caip19_invalid_no_slash() {
        assert!("eip155:1".parse::<Caip19>().is_err());
    }

    #[test]
    fn caip19_invalid_no_asset_colon() {
        assert!("eip155:1/erc20".parse::<Caip19>().is_err());
    }

    #[test]
    fn caip19_rejects_empty_asset_namespace() {
        assert!("eip155:1/:0xabc".parse::<Caip19>().is_err());
    }

    #[test]
    fn caip19_rejects_empty_asset_reference() {
        assert!("eip155:1/erc20:".parse::<Caip19>().is_err());
    }

    #[test]
    fn caip19_roundtrip() {
        let original = "eip155:1/erc20:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
        let parsed: Caip19 = original.parse().unwrap();
        let displayed = parsed.to_string();
        let reparsed: Caip19 = displayed.parse().unwrap();
        assert_eq!(parsed, reparsed);
        assert_eq!(original, displayed);
    }

    #[test]
    fn caip19_serde_roundtrip() {
        let id: Caip19 = "eip155:1/erc20:0xabc".parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"eip155:1/erc20:0xabc\"");
        let deserialized: Caip19 = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    // -- namespace extraction --

    #[test]
    fn caip2_namespace_returns_eip155() {
        let id: Caip2 = "eip155:1".parse().unwrap();
        assert_eq!(id.namespace(), "eip155");
    }

    #[test]
    fn caip2_namespace_returns_solana() {
        let id: Caip2 = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp".parse().unwrap();
        assert_eq!(id.namespace(), "solana");
    }

    #[test]
    fn caip10_namespace_delegates_to_chain_id() {
        let id: Caip10 = "eip155:1:0xabc".parse().unwrap();
        assert_eq!(id.namespace(), "eip155");
    }

    #[test]
    fn caip19_namespace_delegates_to_chain_id() {
        let id: Caip19 = "eip155:1/erc20:0xabc".parse().unwrap();
        assert_eq!(id.namespace(), "eip155");
    }
}
