//! The core Hadron client.

mod schema;

use anyhow::{Context, Result};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::transport::{Channel, Uri};

use crate::common;
use crate::proto::client_client::ClientClient as ProtoClient;

/// A client connection to a Hadron cluster.
///
/// The client is cheap to clone, everyone clone uses the same underlying multiplexed connection,
/// and clients may be freely passed between threads.
#[derive(Clone)]
pub struct Client {
    pub(crate) channel: ProtoClient<Channel>,
    /// The credentials associated with this client.
    pub(crate) creds: Option<ClientCreds>,
}

impl Client {
    /// Construct a new client instance.
    pub fn new(url: &str) -> Result<Self> {
        let uri: Uri = url.parse().context("failed to parse URL")?;
        let chan = Channel::builder(uri)
            .http2_keep_alive_interval(common::DEFAULT_H2_KEEP_ALIVE_INTERVAL)
            .keep_alive_timeout(common::DEFAULT_H2_KEEP_ALIVE_TIMEOUT)
            .keep_alive_while_idle(true)
            .tcp_keepalive(Some(common::DEFAULT_H2_KEEP_ALIVE_INTERVAL))
            .tcp_nodelay(true)
            .connect_lazy()
            .context("error building client channel")?;
        let channel = ProtoClient::new(chan);
        Ok(Self { channel, creds: None })
    }

    /// Attach a token to this client to be used in each request from this client.
    pub fn with_token(mut self, token: &str) -> Result<Self> {
        let orig = token.to_string();
        let header: MetadataValue<Ascii> = format!("bearer {}", token).parse().context("error setting auth token")?;
        self.creds = Some(ClientCreds::Token(orig, header));
        Ok(self)
    }

    /// Attach a username/password to this client to be used in each request from this client.
    pub fn with_password(mut self, username: &str, password: &str) -> Result<Self> {
        let user = username.to_string();
        let pass = password.to_string();
        let basic_auth = base64::encode(format!("{}:{}", username, password));
        let header: MetadataValue<Ascii> = format!("basic {}", basic_auth).parse().context("error setting username/password")?;
        self.creds = Some(ClientCreds::User(user, pass, header));
        Ok(self)
    }

    /// Set the credentials for the request based on the client's current credentials.
    pub(crate) fn set_request_credentials<T>(&self, req: &mut tonic::Request<T>) {
        match &self.creds {
            Some(creds) => match creds {
                ClientCreds::Token(_, header) | ClientCreds::User(_, _, header) => {
                    req.metadata_mut().insert("x-hadron-authorization", header.clone());
                }
            },
            None => (),
        }
    }

    /// Get a cloned handle to the inner gRPC channel.
    pub(crate) fn channel(&self) -> ProtoClient<Channel> {
        self.channel.clone()
    }
}

/// Client credentials.
#[derive(Clone)]
pub(crate) enum ClientCreds {
    Token(String, MetadataValue<Ascii>),
    User(String, String, MetadataValue<Ascii>),
}
