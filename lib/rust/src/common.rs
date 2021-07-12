//! Common library code.

use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use h2::client::SendRequest;
use h2::{RecvStream, SendStream};
use http::HeaderValue;
use http::StatusCode;
use prost::Message;

/// A HTTP2 connection which is used to create multiplexed H2 streams/channels.
///
/// This type may be cheaply cloned and moved across threads and tasks.
pub(crate) type H2Conn = SendRequest<Bytes>;
/// A HTTP2 stream/channel multiplexed over an underlying HTTP2 connection.
pub(crate) type H2Stream = (RecvStream, SendStream<Bytes>);

/// Deserialize the given response body as a concrete type, or an error depending on the status code.
#[tracing::instrument(level = "debug", skip(status, buf))]
pub(crate) fn deserialize_response_or_error<M: Message + Default>(status: StatusCode, buf: Bytes) -> Result<M> {
    // If not a successful response, then interpret the message as an error.
    if !status.is_success() {
        let body_err = proto::v1::Error::decode(buf.as_ref()).context("failed to deserialize error message from body")?;
        return Err(anyhow!(body_err.message));
    }
    M::decode(buf.as_ref()).context("error decoding response body")
}

/// Client credentials.
#[derive(Clone)]
pub enum ClientCreds {
    Token(String, http::HeaderValue),
    User(String, String, http::HeaderValue),
}

impl ClientCreds {
    /// Create a new credentials set using the given token.
    pub fn new_with_token(token: &str) -> Result<Self> {
        let orig = token.to_string();
        let header: HeaderValue = format!("bearer {}", token).parse().context("error setting auth token")?;
        Ok(Self::Token(orig, header))
    }

    /// Create a new credentials set using the given username & password.
    pub fn new_with_password(username: &str, password: &str) -> Result<Self> {
        let user = username.to_string();
        let pass = password.to_string();
        let basic_auth = base64::encode(format!("{}:{}", username, password));
        let header: HeaderValue = format!("basic {}", basic_auth)
            .parse()
            .context("error setting username/password")?;
        Ok(Self::User(user, pass, header))
    }

    /// Get a cloned copy of the credential's header value.
    pub fn header(&self) -> http::HeaderValue {
        match self {
            Self::Token(_, header) | Self::User(_, _, header) => header.clone(),
        }
    }
}
