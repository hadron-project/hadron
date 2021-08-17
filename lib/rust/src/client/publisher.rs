//! Publisher client.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use bytes::BytesMut;
use futures::prelude::*;
use http::request::Request;
use http::Method;
use prost::Message;
use proto::v1::{self, NewEvent, StreamPubRequest, StreamPubResponse, StreamPubSetupRequest, StreamPubSetupResponse, StreamPubSetupResponseResult};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::client::{Client, ClientCreds, ClientEvent, ClientEventTx, ConnectionState, PartitionConnectionsSignalRx, AUTH_HEADER};
use crate::common::{deserialize_response_or_error, H2Conn, H2Stream};

impl Client {
    /// Create a new publisher for the target stream.
    ///
    /// ## Parameters
    /// - `name`: the name of this publisher.
    /// - `stream`: the name of the stream which events are to be published to.
    pub async fn publisher(&self, name: &str, stream: &str) -> Result<PublisherClient> {
        let (tx, rx) = oneshot::channel();
        self.0
            .events
            .send(ClientEvent::CreateStreamPublisher { stream: stream.into(), tx })
            .await
            .map_err(|err| anyhow!("error sending stream publisher creation request to client core: {}", err))
            .context("error while attempting to create stream publisher")?;
        let (id, events_tx, conns) = rx.await.context("error waiting for response from client core")?;

        let (shutdown, shutdown_rx) = oneshot::channel();
        let (client_events_tx, task) = PublisherTask::new(self.clone(), id, conns, events_tx, name.into(), stream.into(), shutdown_rx);
        let handle = Some(task.spawn());
        Ok(PublisherClient { handle, client_events_tx, shutdown: Some(shutdown) })
    }
}

/// A client for publishing data to a stream.
pub struct PublisherClient {
    handle: Option<JoinHandle<()>>,
    client_events_tx: mpsc::Sender<PublisherEvent>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for PublisherClient {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _res = shutdown.send(());
        }
        if let Some(handle) = self.handle.take() {
            tokio::spawn(async move {
                let _res = handle.await;
            });
        }
    }
}

impl PublisherClient {
    /// Publish a single event.
    #[tracing::instrument(level = "debug", skip(self, event))]
    pub async fn publish(&mut self, event: NewEvent) -> Result<StreamPubResponse> {
        let (tx, rx) = oneshot::channel();
        let _res = self
            .client_events_tx
            .send(PublisherEvent::Publish { tx, event })
            .await
            .map_err(|err| anyhow!("error sending publish event to publisher task: {}", err))
            .context("error publishing payload")?;
        rx.await.context("error awaiting publish result")?
    }

    /// Publish a batch of events.
    ///
    /// They key of the first event in the given batch will be used to determine placement of the batch.
    #[tracing::instrument(level = "debug", skip(self, events))]
    pub async fn publish_batch(&mut self, events: Vec<NewEvent>) -> Result<StreamPubResponse> {
        let (tx, rx) = oneshot::channel();
        let _res = self
            .client_events_tx
            .send(PublisherEvent::PublishBatch { tx, events })
            .await
            .map_err(|err| anyhow!("error sending publish event to publisher task: {}", err))
            .context("error publishing payload")?;
        rx.await.context("error awaiting publish result")?
    }
}

/// A publisher task which manages publication of data to the target stream.
struct PublisherTask {
    id: Uuid,
    conns: PartitionConnectionsSignalRx,
    core_events_tx: ClientEventTx,
    client: Client,
    name: String,
    stream: String,
    shutdown: oneshot::Receiver<()>,

    /// Live H2 streams/channels to specific partitions/pods.
    active_channels: BTreeMap<u8, (Arc<String>, H2Stream)>,
    /// The last partition to have been selected for round robin LB.
    rr_partition: Option<u8>,
    client_events_tx: mpsc::Sender<PublisherEvent>,
    client_events_rx: ReceiverStream<PublisherEvent>,
    buf: BytesMut,
}

