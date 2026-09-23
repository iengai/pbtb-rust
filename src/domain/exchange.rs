use crate::domain::error::DomainError;
use k256::SecretKey;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

/// The exchange a bot trades on. Fixed for the bot's life: its credentials,
/// its config's coin list and its quote coin all belong to one exchange.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum Exchange {
    #[default]
    Bybit,
    Hyperliquid,
}

impl Exchange {
    pub const ALL: [Exchange; 2] = [Exchange::Bybit, Exchange::Hyperliquid];

    // Deliberate inherent parser: returns Option, so it is not std::str::FromStr (which returns Result).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "bybit" => Some(Exchange::Bybit),
            "hyperliquid" => Some(Exchange::Hyperliquid),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Exchange::Bybit => "bybit",
            Exchange::Hyperliquid => "hyperliquid",
        }
    }

    /// How the exchange is named to a person.
    pub fn label(&self) -> &'static str {
        match self {
            Exchange::Bybit => "Bybit",
            Exchange::Hyperliquid => "Hyperliquid",
        }
    }

    /// The coin a bot's balance, PnL and capital are counted in.
    pub fn quote(&self) -> &'static str {
        match self {
            Exchange::Bybit => "USDT",
            Exchange::Hyperliquid => "USDC",
        }
    }

    /// The site a bot's public link may point at: the host and every
    /// subdomain of it.
    pub fn public_host(&self) -> &'static str {
        match self {
            Exchange::Bybit => "bybit.com",
            Exchange::Hyperliquid => "hyperliquid.xyz",
        }
    }

    /// Check and normalise the two credential strings a bot is added with.
    /// Returns them in the form they are stored. What the two strings are
    /// depends on the exchange:
    ///
    /// - Bybit: the API key and its secret.
    /// - Hyperliquid: the address of the account that holds the funds, and the
    ///   private key of an API wallet approved for it. An API wallet can trade
    ///   but cannot withdraw; the account's own key can do both, so a key whose
    ///   address is the account's is refused. Hyperliquid keys carry no IP
    ///   whitelist, which makes that distinction the only thing standing
    ///   between a leaked key and the funds.
    ///
    /// No message carries the secret, only what shape was expected.
    pub fn validate_credentials(
        &self,
        api_key: &str,
        secret_key: &str,
    ) -> Result<(String, String), DomainError> {
        let (api_key, secret_key) = (api_key.trim(), secret_key.trim());
        if api_key.is_empty() || secret_key.is_empty() {
            return Err(DomainError::InvalidCredentials(
                "both credentials are required".into(),
            ));
        }
        match self {
            Exchange::Bybit => Ok((api_key.to_string(), secret_key.to_string())),
            Exchange::Hyperliquid => {
                let address = hex_of_len(api_key, 20).ok_or_else(|| {
                    DomainError::InvalidCredentials(
                        "a Hyperliquid account address is 0x followed by 40 hex characters".into(),
                    )
                })?;
                let key = hex_of_len(secret_key, 32).ok_or_else(|| {
                    DomainError::InvalidCredentials(
                        "a Hyperliquid API wallet private key is 0x followed by 64 hex characters"
                            .into(),
                    )
                })?;
                let signer = evm_address(&key).ok_or_else(|| {
                    DomainError::InvalidCredentials(
                        "the Hyperliquid private key is not a valid key".into(),
                    )
                })?;
                if signer == address {
                    return Err(DomainError::InvalidCredentials(
                        "that is the account's own private key, which can withdraw funds; \
                         create an API wallet on Hyperliquid and enter its key instead"
                            .into(),
                    ));
                }
                Ok((format!("0x{address}"), format!("0x{key}")))
            }
        }
    }

    /// The identity a Hyperliquid API wallet signs as, derived from its stored
    /// private key: two bots signing as one wallet share its nonce sequence
    /// and reject each other's orders. `None` on any other exchange, or on a
    /// key that does not parse.
    pub fn signer_of(&self, secret_key: &str) -> Option<String> {
        match self {
            Exchange::Bybit => None,
            Exchange::Hyperliquid => evm_address(&hex_of_len(secret_key, 32)?),
        }
    }
}

/// The exchange a template or a bot's config is for, read from its
/// `pbtb.exchange`. A config carries the coin list and the capital of one
/// exchange, so it is applied to, and launched on, a bot of that exchange
/// alone. A document with no mark predates the field and every one of those
/// was made for Bybit; a mark naming an exchange this service does not offer
/// is refused rather than guessed.
pub fn declared_exchange(config: &serde_json::Value) -> Result<Exchange, DomainError> {
    match config.get("pbtb").and_then(|m| m.get("exchange")) {
        None | Some(serde_json::Value::Null) => Ok(Exchange::Bybit),
        Some(v) => v.as_str().and_then(Exchange::from_str).ok_or_else(|| {
            DomainError::InvalidConfig(format!(
                "pbtb.exchange {v} is not an exchange this service trades on"
            ))
        }),
    }
}

