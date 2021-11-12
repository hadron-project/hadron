use std::sync::Arc;

use anyhow::Result;
use futures::stream::StreamExt;
use kube::api::{Api, ListParams};
use kube::client::Client;
use kube::runtime::watcher::{watcher, Error as WatcherError, Event};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;

use crate::config::Config;
use crate::grpc::StreamPartition;
use hadron_core::crd::{RequiredMetadata, Stream};

const METRIC_STREAMS_WATCHER_ERRORS: &str = "hadron_streams_watcher_errors";

/// A `watch::Sender` of `StreamPartition` metadata.
pub type StreamMetadataTx = watch::Sender<Vec<StreamPartition>>;
/// A `watch::Receiver` of `StreamPartition` metadata.
pub type StreamMetadataRx = watch::Receiver<Vec<StreamPartition>>;

/// A result type used for CR events coming from K8s.
pub type StreamCREventResult = std::result::Result<Event<Stream>, WatcherError>;

/// A K8s event watcher of Stream CRs.
pub struct StreamWatcher {
    /// K8s client.
    client: Client,
    /// Runtime config.
    config: Arc<Config>,
    /// A channel used for triggering graceful shutdown.
    shutdown: BroadcastStream<()>,

    /// Stream metadata channel.
    metadata: StreamMetadataTx,
}

impl StreamWatcher {
    /// Create a new instance.
    pub fn new(client: Client, config: Arc<Config>, shutdown: broadcast::Receiver<()>) -> (Self, StreamMetadataRx) {
        let shutdown = BroadcastStream::new(shutdown);
        metrics::register_counter!(METRIC_STREAMS_WATCHER_ERRORS, metrics::Unit::Count, "k8s watcher errors from the streams watcher");
        let (tx, rx) = watch::channel(vec![]);
        (
            Self {
                client,
                config,
                shutdown,
                metadata: tx,
            },
            rx,
        )
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        let api: Api<Stream> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let params = ListParams {
            field_selector: Some(format!("metadata.name={}", &self.config.stream)),
            ..Default::default()
        };
        let stream = watcher(api, params);
        tokio::pin!(stream);

        tracing::info!("stream CR watcher initialized");
        loop {
            tokio::select! {
                Some(k8s_event_res) = stream.next() => self.handle_k8s_event(k8s_event_res).await,
                _ = self.shutdown.next() => break,
            }
        }

        Ok(())
    }

    /// Handle watcher events coming from K8s.
    #[tracing::instrument(level = "debug", skip(self, res))]
    async fn handle_k8s_event(&mut self, res: StreamCREventResult) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from k8s watch stream");
                metrics::increment_counter!(METRIC_STREAMS_WATCHER_ERRORS);
                let _ = tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(stream) => self.update_partition_info(stream),
            Event::Deleted(_stream) => (),
            Event::Restarted(streams) => {
                tracing::debug!("stream CR watcher restarted");
                if let Some(stream) = streams.into_iter().next() {
                    self.update_partition_info(stream);
                }
            }
        }
    }

    /// Build a new stream partitions info struct and send it to all receivers.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    fn update_partition_info(&mut self, stream: Stream) {
        tracing::debug!("updating stream CR info");
        let data = (0..stream.spec.partitions).fold(vec![], |mut acc, offset| {
            let internal = format!(
                "{}-{}.{}.svc.cluster.local:{}", // TODO: make cluster apex configurable.
                stream.name(),
                offset,
                &self.config.namespace,
                &self.config.client_port,
            );
            // TODO[#62,ingress]: external connection info should be built here when applicable.
            acc.push(StreamPartition {
                partition: offset as u32,
                internal,
                ..Default::default()
            });
            acc
        });
        let _res = self.metadata.send(data);
    }
}
