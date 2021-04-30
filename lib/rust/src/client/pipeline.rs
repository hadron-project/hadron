//! Pipeline subscriber client.

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
    self, PipelineStageOutput, PipelineSubDelivery, PipelineSubDeliveryResponse, PipelineSubDeliveryResponseResult, PipelineSubSetupRequest,
    PipelineSubSetupResponse, PipelineSubSetupResponseResult,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;

use crate::common::{H2Channel, H2DataChannel};
use crate::futures::SubscriberFut;
use crate::handler::PipelineHandler;
use crate::Client;

impl Client {
    /// Create a new pipeline subscription on the target pipeline stage.
    pub fn pipeline(&self, handler: Arc<dyn PipelineHandler>, ns: &str, pipeline: &str, stage: &str) -> PipelineSubscription {
        let (tx, rx) = oneshot::channel();
        let handle = SubscriptionTask::new(self.clone(), ns.into(), pipeline.into(), stage.into(), rx, handler).spawn();
        PipelineSubscription { shutdown: tx, handle }
    }
}

/// A handle to a subscription.
pub struct PipelineSubscription {
    shutdown: oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

impl PipelineSubscription {
    /// Cancel this subscription.
    pub async fn cancel(self) {
        let _ = self.shutdown.send(());
        if let Err(err) = self.handle.await {
            tracing::error!(error = ?err, "error awaiting subscription shutdown");
        }
    }
}

/// A pipeline subscription task which manages all IO, handler invocation, and the subscription protocol overall.
struct SubscriptionTask {
    client: Client,
    ns: String,
    pipeline: String,
    stage: String,
    shutdown: oneshot::Receiver<()>,
    handler: Arc<dyn PipelineHandler>,

    /// Active pipeline subscriptions to specific nodes.
    active_channels: HashSet<Arc<String>>,
    /// A stream of pending deliveries from the cluster.
    inbound: FuturesUnordered<SubscriberFut>,
    tasks_tx: mpsc::Sender<SubTask>,
    tasks_rx: ReceiverStream<SubTask>,
    buf: BytesMut,
}

impl SubscriptionTask {
    /// Create a new instance.
    fn new(client: Client, ns: String, pipeline: String, stage: String, shutdown: oneshot::Receiver<()>, handler: Arc<dyn PipelineHandler>) -> Self {
        let (tasks_tx, tasks_rx) = mpsc::channel(10);
        Self {
            client,
            ns,
            pipeline,
            stage,
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
        tracing::debug!("starting pipeline subscription for {}/{}", self.ns, self.pipeline);

        // Perform an initial attempt at establishing cluster connections.
        self.build_connections().await;

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
        match task {
            SubTask::BuildConnections => self.build_connections().await,
        }
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
                tracing::warn!(partition = ?&*node, "connection to partition has been lost, reconnecting");
                let _ = self.active_channels.remove(&*node);
                let _ = self.tasks_tx.send(SubTask::BuildConnections).await;
                return;
            }
        };

        // Map the payload onto the subscription's handler, then ack or nack.
        let res = match self.try_handle_subscription_delivery(&*node, data).await {
            Ok(output) => self.ack(&mut chan, output),
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
    async fn try_handle_subscription_delivery(&mut self, node: &str, data: Bytes) -> Result<Bytes> {
        let msg = PipelineSubDelivery::decode(data).context("error decoding subscription delivery payload")?;
        let (tx, rx) = oneshot::channel();
        let handler = self.handler.clone();
        tokio::spawn(async move {
            let res = handler.handle(msg).await;
            let _ = tx.send(res);
        });
        rx.await.context("error awaiting subscription handler result")?
    }

    /// Respond with an `ack` on the given data channel.
    #[tracing::instrument(level = "debug", skip(self, chan, output))]
    fn ack(&mut self, chan: &mut H2DataChannel, output: Bytes) -> Result<()> {
        let msg = PipelineSubDeliveryResponse {
            result: Some(PipelineSubDeliveryResponseResult::Ack(PipelineStageOutput { output: output.to_vec() })),
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
        let msg = PipelineSubDeliveryResponse {
            result: Some(PipelineSubDeliveryResponseResult::Nack(proto_err)),
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
    async fn build_connections(&mut self) {
        // FUTURE[metadata]: once metadata system is in place, then only build connections to partitions
        // to which we do not have a live connection.
        if let Err(err) = self.try_build_connections().await {
            tracing::error!(error = ?err, "error building pipeline subscriber connections, attempting to build new connections in 10s");
            let tx = self.tasks_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let _ = tx.send(SubTask::BuildConnections).await;
            });
        }
    }

    /// Try to build connections to all partitions of the target stream.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn try_build_connections(&mut self) -> Result<()> {
        let url = self.client.0.url.clone();
        if self.active_channels.contains(&*url) {
            return Ok(());
        }
        let chan = self
            .setup_subscriber_channel(url.clone())
            .await
            .context("error setting up pipeline subscription channel")?;
        self.active_channels.insert(url.clone());
        self.inbound.push(SubscriberFut::new(url, chan));

        Ok(())
    }

    /// Setup a channel for use as a subscriber channel.
    #[tracing::instrument(level = "debug", skip(self, node))]
    async fn setup_subscriber_channel(&mut self, node: Arc<String>) -> Result<H2DataChannel> {
        // Build up request.
        let body_req = PipelineSubSetupRequest {
            stage_name: self.stage.clone(),
        };
        let mut body = self.buf.clone().split();
        body_req.encode(&mut body).context("error encoding request")?;
        let uri = format!("/{}/{}/{}/{}", v1::URL_V1, v1::URL_PIPELINE, self.ns, self.pipeline);
        let mut builder = Request::builder().method(Method::POST).uri(uri);
        builder = self.client.set_request_credentials(builder);
        let req = builder.body(()).context("error building request")?;

        // Open a new H2 channel to send request. Both ends are left open.
        let mut chan = self.client.get_channel(Some(node)).await?;
        let (rx, mut tx) = chan.send_request(req, false).context("error sending request")?;
        tx.send_data(body.freeze(), false).context("error sending request body")?;
        let mut res = rx.await.context("error during request")?;
        tracing::info!(status = ?res.status(), headers = ?res.headers(), "response from server");

        // Decode response body to ensure our channel is ready for use.
        let res_bytes = res
            .body_mut()
            .data()
            .await
            .context("no response returned after setting up publisher stream")?
            .context("error getting response body")?;
        let setup_res: PipelineSubSetupResponse = self.client.deserialize_response_or_error(res.status(), res_bytes)?;
        if let Some(PipelineSubSetupResponseResult::Err(err)) = setup_res.result {
            bail!(err.message);
        }
        Ok((res.into_body(), tx))
    }
}

/// Subscription maintenance tasks.
enum SubTask {
    /// Build connections to any partitions of the target stream, performing reconnects as needed.
    BuildConnections,
}
