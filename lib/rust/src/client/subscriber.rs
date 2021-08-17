//! Stream subscriber client.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use bytes::{Bytes, BytesMut};
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use http::request::Request;
use http::Method;
use prost::Message;
use proto::v1::{
    self, StreamSubDelivery, StreamSubDeliveryResponse, StreamSubDeliveryResponseResult, StreamSubSetupRequest, StreamSubSetupRequestStartingPoint,
    StreamSubSetupResponse, StreamSubSetupResponseResult,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::client::{Client, ClientCreds, ClientEvent, ClientEventTx, ConnectionState, PartitionConnectionsSignalRx, AUTH_HEADER};
use crate::common::{deserialize_response_or_error, H2Conn, H2Stream};
use crate::futures::SubscriberFut;
use crate::handler::StreamHandler;

impl Client {
    /// Create a new subscription on the target stream.
    ///
    /// ## Parameters
    /// - `stream`: the name of the stream to subscribe to.
    /// - `group`: the name to use for subscription group coordination. Any other subscribers of
    ///   the same stream using the same group name will have events load balanced between all
    ///   group members.
    /// - `config`: the optional subscriber config to use for this subscription.
    /// - `handler`: the handler to use for processing stream events.
    pub async fn subscribe(
        &self, stream: &str, group: &str, config: Option<SubscriberConfig>, handler: Arc<dyn StreamHandler>,
    ) -> Result<StreamSubscription> {
        let (tx, rx) = oneshot::channel();
        self.0
            .events
            .send(ClientEvent::CreateStreamSubscriber { tx, stream: stream.into() })
            .await
            .map_err(|err| anyhow!("error sending stream subscription creation request to client core: {}", err))
            .context("error while attempting to create stream subscriber")?;
        let (id, events_tx, conns) = rx.await.context("error waiting for response from client core")?;

        let config = config.unwrap_or_default();
        let (shutdown, shutdown_rx) = oneshot::channel();
        let handle = SubscriptionTask::new(
            self.clone(),
            id,
            conns,
            events_tx,
            stream.into(),
            group.into(),
            config,
            shutdown_rx,
            handler,
        )
        .spawn();

        Ok(StreamSubscription { shutdown, handle })
    }
}

/// A handle to a stream subscription.
pub struct StreamSubscription {
    shutdown: oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

impl StreamSubscription {
    /// Cancel this subscription.
    pub async fn cancel(self) {
        let _ = self.shutdown.send(());
        if let Err(err) = self.handle.await {
            tracing::error!(error = ?err, "error awaiting subscription shutdown");
        }
    }
}

/// A subscription task which manages all IO, handler invocation, and the subscription protocol overall.
struct SubscriptionTask {
    id: Uuid,
    conns: PartitionConnectionsSignalRx,
    events: ClientEventTx,
    client: Client,
    stream: String,
    group: String,
    config: SubscriberConfig,
    shutdown: oneshot::Receiver<()>,
    handler: Arc<dyn StreamHandler>,

    /// Live H2 streams/channels to specific pods.
    active_channels: HashSet<Arc<String>>,
    /// A stream of pending deliveries from the cluster.
    inbound: FuturesUnordered<SubscriberFut>,
    tasks_tx: mpsc::Sender<Task>,
    tasks_rx: ReceiverStream<Task>,
    buf: BytesMut,
}

impl SubscriptionTask {
    /// Create a new instance.
    fn new(
        client: Client, id: Uuid, conns: PartitionConnectionsSignalRx, events: ClientEventTx, stream: String, group: String,
        config: SubscriberConfig, shutdown: oneshot::Receiver<()>, handler: Arc<dyn StreamHandler>,
    ) -> Self {
        let (tasks_tx, tasks_rx) = mpsc::channel(10);
        Self {
            client,
            id,
            conns,
            events,
            stream,
            group,
            config,
            shutdown,
            handler,
            active_channels: Default::default(),
            inbound: Default::default(),
            tasks_tx,
            tasks_rx: ReceiverStream::new(tasks_rx),
            buf: BytesMut::with_capacity(1000),
        }
    }

    fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        tracing::debug!("starting stream subscription for {}", self.stream);

        // Establish initial partition H2 streams.
        self.ensure_partition_streams().await;

        loop {
            tokio::select! {
                Ok(_) = self.conns.changed() => self.ensure_partition_streams().await,
                Some((node, delivery_opt)) = self.inbound.next() => self.handle_subscription_delivery(node, delivery_opt).await,
                Some(task) = self.tasks_rx.next() => self.handle_sub_task(task).await,
                _ = &mut self.shutdown => break,
            }
        }

        let _res = self
            .events
            .send(ClientEvent::StreamSubscriberClosed { stream: self.stream.clone(), id: self.id })
            .await;
        tracing::debug!("stream subscription {} has shut down", self.stream);
    }

    /// Handle a self-delivered task.
    #[tracing::instrument(level = "debug", skip(self, task))]
    async fn handle_sub_task(&mut self, task: Task) {
        match task {
            Task::BuildConnections => self.ensure_partition_streams().await,
        }
    }

    /// Handle a subscription delivery from a specific node.
    #[tracing::instrument(level = "debug", skip(self, pod, delivery_opt))]
    async fn handle_subscription_delivery(&mut self, pod: Arc<String>, delivery_opt: Option<(H2Stream, Bytes)>) {
        // Unpack the delivery options, pruning the channel if it is dead.
        let (mut chan, data) = match delivery_opt {
            Some(delivery) => delivery,
            // When the delivery is `None`, this indicates that the channel is no longer alive
            // and needs to be removed.
            None => {
                tracing::warn!(
                    partition = pod.as_str(),
                    "connection to stream partition has been lost, will reconnect when available"
                );
                let _ = self.active_channels.remove(pod.as_ref());
                let _res = self.events.send(ClientEvent::DeadConnection(pod)).await;
                return;
            }
        };

        // Map the payload onto the subscription's handler, then ack or nack.
        let res = match self.try_handle_subscription_delivery(data).await {
            Ok(_) => self.ack(&mut chan),
            Err(err) => self.nack(&mut chan, err),
        };
        if let Err(err) = res {
            tracing::error!(error = ?err, "error responding to stream subscription delivery");
        }

        // Put the channel back into the inbound stream.
        self.inbound.push(SubscriberFut::new(pod, chan));
    }

    /// Handle a subscription delivery from a specific node.
    #[tracing::instrument(level = "debug", skip(self, data))]
    async fn try_handle_subscription_delivery(&mut self, data: Bytes) -> Result<()> {
        let msg = StreamSubDelivery::decode(data).context("error decoding stream subscription delivery payload")?;
        let (tx, rx) = oneshot::channel();
        let handler = self.handler.clone();
        tokio::spawn(async move {
            let res = handler.handle(msg).await;
            let _ = tx.send(res);
        });
        rx.await.context("error awaiting stream subscription handler result")?
    }

    /// Respond with an `ack` on the given data channel.
    #[tracing::instrument(level = "debug", skip(self, chan))]
    fn ack(&mut self, chan: &mut H2Stream) -> Result<()> {
        let msg = StreamSubDeliveryResponse {
            result: Some(StreamSubDeliveryResponseResult::Ack(Default::default())),
        };
        let mut buf = self.buf.split();
        msg.encode(&mut buf)
            .context("error encoding stream subscription ack response")?;
        chan.1
            .send_data(buf.freeze(), false)
            .context("error sending stream subscription ack response")?;
        Ok(())
    }

    /// Respond with a `nack` on the given data channel.
    #[tracing::instrument(level = "debug", skip(self, chan, err))]
    fn nack(&mut self, chan: &mut H2Stream, err: anyhow::Error) -> Result<()> {
        let proto_err = v1::Error { message: err.to_string() };
        let msg = StreamSubDeliveryResponse {
            result: Some(StreamSubDeliveryResponseResult::Nack(proto_err)),
        };
        let mut buf = self.buf.split();
        msg.encode(&mut buf)
            .context("error encoding stream subscription nack response")?;
        chan.1
            .send_data(buf.freeze(), false)
            .context("error sending stream subscription nack response")?;
        Ok(())
    }

    /// Ensure a HTTP2 stream/channel exists for each viable connection.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn ensure_partition_streams(&mut self) {
        // Collect into a vec to ensure we don't hold the lock from the `conns.borrow()`.
        let conns: Vec<_> = self
            .conns
            .borrow()
            .iter()
            .filter(|(_, (pod, _))| !self.active_channels.contains(pod.as_ref()))
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
                &mut self.inbound,
                pod,
                &self.stream,
                partition,
                &self.group,
                &self.config,
                self.client.credentials(),
                self.buf.clone().split(),
                conn,
            )
            .await
            {
                tracing::error!(error = ?err, "error building stream subscriber connections, attempting to build new connections in 10s");
                needs_retry = true;
            }
        }
        if needs_retry {
            let tx = self.tasks_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let _ = tx.send(Task::BuildConnections).await;
            });
        }
    }

    /// Try to build H2 streams to all partitions of the target stream.
    #[tracing::instrument(
        level = "debug",
        skip(active_channels, inbound, pod, stream, partition, group, config, credentials, buf, conn)
    )]
    async fn try_build_h2_stream(
        active_channels: &mut HashSet<Arc<String>>, inbound: &mut FuturesUnordered<SubscriberFut>, pod: Arc<String>, stream: &str, partition: u8,
        group: &str, config: &SubscriberConfig, credentials: Arc<ClientCreds>, buf: BytesMut, conn: H2Conn,
    ) -> Result<()> {
        let chan = Self::setup_subscriber_channel(stream, partition, group, config, credentials, buf, conn.clone())
            .await
            .context("error setting up stream subscription channel")?;
        active_channels.insert(pod.clone());
        inbound.push(SubscriberFut::new(pod.clone(), chan));
        Ok(())
    }

    /// Setup a channel for use as a subscriber channel.
    #[tracing::instrument(level = "debug", skip(stream, partition, group, config, credentials, buf, conn))]
    async fn setup_subscriber_channel(
        stream: &str, partition: u8, group: &str, config: &SubscriberConfig, credentials: Arc<ClientCreds>, buf: BytesMut, mut conn: H2Conn,
    ) -> Result<H2Stream> {
        // Build up request.
        let body_req = StreamSubSetupRequest {
            group_name: group.to_string(),
            durable: config.durable,
            max_batch_size: config.max_batch_size,
            starting_point: Some(match config.starting_point {
                SubscriptionStartingPoint::Beginning => StreamSubSetupRequestStartingPoint::Beginning(Default::default()),
                SubscriptionStartingPoint::Latest => StreamSubSetupRequestStartingPoint::Latest(Default::default()),
                SubscriptionStartingPoint::Offset(offset) => StreamSubSetupRequestStartingPoint::Offset(offset),
            }),
        };
        let mut body = buf;
        body_req.encode(&mut body).context("error encoding request")?;
        let uri = format!(
            "/{}/{}/{}/{}/{}",
            v1::URL_V1,
            v1::URL_STREAM,
            &stream,
            partition,
            v1::URL_STREAM_SUBSCRIBE
        );
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
            .context("no response returned after setting up subscriber stream")?
            .context("error getting response body")?;
        let setup_res: StreamSubSetupResponse = deserialize_response_or_error(res.status(), res_bytes)?;
        if let Some(StreamSubSetupResponseResult::Err(err)) = setup_res.result {
            bail!(err.message);
        }
        Ok((res.into_body(), tx))
    }
}

