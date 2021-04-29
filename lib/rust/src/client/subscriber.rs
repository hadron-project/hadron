//! Subscriber client.

use std::collections::{HashMap, HashSet};
use std::hash::Hasher;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use anyhow::{bail, Context, Result};
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

use crate::client::H2DataChannel;
use crate::handler::Handler;
use crate::Client;

impl Client {
    /// Create a new subscription on the target stream.
    pub fn subscribe(&self, handler: Arc<dyn Handler>, ns: &str, stream: &str, group: &str, config: Option<SubscriberConfig>) -> Subscription {
        let config = config.unwrap_or_default();
        let (tx, rx) = oneshot::channel();
        let handle = SubscriptionTask::new(self.clone(), ns.into(), stream.into(), group.into(), config, rx, handler).spawn();
        Subscription { shutdown: tx, handle }
    }
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

/// A handle to a subscription.
pub struct Subscription {
    shutdown: oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

impl Subscription {
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
    client: Client,
    ns: String,
    stream: String,
    group: String,
    config: SubscriberConfig,
    shutdown: oneshot::Receiver<()>,
    handler: Arc<dyn Handler>,

    active_channels: HashSet<Arc<String>>,
    inbound: FuturesUnordered<SubscriberFut>,
    tasks_tx: mpsc::Sender<SubTask>,
    tasks_rx: ReceiverStream<SubTask>,
    buf: BytesMut,
}

impl SubscriptionTask {
    /// Create a new instance.
    fn new(
        client: Client, ns: String, stream: String, group: String, config: SubscriberConfig, shutdown: oneshot::Receiver<()>,
        handler: Arc<dyn Handler>,
    ) -> Self {
        let (tasks_tx, tasks_rx) = mpsc::channel(10);
        Self {
            client,
            ns,
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
        tracing::debug!("starting subscription for {}/{}", self.ns, self.stream);

        // Perform an initial attempt at establishing cluster connections.
        if let Err(err) = self.build_connections().await {
            tracing::error!(error = ?err, "error building subscriber connections, attempting to build new connections in 10s");
            let tx = self.tasks_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let _ = tx.send(SubTask::BuildConnections).await;
            });
        }

        loop {
            tokio::select! {
                Some((node, delivery_opt)) = self.inbound.next() => self.handle_subscription_delivery(node, delivery_opt).await,
                Some(task) = self.tasks_rx.next() => self.handle_sub_task(task).await,
                _res = &mut self.shutdown => break,
            }
        }
    }

    /// Handle a self-delivered task.
    #[tracing::instrument(level = "debug", skip(self, task))]
    async fn handle_sub_task(&mut self, task: SubTask) {
        // TODO: handle tasks
    }

    /// Handle a subscription delivery from a specific node.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn handle_subscription_delivery(&mut self, node: Arc<String>, delivery_opt: Option<(H2DataChannel, Bytes)>) {
        // Unpack the delivery options, pruning the channel if it is dead.
        let (mut chan, data) = match delivery_opt {
            Some(delivery) => delivery,
            // When the delivery is `None`, this indicates that the channel is no longer alive
            // and needs to be removed.
            None => {
                let _ = self.active_channels.remove(&*node);
                let _ = self.tasks_tx.send(SubTask::ReconnectTo(node)).await;
                return;
            }
        };

        // Map the payload onto the subscription's handler, then ack or nack.
        let res = match self.try_handle_subscription_delivery(&*node, data).await {
            Ok(_) => self.ack(&mut chan),
            Err(err) => self.nack(&mut chan, err),
        };
        if let Err(err) = res {
            tracing::error!(error = ?err, "error responding to subscription delivery");
        }

        // Put the channel back into the inbound stream.
        self.inbound.push(SubscriberFut::new(node, chan));
    }

