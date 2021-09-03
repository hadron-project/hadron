//! Stream subscriber client.

use std::collections::BTreeSet;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::Result;
use futures::prelude::*;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tonic::IntoStreamingRequest;
use tonic::{Status, Streaming};

use crate::client::Client;
use crate::grpc::stream::stream_subscribe_request::Action as StreamSubscribeRequestAction;
use crate::grpc::stream::stream_subscribe_setup::StartingPoint;
use crate::grpc::stream::{StreamSubscribeRequest, StreamSubscribeResponse, StreamSubscribeSetup};
use crate::handler::StreamHandler;

const DEFAULT_MAX_BATCH_SIZE: u32 = 1;

type StreamTask = Task<StreamSubscribeResponse, StreamSubscribeRequest>;

impl Client {
    /// Create a new subscription on the target stream.
    ///
    /// Once created, a subscription maintains individual connections with each partition of the
    /// cluster, and will not shut down in the face of errors; it will reconnect, and resume where
    /// it left off as needed.
    ///
    /// ## Parameters
    /// - `group`: the name to use for subscription group coordination. Any other subscribers of
    ///   the same stream using the same group name will have events load balanced between all
    ///   group members.
    /// - `config`: the optional subscriber config to use for this subscription.
    /// - `handler`: the handler to use for processing stream events.
    pub async fn subscribe(&self, group: &str, config: Option<SubscriberConfig>, handler: Arc<dyn StreamHandler>) -> Result<StreamSubscription> {
        let config = config.unwrap_or_default();
        let (shutdown, _) = broadcast::channel(1);
        let handle = SubscriptionTask::new(self.clone(), group.into(), config, shutdown.clone(), handler).spawn();

        Ok(StreamSubscription { shutdown, handle })
    }
}

/// A handle to a stream subscription.
pub struct StreamSubscription {
    shutdown: broadcast::Sender<()>,
    handle: JoinHandle<()>,
}

impl StreamSubscription {
    /// Cancel this subscription and await its completion.
    pub async fn cancel(self) {
        let _ = self.shutdown.send(());
        if let Err(err) = self.handle.await {
            tracing::error!(error = ?err, "error awaiting subscription shutdown");
        }
    }
}

/// A subscription task which manages all IO, handler invocation, and the subscription protocol overall.
struct SubscriptionTask {
    client: Client,
    group: String,
    config: SubscriberConfig,
    shutdown: broadcast::Sender<()>,
    handler: Arc<dyn StreamHandler>,

    active_subs: BTreeSet<u32>,
    tasks_tx: mpsc::Sender<StreamTask>,
    tasks_rx: ReceiverStream<StreamTask>,
}

impl SubscriptionTask {
    /// Create a new instance.
    fn new(client: Client, group: String, config: SubscriberConfig, shutdown: broadcast::Sender<()>, handler: Arc<dyn StreamHandler>) -> Self {
        let (tasks_tx, tasks_rx) = mpsc::channel(100);
        Self {
            client,
            group,
            config,
            shutdown,
            handler,
            active_subs: Default::default(),
            tasks_tx,
            tasks_rx: ReceiverStream::new(tasks_rx),
        }
    }

    fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        tracing::debug!(%self.group, "starting stream subscription");

        // Establish initial partition subscriptions.
        self.build_subscriptions().await;
        let mut shutdown = BroadcastStream::new(self.shutdown.subscribe());
        let mut conn_changes = self.client.inner.changes.clone();

        loop {
            tokio::select! {
                _ = conn_changes.changed() => self.build_subscriptions().await,
                Some(task) = self.tasks_rx.next() => self.handle_sub_task(task).await,
                _ = shutdown.next() => break,
            }
        }

