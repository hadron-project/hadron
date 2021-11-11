//! Common library code.

use anyhow::{Context, Result};
use tonic::metadata::{AsciiMetadataKey, AsciiMetadataValue};

/// Client credentials.
#[derive(Clone)]
pub enum ClientCreds {
    Token(String, AsciiMetadataValue),
    User(String, String, AsciiMetadataValue),
}

impl ClientCreds {
    /// Create a new credentials set using the given token.
    pub fn new_with_token(token: &str) -> Result<Self> {
        let orig = token.to_string();
        let header: AsciiMetadataValue = format!("bearer {}", token).parse().context("error setting auth token")?;
        Ok(Self::Token(orig, header))
    }

    /// Create a new credentials set using the given username & password.
    pub fn new_with_password(username: &str, password: &str) -> Result<Self> {
        let user = username.to_string();
        let pass = password.to_string();
        let basic_auth = base64::encode(format!("{}:{}", username, password));
        let header: AsciiMetadataValue = format!("basic {}", basic_auth).parse().context("error setting username/password")?;
        Ok(Self::User(user, pass, header))
    }

    /// Get a cloned copy of the credential's header value.
    pub fn header(&self) -> (AsciiMetadataKey, AsciiMetadataValue) {
        match self {
            Self::Token(_, header) | Self::User(_, _, header) => (AsciiMetadataKey::from_static("authorization"), header.clone()),
        }
    }
}