    /// Handle a subscription delivery from a specific node.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn try_handle_subscription_delivery(&mut self, node: &str, data: Bytes) -> Result<()> {
        let msg = StreamSubDelivery::decode(data).context("error decoding subscription delivery payload")?;
        let (tx, rx) = oneshot::channel();
        let handler = self.handler.clone();
        tokio::spawn(async move {
            let res = handler.handle(msg).await;
            let _ = tx.send(res);
        });
        rx.await.context("error awaiting subscription handler result")?
    }

    /// Respond with an `ack` on the given data channel.
    #[tracing::instrument(level = "debug", skip(self, chan))]
    fn ack(&mut self, chan: &mut H2DataChannel) -> Result<()> {
        let msg = StreamSubDeliveryResponse {
            result: Some(StreamSubDeliveryResponseResult::Ack(Default::default())),
        };
        let mut buf = self.buf.split();
        msg.encode(&mut buf).context("error encoding subscription ack response")?;
        chan.1.send_data(buf.freeze(), false).context("error sending subscription ack response")?;
        Ok(())
    }

    /// Respond with a `nack` on the given data channel.
    #[tracing::instrument(level = "debug", skip(self, chan, err))]
    fn nack(&mut self, chan: &mut H2DataChannel, err: anyhow::Error) -> Result<()> {
        let proto_err = v1::Error { message: err.to_string() };
        let msg = StreamSubDeliveryResponse {
            result: Some(StreamSubDeliveryResponseResult::Nack(proto_err)),
        };
        let mut buf = self.buf.split();
        msg.encode(&mut buf).context("error encoding subscription nack response")?;
        chan.1
            .send_data(buf.freeze(), false)
            .context("error sending subscription nack response")?;
        Ok(())
    }

    /// Consult the current metadata channel and build connections to any partitions of the target stream.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn build_connections(&mut self) -> Result<()> {
        // FUTURE: when the metadata system is implemented, we will connect to other nodes. For
        // now, just connect to the original target node.

        let url = self.client.0.url.clone();
        if self.active_channels.contains(&*url) {
            return Ok(());
        }
        let chan = self
            .setup_subscriber_channel(url.clone())
            .await
            .context("error setting up subscription channel")?;
        self.active_channels.insert(url.clone());
        self.inbound.push(SubscriberFut::new(url, chan));

        Ok(())
    }

    /// Setup a channel for use as a subscriber channel.
    #[tracing::instrument(level = "debug", skip(self, node))]
    async fn setup_subscriber_channel(&mut self, node: Arc<String>) -> Result<H2DataChannel> {
        // Build up request.
        let body_req = StreamSubSetupRequest {
            group_name: self.group.clone(),
            durable: self.config.durable,
            max_batch_size: self.config.max_batch_size,
            starting_point: Some(match self.config.starting_point {
                SubscriptionStartingPoint::Beginning => StreamSubSetupRequestStartingPoint::Beginning(Default::default()),
                SubscriptionStartingPoint::Latest => StreamSubSetupRequestStartingPoint::Latest(Default::default()),
                SubscriptionStartingPoint::Offset(offset) => StreamSubSetupRequestStartingPoint::Offset(offset),
            }),
        };
        let mut body = self.buf.clone().split();
        body_req.encode(&mut body).context("error encoding request")?;
        let uri = format!(
            "/{}/{}/{}/{}/{}",
            v1::URL_V1,
            v1::URL_STREAM,
            self.ns,
            self.stream,
            v1::URL_STREAM_SUBSCRIBE
        );
        let mut builder = Request::builder().method(Method::POST).uri(uri);
        builder = self.client.set_request_credentials(builder);
        let req = builder.body(()).context("error building request")?;

        // Open a new H2 channel to send request. Both ends are left open.
        let mut chan = self.client.get_channel(Some(node)).await?;
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
        let setup_res: StreamSubSetupResponse = self.client.deserialize_response(res_bytes)?;
        if let Some(StreamSubSetupResponseResult::Err(err)) = setup_res.result {
            bail!(err.message);
        }
        Ok((res.into_body(), tx))
    }
}

/// Subscription maintenance tasks.
enum SubTask {
    /// New connections need to be built.
    BuildConnections,
    /// Reconnect to the target node if still applicable for this subscription.
    ReconnectTo(Arc<String>),
}

//////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////

/// An H2 channel wrapper which resolves data frames along with the H2 channel.
pub struct SubscriberFut {
    node: Arc<String>,
    chan: Option<H2DataChannel>,
}

impl SubscriberFut {
    /// Create a new instance.
    pub fn new(node: Arc<String>, chan: H2DataChannel) -> Self {
        Self { node, chan: Some(chan) }
    }
}

impl Future for SubscriberFut {
    type Output = (Arc<String>, Option<(H2DataChannel, Bytes)>);

    /// Poll the underlying H2 channel for the next data frame.
    ///
    /// This implementation takes care to ensure that the underlying channel is resolved along with
    /// the data frame, which allows for the handler to use the response channel as needed.
    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut chan = match self.chan.take() {
            Some(chan) => chan,
            None => return Poll::Ready((self.node.clone(), None)),
        };
        let poll_res = chan.0.poll_data(cx);
        match poll_res {
            Poll::Pending => {
                self.chan = Some(chan);
                Poll::Pending
            }
            Poll::Ready(Some(val)) => match val {
                Ok(data) => Poll::Ready((self.node.clone(), Some((chan, data)))),
                Err(err) => {
                    tracing::error!(node = ?&*self.node, error = ?err, "error while awaiting subscription delivery from node");
                    self.chan = Some(chan);
                    Poll::Pending
                }
            },
            Poll::Ready(None) => {
                self.chan = Some(chan);
                Poll::Ready((self.node.clone(), None))
            }
        }
    }
}
