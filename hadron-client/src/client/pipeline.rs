//! Pipeline subscriber client.

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::Result;
use futures::prelude::*;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tonic::IntoStreamingRequest;

use crate::client::subscriber::{SubscriptionStream, Task};
use crate::client::Client;
use crate::grpc::stream::pipeline_subscribe_request::Action as PipelineSubscribeRequestAction;
use crate::grpc::stream::{NewEvent, PipelineStageOutput, PipelineSubscribeRequest, PipelineSubscribeResponse};
use crate::handler::PipelineHandler;

type PipelineTask = Task<PipelineSubscribeResponse, PipelineSubscribeRequest>;

impl Client {
    /// Create a new subscription on the target pipeline stage.
    ///
    /// Once created, a subscription maintains individual connections with each partition of the
    /// cluster, and will not shut down in the face of errors; it will reconnect, and resume where
    /// it left off as needed.
    ///
    /// ## Parameters
    /// - `pipeline`: the name of the pipeline to subscribe to.
    /// - `stage`: the name of the pipeline's stage which this client is to process.
    /// - `handler`: the handler to use for processing pipeline stage events.
    pub async fn pipeline(&self, pipeline: &str, stage: &str, handler: Arc<dyn PipelineHandler>) -> Result<PipelineSubscription> {
        let (shutdown, _) = broadcast::channel(1);
        let handle = PipelineSubscriptionTask::new(self.clone(), pipeline.into(), stage.into(), shutdown.clone(), handler).spawn();

        Ok(PipelineSubscription { shutdown, handle })
    }
}

/// A handle to a stream subscription.
pub struct PipelineSubscription {
    shutdown: broadcast::Sender<()>,
    handle: JoinHandle<()>,
}

impl PipelineSubscription {
    /// Cancel this subscription and await its completion.
    pub async fn cancel(self) {
        let _ = self.shutdown.send(());
        if let Err(err) = self.handle.await {
            tracing::error!(error = ?err, "error awaiting subscription shutdown");
        }
    }
}

/// A subscription task which manages all IO, handler invocation, and the subscription protocol overall.
struct PipelineSubscriptionTask {
    client: Client,
    pipeline: String,
    stage: String,
    shutdown: broadcast::Sender<()>,
    handler: Arc<dyn PipelineHandler>,

    active_subs: BTreeSet<u32>,
    tasks_tx: mpsc::Sender<PipelineTask>,
    tasks_rx: ReceiverStream<PipelineTask>,
}

impl PipelineSubscriptionTask {
    /// Create a new instance.
    fn new(client: Client, pipeline: String, stage: String, shutdown: broadcast::Sender<()>, handler: Arc<dyn PipelineHandler>) -> Self {
        let (tasks_tx, tasks_rx) = mpsc::channel(100);
        Self {
            client,
            pipeline,
            stage,
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
        tracing::debug!(%self.pipeline, %self.stage, "starting pipeline subscription");

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

        tracing::debug!(%self.pipeline, %self.stage, "pipeline subscription has shut down");
    }

    #[tracing::instrument(level = "debug", skip(self, event))]
    async fn handle_sub_task(&mut self, event: PipelineTask) {
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
        for (partition, chan) in conns.iter() {
            if self.active_subs.contains(&partition) {
                continue;
            }

            // Prepare and send the initial setup request.
            let (partition, mut chan) = (*partition, chan.clone());
            let (tx, rx) = mpsc::channel::<PipelineSubscribeRequest>(1);
            let mut req = ReceiverStream::new(rx).into_streaming_request();
            let header = self.client.inner.creds.header();
            req.metadata_mut().insert(header.0, header.1);
            let (setup, setup_tx) = (
                PipelineSubscribeRequest {
                    action: Some(PipelineSubscribeRequestAction::StageName(self.stage.clone())),
                    pipeline: self.pipeline.clone(),
                },
                tx.clone(),
            );
            tokio::spawn(async move {
                let _res = setup_tx.send(setup).await;
            });

            // Send the request over to the partition & await a response.
            let inbound = match chan.pipeline_subscribe(req).await {
                Ok(res) => res.into_inner(),
                Err(err) => {
                    tracing::error!(error = ?err, partition, %self.pipeline, %self.stage, "error opening pipeline subscription");
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
    fn handle_subscription_delivery(&mut self, partition: u32, msg: PipelineSubscribeResponse, res_tx: mpsc::Sender<PipelineSubscribeRequest>) {
        tracing::debug!(partition, %self.pipeline, %self.stage, "handling pipeline subscription delivery");
        let (handler, pipeline) = (self.handler.clone(), self.pipeline.clone());
        tokio::spawn(async move {
            let res = handler.handle(msg).await;
            match res {
                Ok(data) => Self::ack(res_tx, pipeline, data).await,
                Err(err) => Self::nack(res_tx, pipeline, err).await,
            }
        });
    }

    /// Respond to the server with an `ack` over the given channel.
    #[tracing::instrument(level = "debug", skip(res_tx, pipeline, data))]
    async fn ack(res_tx: mpsc::Sender<PipelineSubscribeRequest>, pipeline: String, data: NewEvent) {
        let msg = PipelineSubscribeRequest {
            action: Some(PipelineSubscribeRequestAction::Ack(PipelineStageOutput { output: Some(data) })),
            pipeline,
        };
        let _res = res_tx.send(msg).await;
    }

    /// Respond to the server with an `nack` over the given channel.
    #[tracing::instrument(level = "debug", skip(res_tx, pipeline, err))]
    async fn nack(res_tx: mpsc::Sender<PipelineSubscribeRequest>, pipeline: String, err: anyhow::Error) {
        let msg = PipelineSubscribeRequest {
            action: Some(PipelineSubscribeRequestAction::Nack(err.to_string())),
            pipeline,
        };
        let _res = res_tx.send(msg).await;
    }
}
