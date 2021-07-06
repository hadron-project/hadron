use anyhow::{bail, ensure, Result};
use jsonwebtoken::{Algorithm, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::AppError;

/// The authorization header bearer prefix — for token creds.
const BEARER_PREFIX: &str = "bearer ";

/// A token credenitals set, containing the ID of the token and other associated data.
///
/// This is construct by cryptographically verifying a token, and validating its claims.
#[derive(Clone)]
pub struct TokenCredentials {
    /// The ID of the token presented.
    pub claims: TokenClaims,
    /// The original token header value of these credentials.
    pub header: http::HeaderValue,
}

impl TokenCredentials {
    /// Extract a token from the given header value bytes.
    pub fn from_auth_header(header: http::HeaderValue, config: &Config) -> Result<Self> {
        let header_str = header
            .to_str()
            .map_err(|_| AppError::InvalidCredentials("must be a valid string value".into()))?;

        // Split the header on the bearer prefix & ensure the leading segment is empty.
        let mut splits = header_str.splitn(2, BEARER_PREFIX);
        ensure!(
            splits.next() == Some(""),
            AppError::InvalidCredentials("authorization header value must begin with 'bearer '".into()),
        );

        // Check the final segment and ensure we have a populated value.
        let token = match splits.next() {
            Some(token) if !token.is_empty() => token.to_string(),
            _ => bail!(AppError::InvalidCredentials("no token detected in header".into())),
        };
        let claims = TokenClaims::decode(&token, &config).map_err(|err| AppError::InvalidCredentials(err.to_string()))?;
        Ok(TokenCredentials { claims, header })
    }
}

/// The model of a JWT created by a Hadron cluster.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    /// The ID of this token.
    pub id: String,
    /// The name of the cluster which this token applies to.
    pub cluster: String,
}

impl TokenClaims {
    /// Create a new instance.
    pub fn new(cluster: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            cluster: cluster.into(),
        }
    }

    /// Encode this claims body as a JWT.
    pub fn encode(&self, config: &Config) -> jsonwebtoken::errors::Result<String> {
        let header = Header::new(Algorithm::RS512);
        jsonwebtoken::encode(&header, &self, &config.jwt_encoding_key)
    }

    /// Decode the given string as a JWT with a `TokenClaims` body.
    pub fn decode(token: impl AsRef<str>, config: &Config) -> jsonwebtoken::errors::Result<Self> {
        let validation = Validation::new(Algorithm::RS512);
        jsonwebtoken::decode(token.as_ref(), &config.jwt_decoding_key, &validation).map(|body| body.claims)
    }
}