/// Subscription maintenance tasks.
enum Task {
    /// Build connections to any partitions of the target stream, performing reconnects as needed.
    BuildConnections,
}

/// Subscriber configuration.
#[derive(Clone, Debug)]
pub struct SubscriberConfig {
    /// A bool indicating if this subscription should be considered durable; if `false`, then its
    /// offsets will be held in memory only.
    pub durable: bool,
    /// The maximum batch size for this subscriber.
    pub max_batch_size: u32,
    /// The starting point of a new subscription.
    pub starting_point: SubscriptionStartingPoint,
}

impl Default for SubscriberConfig {
    fn default() -> Self {
        Self {
            durable: true,
            max_batch_size: 50,
            starting_point: SubscriptionStartingPoint::default(),
        }
    }
}

/// The starting point of a new subscription.
///
/// Starting points are ignored for durable subscriptions as durable subscriptions always resume
/// from their last recorded offset.
#[derive(Clone, Debug)]
pub enum SubscriptionStartingPoint {
    /// The very beginning of the stream.
    Beginning,
    /// The most recent record on the stream.
    Latest,
    /// A specific offset, defaulting to the latest record
    /// on the stream if the offset is out of bounds.
    Offset(u64),
}

impl Default for SubscriptionStartingPoint {
    fn default() -> Self {
        Self::Latest
    }
}
