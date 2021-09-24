//! Publisher client.

use std::hash::Hasher;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tonic::{transport::Channel, Request};

use crate::client::Client;
use crate::grpc::stream::{stream_controller_client::StreamControllerClient, Event, StreamPublishRequest, StreamPublishResponse, WriteAck};

impl Client {
    /// Create a new publisher for the target stream.
    ///
    /// ## Parameters
    /// - `name`: the name of this publisher.
    pub async fn publisher(&self, name: &str) -> Result<PublisherClient> {
        Ok(PublisherClient {
            client: self.clone(),
            name: Arc::new(name.into()),
            rr_partition: 0,
        })
    }
}

/// A client for publishing data to a stream.
///
/// Publisher instances can be cheeply cloned as needed, however round-robin load balancing is
/// tracked per instance, so if your events are being published with an empty `key`, which will
/// cause round-robin load balancing to be used, then do not clone the instance for each publication.
#[derive(Clone)]
pub struct PublisherClient {
    client: Client,
    name: Arc<String>,
    rr_partition: u32,
}

impl PublisherClient {
    /// Wait for at least one active connection to be available for use.
    ///
    /// If a `timeout` is provided, then wait at most `timeout` seconds before returning an error.
    /// If no `timeout` is provided, or connections become available before the `timeout` elapses,
    /// then no error will be returned.
    #[tracing::instrument(level = "debug", skip(self, timeout))]
    pub async fn ready(&mut self, timeout: Option<Duration>) -> Result<()> {
        let timer = match timeout {
            Some(timeout) => futures::future::Either::Left(tokio::time::sleep(timeout)),
            None => futures::future::Either::Right(futures::future::pending()),
        };
        let mut conn_changes = self.client.inner.changes.clone();
        tokio::pin!(timer);
        loop {
            tokio::select! {
                _ = &mut timer => return Err(anyhow::anyhow!("timeout while waiting for available connections")),
                _ = conn_changes.changed() => (),
            }
            if self.client.inner.conns.load().is_empty() {
                continue;
            } else {
                return Ok(());
            }
        }
    }

    /// Publish a single event.
    #[tracing::instrument(level = "debug", skip(self, event, ack, fsync))]
    pub async fn publish(&mut self, event: Event, ack: WriteAck, fsync: bool) -> Result<StreamPublishResponse> {
        self.try_publish_event(StreamPublishRequest { batch: vec![event], ack: ack as i32, fsync })
            .await
    }

    /// Publish a batch of events.
    ///
    /// They key of the first event in the given batch will be used to determine placement of the batch.
    #[tracing::instrument(level = "debug", skip(self, batch, ack, fsync))]
    pub async fn publish_batch(&mut self, batch: Vec<Event>, ack: WriteAck, fsync: bool) -> Result<StreamPublishResponse> {
        self.try_publish_event(StreamPublishRequest { batch, ack: ack as i32, fsync })
            .await
    }

    /// Publish the given event to the target stream.
    #[tracing::instrument(level = "debug", skip(self, proto))]
    async fn try_publish_event(&mut self, proto: StreamPublishRequest) -> Result<StreamPublishResponse> {
        let (id, source) = proto
            .batch
            .get(0)
            .map(|val| (val.id.as_str(), val.source.as_str()))
            .unwrap_or(("", ""));
        let mut conn = self
            .select_partition(id, source)
            .ok_or_else(|| anyhow!("no partitions available"))?;

        let header = self.client.inner.creds.header();
        let mut req = Request::new(proto);
        req.metadata_mut().insert(header.0, header.1);
        tracing::debug!("publishing request");
        let res = conn
            .stream_publish(req)
            .await
            .context("error publishing request to stream")?;

        tracing::debug!("request published");
        Ok(res.into_inner())
    }

    /// Select a partition to which the given event key should be published.
    ///
    /// If the given key is empty, then a partition will be selected based on a round robin algorithm.
    fn select_partition(&mut self, id: &str, source: &str) -> Option<StreamControllerClient<Channel>> {
        let conns = self.client.inner.conns.load();
        if conns.is_empty() {
            return None;
        }

        let mut hasher = seahash::SeaHasher::default();
        hasher.write(id.as_bytes());
        hasher.write(source.as_bytes());
        let offset = (hasher.finish() % conns.len() as u64) as u32;
        conns.get(&offset).cloned()
    }
}
