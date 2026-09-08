//! Tokens that are handed out, and the form they are stored in.

use sha2::{Digest, Sha256};

/// A URL-safe token with 256 bits of entropy from the OS.
///
/// Hex rather than something denser because these travel in URLs and get read
/// aloud out of error reports; a token that needs escaping is a token that will
/// eventually be mangled.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source");
    hex::encode(bytes)
}

/// What is stored in place of a token.
///
/// The rows sit in a table several roles can read, and a readable token is a
/// usable one — so what is kept is only enough to recognise the token when it
/// comes back.
pub fn token_digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_hex_and_long_enough_to_be_unguessable() {
        let token = random_token();
        assert_eq!(token.len(), 64, "32 bytes, hex encoded");
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(token, random_token(), "two calls are not the same token");
    }

    #[test]
    fn the_digest_does_not_carry_the_token() {
        let token = random_token();
        let digest = token_digest(&token);
        assert_eq!(digest.len(), 64);
        assert!(
            !digest.contains(&token[..8]),
            "a digest that echoed the token would defeat storing it instead"
        );
        assert_eq!(digest, token_digest(&token), "and it is stable");
    }
}
