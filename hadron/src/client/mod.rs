use anyhow::{Context, Result};
use tonic::transport::{Channel, Uri};

use crate::common;
use crate::proto::client_client::ClientClient as ProtoClient;

/// A client connection to a Hadron cluster.
#[derive(Clone)]
pub struct Client {
    inner: ProtoClient<Channel>,
}

impl std::ops::Deref for Client {
    type Target = ProtoClient<Channel>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for Client {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl Client {
    /// Construct a new client instance.
    pub async fn new(url: &str) -> Result<Self> {
        let uri: Uri = url.parse().context("failed to parse URL")?;
        let channel = Channel::builder(uri)
            .http2_keep_alive_interval(common::DEFAULT_H2_KEEP_ALIVE_INTERVAL)
            .keep_alive_timeout(common::DEFAULT_H2_KEEP_ALIVE_TIMEOUT)
            .keep_alive_while_idle(true)
            .tcp_keepalive(Some(common::DEFAULT_H2_KEEP_ALIVE_INTERVAL))
            .tcp_nodelay(true)
            .connect()
            .await
            .context("error building client channel")?;
        let inner = ProtoClient::new(channel);
        Ok(Self { inner })
    }
}
