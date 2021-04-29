//! Publisher client.

use std::collections::HashMap;
use std::hash::Hasher;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use http::request::Request;
use http::Method;
use prost::Message;
use proto::v1::{self, StreamPubRequest, StreamPubResponse, StreamPubSetupRequest, StreamPubSetupResponse, StreamPubSetupResponseResult};

use crate::client::H2DataChannel;
use crate::Client;

impl Client {
    /// Create a new publisher client.
    ///
    /// TODO: refactor this to make it specific to a stream.
    pub fn publisher(&self, name: &str, ns: &str, stream: &str) -> PublisherClient {
        PublisherClient {
            inner: self.clone(),
            name: name.into(),
            ns: ns.into(),
            stream: stream.into(),
            publisher_channels: Default::default(),
        }
    }
}

/// A publisher client which manages a bidirection channels to target stream partitions.
pub struct PublisherClient {
    inner: Client,
    /// The name of this publisher instance.
    name: String,
    /// The namespace of the target stream.
    ns: String,
    /// The name of the target stream.
    stream: String,
    /// A mapping of partition nodes to data channels.
    publisher_channels: HashMap<Arc<String>, H2DataChannel>,
}

impl PublisherClient {
    /// Publish a single payload of data, optionally specifying the partition to target.
    ///
    /// TODO: docs
    #[tracing::instrument(level = "debug", skip(self, data))]
    pub async fn publish_payload(&mut self, data: Vec<u8>) -> Result<StreamPubResponse> {
        // FUTURE: if no partition specified, then hash the message key & select a partition.

        // Get a handle to an initialize publisher channel ready for publishing data.
        let mut body = self.inner.0.buf.clone().split();
        let chan = self
            .get_publisher_channel()
            .await
            .context("error getting publisher channel for publishing data")?;

        // Create a new publish request payload and send it over the channel.
        let body_req = StreamPubRequest { batch: vec![data] };
        body_req.encode(&mut body).context("error encoding request")?;

        // Send data and then wait for response.
        chan.1.send_data(body.freeze(), false).context("error publishing data to stream")?;
        let mut res_body = chan
            .0
            .data()
            .await
            .context("channel closed without receiving response")?
            .context("error awaiting response from server")?;
        let res = StreamPubResponse::decode(&mut res_body).context("error decoding response body")?;
        Ok(res)
    }

    /// Get a handle to the initialized publisher stream for the target stream partition.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn get_publisher_channel(&mut self) -> Result<&mut H2DataChannel> {
        // FUTURE: use the metadata system to find the node by its metadata partition name.

        // First we check the target channel to ensure it is ready for use. If not, then drop it.
        let node = self.inner.0.url.clone();
        if let Some(chan) = self.publisher_channels.get(&node) {
            if chan.0.is_end_stream() {
                let _ = self.publisher_channels.remove(&node);
            }
        }

        // Get the data channel to the target node if already available, else create one.
        let mut new_chan = None;
        if !self.publisher_channels.contains_key(&node) {
            new_chan = Some(
                self.setup_publisher_channel(node.clone())
                    .await
                    .context("error establishing new publisher channel")?,
            );
        }
        match new_chan {
            Some(chan) => Ok(self.publisher_channels.entry(node).or_insert(chan)),
            None => Ok(self.publisher_channels.get_mut(&node).context("error getting publisher channel")?),
        }
    }

    /// Setup a channel for use as a publisher channel.
    #[tracing::instrument(level = "debug", skip(self, node))]
    async fn setup_publisher_channel(&mut self, node: Arc<String>) -> Result<H2DataChannel> {
        // Build up request.
        let body_req = StreamPubSetupRequest { name: self.name.clone() };
        let mut body = self.inner.0.buf.clone().split();
        body_req.encode(&mut body).context("error encoding request")?;
        let uri = format!(
            "/{}/{}/{}/{}/{}",
            v1::URL_V1,
            v1::URL_STREAM,
            self.ns,
            self.stream,
            v1::URL_STREAM_PUBLISH
        );
        let mut builder = Request::builder().method(Method::POST).uri(uri);
        builder = self.inner.set_request_credentials(builder);
        let req = builder.body(()).context("error building request")?;

        // Open a new H2 channel to send request. Both ends are left open.
        let mut chan = self.inner.get_channel(Some(node)).await?;
        let (rx, mut tx) = chan.send_request(req, false).context("error sending request")?;
        tx.send_data(body.freeze(), false).context("error sending request body")?;
        let mut res = rx.await.context("error during request")?;
        tracing::info!(res = ?res, "response from server");

        // Decode response body to ensure our channel is ready for use.
        let res_bytes = res
            .body_mut()
            .data()
            .await
            .context("no response returned after setting up publisher stream")?
            .context("error getting response body")?;
        let setup_res: StreamPubSetupResponse = self.inner.deserialize_response(res_bytes)?;
        if let Some(StreamPubSetupResponseResult::Err(err)) = setup_res.result {
            bail!(err.message);
        }
        Ok((res.into_body(), tx))
    }
}