        tracing::debug!(%self.group, "stream subscription has shut down");
    }

    #[tracing::instrument(level = "debug", skip(self, event))]
    async fn handle_sub_task(&mut self, event: StreamTask) {
        match event {
            Task::Delivery(partition, msg, res_tx) => self.handle_subscription_delivery(partition, msg, res_tx),
            Task::SubscriptionClosed(partition) => {
                self.active_subs.remove(&partition);
                self.build_subscriptions().await;
            }
            Task::EnsureSubscriptions => self.build_subscriptions().await,
        }
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn build_subscriptions(&mut self) {
        let conns = self.client.inner.conns.load();
        let max_batch_size = if self.config.max_batch_size == 0 {
            DEFAULT_MAX_BATCH_SIZE
        } else {
            self.config.max_batch_size
        };
        for (partition, chan) in conns.iter() {
            if self.active_subs.contains(&partition) {
                continue;
            }

            // Prepare and send the initial setup request.
            let (partition, mut chan) = (*partition, chan.clone());
            let (tx, rx) = mpsc::channel::<StreamSubscribeRequest>(1);
            let mut req = ReceiverStream::new(rx).into_streaming_request();
            let header = self.client.inner.creds.header();
            req.metadata_mut().insert(header.0, header.1);
            let (setup, setup_tx) = (
                StreamSubscribeRequest {
                    action: Some(StreamSubscribeRequestAction::Setup(StreamSubscribeSetup {
                        group_name: self.group.clone(),
                        durable: self.config.durable,
                        max_batch_size,
                        starting_point: Some(self.config.starting_point.clone().into()),
                    })),
                },
                tx.clone(),
            );
            tokio::spawn(async move {
                let _res = setup_tx.send(setup).await;
            });

            // Send the request over to the partition & await a response.
            let inbound = match chan.stream_subscribe(req).await {
                Ok(res) => res.into_inner(),
                Err(err) => {
                    tracing::error!(error = ?err, partition, "error opening stream subscription");
                    let tx = self.tasks_tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        tx.send(Task::EnsureSubscriptions).await
                    });
                    continue;
                }
            };
            self.active_subs.insert(partition);
            let sub_stream = SubscriptionStream::new(tx, inbound, partition);
            tokio::spawn(sub_stream.process(self.tasks_tx.clone(), self.shutdown.subscribe()));
        }
    }

    /// Handle a subscription delivery from a specific partition.
    #[tracing::instrument(level = "debug", skip(self, partition, msg, res_tx))]
    fn handle_subscription_delivery(&mut self, partition: u32, msg: StreamSubscribeResponse, res_tx: mpsc::Sender<StreamSubscribeRequest>) {
        tracing::debug!(partition, %self.group, "handling subscription delivery");
        let handler = self.handler.clone();
        tokio::spawn(async move {
            let res = handler.handle(msg).await;
            match res {
                Ok(_data) => Self::ack(res_tx).await,
                Err(err) => Self::nack(res_tx, err).await,
            }
        });
    }

    /// Respond to the server with an `ack` over the given channel.
    #[tracing::instrument(level = "debug", skip(res_tx))]
    async fn ack(res_tx: mpsc::Sender<StreamSubscribeRequest>) {
        let msg = StreamSubscribeRequest {
            action: Some(StreamSubscribeRequestAction::Ack(Default::default())),
        };
        let _res = res_tx.send(msg).await;
    }

    /// Respond to the server with an `nack` over the given channel.
    #[tracing::instrument(level = "debug", skip(res_tx, err))]
    async fn nack(res_tx: mpsc::Sender<StreamSubscribeRequest>, err: anyhow::Error) {
        let msg = StreamSubscribeRequest {
            action: Some(StreamSubscribeRequestAction::Nack(err.to_string())),
        };
        let _res = res_tx.send(msg).await;
    }
}

/// Internal tasks for a subscription object.
pub(super) enum Task<Res, Req> {
    /// Ensure all partitions have an active subscription.
    EnsureSubscriptions,
    /// A payload has been delivered from the server.
    Delivery(u32, Res, mpsc::Sender<Req>),
    /// The given partition subscription has closed.
    SubscriptionClosed(u32),
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

impl From<SubscriptionStartingPoint> for StartingPoint {
    fn from(src: SubscriptionStartingPoint) -> Self {
        match src {
            SubscriptionStartingPoint::Beginning => Self::Beginning(Default::default()),
            SubscriptionStartingPoint::Latest => Self::Latest(Default::default()),
            SubscriptionStartingPoint::Offset(val) => Self::Offset(val),
        }
    }
}

/// A stream of subscription deliveries along with its response channel.
pub(super) struct SubscriptionStream<Res, Req> {
    tx: mpsc::Sender<Req>,
    inbound: Streaming<Res>,
    partition: u32,
    closed: bool,
}

impl<Res, Req> SubscriptionStream<Res, Req> {
    /// Create a new instance.
    pub(super) fn new(tx: mpsc::Sender<Req>, inbound: Streaming<Res>, partition: u32) -> Self {
        Self { tx, inbound, partition, closed: false }
    }

    /// Process inbound data from this stream, fully managing its lifecycle.
    #[tracing::instrument(level = "debug", skip(self, tasks, shutdown))]
    pub(super) async fn process(mut self, tasks: mpsc::Sender<Task<Res, Req>>, mut shutdown: broadcast::Receiver<()>) {
        loop {
            tokio::select! {
                msg = self.next() => match msg {
                    Some(SubscriptionEvent::Delivery(msg, tx)) => {
                        if let Err(_err) = tasks.send(Task::Delivery(self.partition, msg, tx)).await {
                            break;
                        }
                    }
                    Some(SubscriptionEvent::Closed(err_opt)) => {
                        match err_opt {
                            Some(err) => tracing::error!(error = ?err, self.partition, "error from subscription stream, closing"),
                            None => tracing::debug!(self.partition, "subscription closed"),
                        }
                        break;
                    }
                    None => break,
                },
                _ = shutdown.recv() => break,
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let _res = tasks.send(Task::SubscriptionClosed(self.partition)).await;
    }
}

impl<Res, Req> Stream for SubscriptionStream<Res, Req> {
    type Item = SubscriptionEvent<Res, Req>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.closed {
            return Poll::Ready(None);
        }
        match self.inbound.poll_next_unpin(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(msg))) => Poll::Ready(Some(SubscriptionEvent::Delivery(msg, self.tx.clone()))),
            Poll::Ready(Some(Err(err))) => {
                self.closed = true;
                Poll::Ready(Some(SubscriptionEvent::Closed(Some(err))))
            }
            Poll::Ready(None) => {
                self.closed = true;
                Poll::Ready(Some(SubscriptionEvent::Closed(None)))
            }
        }
    }
}

/// An event coming from a partition subscription for a Stream or Pipeline.
pub(super) enum SubscriptionEvent<Res, Req> {
    /// A payload has been delivered from the server.
    Delivery(Res, mpsc::Sender<Req>),
    /// The subscription has closed, potentially with an error.
    Closed(Option<Status>),
}
