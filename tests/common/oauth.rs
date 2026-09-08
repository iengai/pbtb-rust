//! A stand-in authorization server.
//!
//! The keypair and key set under `tests/fixtures/` are test material with no
//! secret in them — they exist so a test can mint a token the real verifier
//! accepts, and so the ones it must reject can be minted the same way.

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const KID: &str = "test-key-1";
pub const SUBJECT: &str = "user_01JXXXXXXXXXXXXXXXXXXXXXX";

/// A symmetric key, and the `kid` a key set would name it under.
///
/// The point of publishing one is that a key set is public: the "public" key IS
/// the signing secret, so anyone who can read the document can mint tokens.
const SYMMETRIC_KID: &str = "symmetric-key";
const SYMMETRIC_SECRET: &[u8] = b"a-secret-an-issuer-must-never-publish";
const SYMMETRIC_JWK: &str = r#"{
  "kty": "oct",
  "alg": "HS256",
  "kid": "symmetric-key",
  "k": "YS1zZWNyZXQtYW4taXNzdWVyLW11c3QtbmV2ZXItcHVibGlzaA"
}"#;

const SIGNING_KEY: &str = include_str!("../fixtures/oauth_signing_key.pem");
const JWKS: &str = include_str!("../fixtures/oauth_jwks.json");

pub struct FakeIssuer {
    server: MockServer,
}

impl FakeIssuer {
    /// Serve OIDC discovery and a key set, the two documents a resource server
    /// needs before it can check a signature.
    pub async fn start() -> Self {
        Self::start_with(false).await
    }

    /// The same, with a symmetric key in the published set.
    pub async fn start_publishing_a_symmetric_key() -> Self {
        Self::start_with(true).await
    }

    async fn start_with(symmetric: bool) -> Self {
        let server = MockServer::start().await;
        let issuer = server.uri();
        let jwks = if symmetric {
            JWKS.replace("\"keys\": [", &format!("\"keys\": [\n    {SYMMETRIC_JWK},"))
        } else {
            JWKS.to_string()
        };

        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "issuer": issuer,
                "jwks_uri": format!("{issuer}/oauth2/jwks"),
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/oauth2/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(jwks, "application/json"))
            .mount(&server)
            .await;

        Self { server }
    }

    pub fn issuer(&self) -> String {
        self.server.uri()
    }

    /// A signed token. Everything a test wants to vary — who it is for, what it
    /// can do, when it expires, who signed it — is an argument, so a rejection
    /// test differs from an acceptance test by exactly the field under test.
    pub fn token(&self, claims: serde_json::Value) -> String {
        self.sign(KID, claims)
    }

    /// A token signed with the secret the key set published — the token a
    /// reader of that public document can mint for anybody.
    ///
    /// It verifies correctly against the published key, which is the whole
    /// problem: only refusing the algorithm outright stops it.
    pub fn forge_symmetric(&self, claims: serde_json::Value) -> String {
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(SYMMETRIC_KID.to_string());
        encode(
            &header,
            &claims,
            &EncodingKey::from_secret(SYMMETRIC_SECRET),
        )
        .expect("sign")
    }

    pub fn sign(&self, kid: &str, claims: serde_json::Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_string());
        encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(SIGNING_KEY.as_bytes()).expect("the fixture key parses"),
        )
        .expect("sign")
    }

    /// Claims that verify: this issuer, this resource, not yet expired.
    pub fn claims(&self, subject: &str, scope: &str, audience: &str) -> serde_json::Value {
        json!({
            "iss": self.issuer(),
            "aud": audience,
            "sub": subject,
            "scope": scope,
            "exp": now() + 3600,
            "iat": now(),
        })
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs() as i64
}
