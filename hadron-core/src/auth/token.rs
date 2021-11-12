use anyhow::{bail, ensure, Result};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use tonic::metadata::AsciiMetadataValue;
use uuid::Uuid;

use crate::error::AppError;

/// The authorization header bearer prefix — for token creds.
const BEARER_PREFIX: &str = "bearer ";
/// The key/value key used for storing a generated JWT in a K8s Secret.
pub const SECRET_KEY_TOKEN: &str = "token";
/// The key/value key used to store an HMAC key used to generate a JWT.
pub const SECRET_HMAC_KEY: &str = "hmac_key";

/// An unverified token credenitals set, containing the ID of the token and other associated data.
#[derive(Clone)]
pub struct UnverifiedTokenCredentials {
    /// The ID of the token presented.
    pub claims: TokenClaims,
    /// The original token header value of these credentials.
    pub header: AsciiMetadataValue,
    /// The raw string form of the token extracted from the given header.
    pub token: String,
}

impl UnverifiedTokenCredentials {
    /// Extract a token from the given header value bytes.
    pub fn from_auth_header(header: AsciiMetadataValue) -> Result<Self> {
        let header_str = header.to_str().map_err(|_| AppError::InvalidCredentials("must be a valid string value".into()))?;

        // Split the header on the bearer prefix & ensure the leading segment is empty.
        let mut splits = header_str.splitn(2, BEARER_PREFIX);
        ensure!(splits.next() == Some(""), AppError::InvalidCredentials("authorization header value must begin with 'bearer '".into()));

        // Check the final segment and ensure we have a populated value.
        let token = match splits.next() {
            Some(token) if !token.is_empty() => token.to_string(),
            _ => bail!(AppError::InvalidCredentials("no token detected in header".into())),
        };
        let claims = TokenClaims::decode_unverified(&token).map_err(|err| AppError::InvalidCredentials(err.to_string()))?;
        Ok(Self { claims, header, token })
    }

    /// Verify the integrity of these credentials using the given key.
    pub fn verify(self, key: &DecodingKey) -> Result<TokenCredentials> {
        let claims = TokenClaims::decode(&self.token, key).map_err(|err| AppError::InvalidCredentials(err.to_string()))?;
        Ok(TokenCredentials {
            claims,
            header: self.header,
            token: self.token,
        })
    }
}

/// A token credenitals set, containing the ID of the token and other associated data.
///
/// This is construct by cryptographically verifying a token, and validating its claims.
#[derive(Clone)]
pub struct TokenCredentials {
    /// The ID of the token presented.
    pub claims: TokenClaims,
    /// The original token header value of these credentials.
    pub header: AsciiMetadataValue,
    /// The raw string form of the token extracted from the given header.
    pub token: String,
}

/// The model of a JWT created by a Hadron cluster.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    /// The JWT ID of this token, always a UUID unique to the token's original creation.
    pub jti: String,
    /// The name of the Token CR which this token corresponds to.
    pub sub: String,
}

impl TokenClaims {
    /// Create a new instance.
    pub fn new(cr_name: &str) -> Self {
        Self {
            jti: Uuid::new_v4().to_string(),
            sub: cr_name.into(),
        }
    }

    /// Encode this claims body as a JWT.
    pub fn encode(&self, key: &EncodingKey) -> jsonwebtoken::errors::Result<String> {
        let header = Header::new(Algorithm::HS512);
        jsonwebtoken::encode(&header, &self, key)
    }

    /// Decode the given string as a JWT with a `TokenClaims` body.
    pub fn decode(token: impl AsRef<str>, key: &DecodingKey) -> jsonwebtoken::errors::Result<Self> {
        let validation = Validation {
            algorithms: vec![Algorithm::HS512],
            validate_exp: false,
            validate_nbf: false,
            ..Default::default()
        };
        jsonwebtoken::decode(token.as_ref(), key, &validation).map(|body| body.claims)
    }

    /// Decode the given string as a JWT with a `TokenClaims` body.
    ///
    /// This does not verify the integrity of the token, and is intended only to extract the
    /// ID & sub of the token for later verification.
    pub fn decode_unverified(token: impl AsRef<str>) -> jsonwebtoken::errors::Result<Self> {
        jsonwebtoken::dangerous_insecure_decode(token.as_ref()).map(|body| body.claims)
    }
}