impl PublisherTask {
    fn new(
        client: Client, id: Uuid, conns: PartitionConnectionsSignalRx, events: ClientEventTx, name: String, stream: String,
        shutdown: oneshot::Receiver<()>,
    ) -> (mpsc::Sender<PublisherEvent>, Self) {
        let (tasks_tx, tasks_rx) = mpsc::channel(10);
        (
            tasks_tx.clone(),
            Self {
                client,
                id,
                conns,
                core_events_tx: events,
                name,
                stream,
                shutdown,
                active_channels: Default::default(),
                rr_partition: None,
                client_events_tx: tasks_tx,
                client_events_rx: ReceiverStream::new(tasks_rx),
                buf: BytesMut::with_capacity(1000),
            },
        )
    }

    fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        tracing::debug!("starting stream publisher for {}", self.stream);

        // Establish initial partition H2 streams.
        self.ensure_partition_streams().await;

        loop {
            tokio::select! {
                Ok(_) = self.conns.changed() => self.ensure_partition_streams().await,
                Some(event) = self.client_events_rx.next() => self.handle_event(event).await,
                _ = &mut self.shutdown => break,
            }
        }

        let _res = self
            .core_events_tx
            .send(ClientEvent::StreamPublisherClosed { stream: self.stream.clone(), id: self.id })
            .await;
        tracing::debug!("stream publisher {} has shut down", self.stream);
    }

    /// Handle the given publisher event.
    #[tracing::instrument(level = "debug", skip(self, event))]
    async fn handle_event(&mut self, event: PublisherEvent) {
        match event {
            PublisherEvent::BuildConnections => self.ensure_partition_streams().await,
            PublisherEvent::Publish { event, tx } => self.handle_publish_event(event, tx).await,
            PublisherEvent::PublishBatch { events, tx } => self.handle_publish_events_batch(events, tx).await,
        }
    }

    /// Publish the given event to the target stream.
    #[tracing::instrument(level = "debug", skip(self, event, tx))]
    async fn handle_publish_event(&mut self, event: NewEvent, tx: oneshot::Sender<Result<StreamPubResponse>>) {
        let req = StreamPubRequest { batch: vec![event] };
        let res = self.try_publish_event(req).await;
        let _res = tx.send(res);
    }

    /// Publish the given events batch to the target stream.
    #[tracing::instrument(level = "debug", skip(self, events, tx))]
    async fn handle_publish_events_batch(&mut self, events: Vec<NewEvent>, tx: oneshot::Sender<Result<StreamPubResponse>>) {
        let req = StreamPubRequest { batch: events };
        let res = self.try_publish_event(req).await;
        let _res = tx.send(res);
    }

    /// Publish the given event to the target stream.
    #[tracing::instrument(level = "debug", skip(self, req))]
    async fn try_publish_event(&mut self, req: StreamPubRequest) -> Result<StreamPubResponse> {
        // Hash to a specific available partition, else round robin.
        let mut body = self.buf.clone().split();
        let key = req.batch.first().map(|event| event.key.as_str()).unwrap_or("");
        let (_pod, chan) = match self.select_partition(key) {
            Some((pod, chan)) => (pod, chan),
            None => bail!("no partition connections available"),
        };

        // Encode the request, send it and then wait for response.
        req.encode(&mut body).context("error encoding request")?;
        chan.1
            .send_data(body.freeze(), false)
            .context("error publishing data to stream")?;
        let mut res_body = chan
            .0
            .data()
            .await
            .context("channel closed without receiving response")?
            .context("error awaiting response from server")?;
        let res = StreamPubResponse::decode(&mut res_body).context("error decoding response body")?;
        Ok(res)
    }

    /// Select a partition to which the given event key shoule be published.
    ///
    /// If the given key is empty, then a partition will be selected based on a round robin algorithm.
    fn select_partition(&mut self, key: &str) -> Option<&mut (Arc<String>, H2Stream)> {
        if key.is_empty() {
            let next = self.rr_partition.and_then(|val| val.checked_add(1)).unwrap_or(0);
            // Get the partition by looking for the next logical partition number and on.
            let partition_opt = self.active_channels.range(next..).next().map(|val| *val.0)
                // Else, nothing else exists after that key, so start again from the beginning.
                .or_else(|| self.active_channels.range(..).next().map(|val| *val.0));
            match partition_opt {
                Some(partition) => {
                    self.rr_partition = Some(partition);
                    self.active_channels.get_mut(&partition)
                }
                None => None,
            }
        } else {
            if self.active_channels.is_empty() {
                return None;
            }
            let offset = seahash::hash(key.as_bytes()) % self.active_channels.len() as u64;
            self.active_channels.iter_mut().nth(offset as usize).map(|(_, val)| val)
        }
    }

    /// Ensure a HTTP2 stream/channel exists for each viable connection.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn ensure_partition_streams(&mut self) {
        // Collect into a vec to ensure we don't hold the lock from the `conns.borrow()`.
        let conns: Vec<_> = self
            .conns
            .borrow()
            .iter()
            .filter(|(partition, (_, _))| !self.active_channels.contains_key(partition))
            .filter_map(|(partition, (pod, conn_state))| match conn_state {
                ConnectionState::Connected(conn) => Some((*partition, pod.clone(), conn.clone())),
                _ => None,
            })
            .collect();

        // Iterate over all pod conns which need to have a new H2 stream created, and create the new stream.
        let mut needs_retry = false;
        for (partition, pod, conn) in conns {
            if let Err(err) = Self::try_build_h2_stream(
                &mut self.active_channels,
                pod,
                &self.name,
                &self.stream,
                partition,
                self.client.credentials(),
                self.buf.clone().split(),
                conn,
            )
            .await
            {
                tracing::error!(error = ?err, "error building stream publisher connection, retry in 10s");
                needs_retry = true;
            }
        }
        if needs_retry {
            let tx = self.client_events_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let _ = tx.send(PublisherEvent::BuildConnections).await;
            });
        }
    }

    /// Try to build H2 streams to all partitions of the target stream.
    #[tracing::instrument(level = "debug", skip(active_channels, pod, name, stream, partition, credentials, buf, conn))]
    async fn try_build_h2_stream(
        active_channels: &mut BTreeMap<u8, (Arc<String>, H2Stream)>, pod: Arc<String>, name: &str, stream: &str, partition: u8,
        credentials: Arc<ClientCreds>, buf: BytesMut, conn: H2Conn,
    ) -> Result<()> {
        let chan = Self::setup_publisher_channel(name, stream, partition, credentials, buf, conn.clone())
            .await
            .context("error setting up stream publisher channel")?;
        active_channels.insert(partition, (pod.clone(), chan));
        Ok(())
    }

    /// Setup a channel for use as a publisher channel.
    #[tracing::instrument(level = "debug", skip(name, stream, partition, credentials, buf, conn))]
    async fn setup_publisher_channel(
        name: &str, stream: &str, partition: u8, credentials: Arc<ClientCreds>, buf: BytesMut, mut conn: H2Conn,
    ) -> Result<H2Stream> {
        // Build up request.
        let body_req = StreamPubSetupRequest { name: name.into() };
        let mut body = buf;
        body_req.encode(&mut body).context("error encoding request")?;
        let uri = format!("/{}/{}/{}/{}/{}", v1::URL_V1, v1::URL_STREAM, &stream, partition, v1::URL_STREAM_PUBLISH);
        let req = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(AUTH_HEADER, credentials.header())
            .body(())
            .context("error building request")?;

        // Open a new H2 channel to send request. Both ends are left open.
        let (rx, mut tx) = conn.send_request(req, false).context("error sending request")?;
        tx.send_data(body.freeze(), false).context("error sending request body")?;
        let mut res = rx.await.context("error during request")?;

        // Decode response body to ensure our channel is ready for use.
        let res_bytes = res
            .body_mut()
            .data()
            .await
            .context("no response returned after setting up publisher stream")?
            .context("error getting response body")?;
        let setup_res: StreamPubSetupResponse = deserialize_response_or_error(res.status(), res_bytes)?;
        if let Some(StreamPubSetupResponseResult::Err(err)) = setup_res.result {
            bail!(err.message);
        }
        Ok((res.into_body(), tx))
    }
}

/// Publisher events.
enum PublisherEvent {
    /// Build connections to any partitions of the target stream, performing reconnects as needed.
    BuildConnections,
    /// Publish the given event to the target stream.
    Publish {
        /// The event to publish.
        event: NewEvent,
        /// The channel used for sending the response.
        tx: oneshot::Sender<Result<StreamPubResponse>>,
    },
    PublishBatch {
        /// The event to publish.
        events: Vec<NewEvent>,
        /// The channel used for sending the response.
        tx: oneshot::Sender<Result<StreamPubResponse>>,
    },
}