/// Refuse a config made for another exchange than the bot's.
pub fn ensure_same_exchange(config_for: Exchange, bot: Exchange) -> Result<(), DomainError> {
    if config_for == bot {
        Ok(())
    } else {
        Err(DomainError::InvalidConfig(format!(
            "this config is for {}, and the bot trades on {}; pick a {} config",
            config_for.label(),
            bot.label(),
            bot.label()
        )))
    }
}

/// `s` as lowercase hex of exactly `bytes` bytes, with or without a `0x`
/// prefix; `None` on any other shape.
fn hex_of_len(s: &str, bytes: usize) -> Option<String> {
    let body = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    (body.len() == bytes * 2 && body.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| body.to_ascii_lowercase())
}

/// The EVM address of a secp256k1 private key given as 64 hex characters:
/// the last 20 bytes of the Keccak-256 of the uncompressed public key, as
/// lowercase hex without a prefix.
fn evm_address(key_hex: &str) -> Option<String> {
    let bytes = hex::decode(key_hex).ok()?;
    let key = SecretKey::from_slice(&bytes).ok()?;
    let point = key.public_key().to_encoded_point(false);
    let digest = Keccak256::digest(&point.as_bytes()[1..]);
    Some(hex::encode(&digest[12..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The first Hardhat/Anvil development account: a key published with the
    // tooling, so its address is known and it guards nothing.
    const DEV_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const DEV_ADDR: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
    const ACCOUNT: &str = "0x1111111111111111111111111111111111111111";

    #[test]
    fn round_trips_every_exchange() {
        for e in Exchange::ALL {
            assert_eq!(Exchange::from_str(e.as_str()), Some(e));
        }
        assert_eq!(
            Exchange::from_str(" HyperLiquid "),
            Some(Exchange::Hyperliquid)
        );
        assert_eq!(Exchange::from_str("binance"), None);
    }

    #[test]
    fn a_config_s_exchange_defaults_to_bybit_and_refuses_unknown_marks() {
        use serde_json::json;
        assert_eq!(declared_exchange(&json!({})).unwrap(), Exchange::Bybit);
        assert_eq!(
            declared_exchange(&json!({"pbtb": {"exchange": "hyperliquid"}})).unwrap(),
            Exchange::Hyperliquid
        );
        for bad in [json!("binance"), json!(1)] {
            let err = declared_exchange(&json!({"pbtb": {"exchange": bad}})).unwrap_err();
            assert!(matches!(err, DomainError::InvalidConfig(_)), "{err}");
        }
        assert!(ensure_same_exchange(Exchange::Bybit, Exchange::Bybit).is_ok());
        let err = ensure_same_exchange(Exchange::Bybit, Exchange::Hyperliquid).unwrap_err();
        assert!(
            err.to_string().contains("pick a Hyperliquid config"),
            "{err}"
        );
    }

    #[test]
    fn derives_the_address_of_a_known_key() {
        assert_eq!(
            Exchange::Hyperliquid.signer_of(DEV_KEY).as_deref(),
            Some(&DEV_ADDR[2..])
        );
        assert_eq!(Exchange::Bybit.signer_of(DEV_KEY), None);
    }

    #[test]
    fn bybit_credentials_are_taken_as_given_but_trimmed() {
        assert_eq!(
            Exchange::Bybit
                .validate_credentials(" ak ", "sk\n")
                .unwrap(),
            ("ak".to_string(), "sk".to_string())
        );
        assert!(matches!(
            Exchange::Bybit.validate_credentials("ak", "  "),
            Err(DomainError::InvalidCredentials(_))
        ));
    }

    #[test]
    fn hyperliquid_credentials_are_normalised() {
        let upper = format!("0X{}", DEV_KEY[2..].to_ascii_uppercase());
        let (addr, key) = Exchange::Hyperliquid
            .validate_credentials(&ACCOUNT[2..], &upper)
            .unwrap();
        assert_eq!(addr, ACCOUNT);
        assert_eq!(key, DEV_KEY);
    }

    #[test]
    fn hyperliquid_refuses_malformed_values_without_echoing_the_key() {
        for (addr, key) in [
            ("0x1234", DEV_KEY),
            (ACCOUNT, "0x1234"),
            (ACCOUNT, &format!("{}zz", &DEV_KEY[..64])),
            // Zero is not a valid secp256k1 scalar.
            (
                ACCOUNT,
                "0x0000000000000000000000000000000000000000000000000000000000000000",
            ),
        ] {
            let err = Exchange::Hyperliquid
                .validate_credentials(addr, key)
                .unwrap_err();
            assert!(matches!(err, DomainError::InvalidCredentials(_)), "{err}");
            assert!(!err.to_string().contains(&DEV_KEY[2..]), "{err}");
        }
    }

    #[test]
    fn hyperliquid_refuses_the_account_s_own_key() {
        let err = Exchange::Hyperliquid
            .validate_credentials(DEV_ADDR, DEV_KEY)
            .unwrap_err();
        assert!(err.to_string().contains("API wallet"), "{err}");
    }
}
