use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use futures::stream::StreamExt;
use kube::api::{Api, ListParams};
use kube::client::Client;
use kube::runtime::watcher::{watcher, Error as WatcherError, Event};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;

use crate::config::Config;
use crate::database::Database;
use crate::pipeline::{PipelineCtl, PipelineCtlMsg, PipelineHandle};
use hadron_core::crd::Pipeline;

/// All known Pipelines mapped from their name to a handle holding their CR & controller.
pub type PipelinesMap = Arc<ArcSwap<HashMap<Arc<String>, PipelineHandle>>>;

/// A result type used for CR events coming from K8s.
pub type PipelineCREventResult = std::result::Result<Event<Pipeline>, WatcherError>;

/// A K8s event watcher of Pipeline CRs.
pub struct PipelineWatcher {
    /// K8s client.
    client: Client,
    /// Runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,

    /// The watch channel of the stream's current offset.
    stream_signal: watch::Receiver<u64>,
    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,

    /// The atomic map of all spawned pipeline controllers.
    pipelines: PipelinesMap,
    /// Join handles for all spawned pipelines.
    pipeline_handles: HashMap<Arc<String>, JoinHandle<Result<()>>>,
}

impl PipelineWatcher {
    /// Create a new instance.
    pub fn new(client: Client, config: Arc<Config>, db: Database, stream_signal: watch::Receiver<u64>, shutdown_tx: broadcast::Sender<()>) -> (Self, PipelinesMap) {
        let pipelines: PipelinesMap = Default::default();
        (
            Self {
                client,
                config,
                db,
                stream_signal,
                shutdown_tx,
                pipelines: pipelines.clone(),
                pipeline_handles: Default::default(),
            },
            pipelines,
        )
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        let pipelines_api: Api<Pipeline> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let stream = watcher(pipelines_api, ListParams::default());
        tokio::pin!(stream);

        tracing::info!("Pipeline CR watcher initialized");
        let mut shutdown = BroadcastStream::new(self.shutdown_tx.subscribe());
        loop {
            tokio::select! {
                Some(k8s_event_res) = stream.next() => self.handle_k8s_event(k8s_event_res).await,
                _ = shutdown.next() => break,
            }
        }

        Ok(())
    }

    /// Handle watcher events coming from K8s.
    #[tracing::instrument(level = "debug", skip(self, res))]
    async fn handle_k8s_event(&mut self, res: PipelineCREventResult) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from k8s watch stream");
                let _ = tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(pipeline) => self.handle_pipeline_applied(pipeline).await,
            Event::Deleted(pipeline) => {
                // Only process Pipelines for this Stream.
                if pipeline.spec.source_stream != self.config.stream {
                    return;
                }
                let pipeline = Arc::new(pipeline);
                let name = match &pipeline.metadata.name {
                    Some(name) => name,
                    None => return,
                };
                tracing::debug!(%name, "deleting Pipeline CR");
                let orig = self.pipelines.load_full();
                let mut updated = orig.as_ref().clone();
                let old = match updated.remove(name) {
                    Some(old) => old,
                    None => {
                        tracing::debug!(%name, "no spawned Pipeline CR controller found, no-op");
                        return;
                    }
                };
                tracing::debug!(%name, "old Pipeline CR found and deleted");
                self.pipelines.store(Arc::new(updated));
                let _res = old.tx.send(PipelineCtlMsg::PipelineDeleted(pipeline.clone())).await;
                if let Some(handle) = self.pipeline_handles.remove(name) {
                    if let Err(err) = handle.await.context("error joining pipeline controller handle").and_then(|res| res) {
                        tracing::error!(error = ?err, "error shutting down pipeline controller");
                    }
                }
            }
            // Handle K8s watcher stream restart.
            //
            // This handler is a bit more tricky as we can not discard the current state. Instead,
            // we iterate over the new input, pulling in the data from the current map, and then
            // we discard the old data at the end by finding old pipelines and stopping them.
            Event::Restarted(pipelines) => {
                tracing::debug!("Pipeline CR stream restarted");
                let orig = self.pipelines.load();
                let mut new_pipelines = HashMap::new();
                for pipeline in pipelines {
                    // Only process Pipelines for this Stream.
                    if pipeline.spec.source_stream != self.config.stream {
                        continue;
                    }
                    let name = match &pipeline.metadata.name {
                        Some(name) => name,
                        None => continue,
                    };
                    match orig.get_key_value(name) {
                        // Pipeline already exists, so just re-index & pass along the updated model.
                        Some((k, v)) => {
                            let pipeline = Arc::new(pipeline);
                            new_pipelines.insert(
                                k.clone(),
                                PipelineHandle {
                                    pipeline: pipeline.clone(),
                                    tx: v.tx.clone(),
                                },
                            );
                            let _res = v.tx.send(PipelineCtlMsg::PipelineUpdated(pipeline)).await;
                        }
                        None => {
                            let (pipeline_name, pipeline) = (Arc::new(name.to_string()), Arc::new(pipeline));
                            let events_tx = match self.spawn_pipeline_controller(pipeline_name.clone(), pipeline.clone()).await {
                                Some(events_tx) => events_tx,
                                None => continue,
                            };
                            new_pipelines.insert(
                                pipeline_name.clone(),
                                PipelineHandle {
                                    pipeline: pipeline.clone(),
                                    tx: events_tx,
                                },
                            );
                        }
                    };
                }

                // For any old pipelines which are about to be dropped, we need to stop them first.
                for (old_name, old_val) in orig.iter().filter(|(k, _v)| !new_pipelines.contains_key(k.as_ref())) {
                    let _res = old_val.tx.send(PipelineCtlMsg::PipelineDeleted(old_val.pipeline.clone())).await;
                    if let Some(handle) = self.pipeline_handles.remove(old_name) {
                        if let Err(err) = handle.await.context("error joining pipeline controller handle").and_then(|res| res) {
                            tracing::error!(error = ?err, "error shutting down pipeline controller");
                        }
                    }
                }

                tracing::debug!(len = new_pipelines.len(), "new Pipeline CR map created");
                self.pipelines.store(Arc::new(new_pipelines));
            }
        }
    }

    /// Handle a pipeline applied/updated event.
    #[tracing::instrument(level = "debug", skip(self, pipeline))]
    async fn handle_pipeline_applied(&mut self, pipeline: Pipeline) {
        // Only process Pipelines for this Stream.
        if pipeline.spec.source_stream != self.config.stream {
            return;
        }
        let name = match &pipeline.metadata.name {
            Some(name) => name,
            None => return,
        };

        tracing::debug!(%name, "adding new Pipeline CR");
        let orig = self.pipelines.load_full();

        // If pipeline already exists, then simply pass along the updated model.
        if let Some(handle) = orig.get(name) {
            let _res = handle.tx.send(PipelineCtlMsg::PipelineUpdated(Arc::new(pipeline))).await;
            return;
        }

        // Else, spawn new pipeline.
        let mut updated = orig.as_ref().clone();
        let (pipeline_name, pipeline) = (Arc::new(name.to_string()), Arc::new(pipeline));
        let events_tx = match self.spawn_pipeline_controller(pipeline_name.clone(), pipeline.clone()).await {
            Some(events_tx) => events_tx,
            None => return,
        };

        updated.insert(
            pipeline_name.clone(),
            PipelineHandle {
                pipeline: pipeline.clone(),
                tx: events_tx,
            },
        );
        self.pipelines.store(Arc::new(updated));
    }

    /// Spawn a pipeline controller.
    async fn spawn_pipeline_controller(&mut self, pipeline_name: Arc<String>, pipeline: Arc<Pipeline>) -> Option<mpsc::Sender<PipelineCtlMsg>> {
        let (events_tx, events_rx) = mpsc::channel(1000);
        let pipeline_ctl_res = PipelineCtl::new(
            self.config.clone(),
            self.db.clone(),
            pipeline,
            self.config.partition,
            self.stream_signal.clone(),
            self.shutdown_tx.clone(),
            events_tx.clone(),
            events_rx,
        )
        .await;
        let pipeline_ctl = match pipeline_ctl_res {
            Ok(pipeline_ctl) => pipeline_ctl,
            Err(err) => {
                tracing::error!(error = ?err, "error spawning pipeline controller, shutting down");
                let _res = self.shutdown_tx.send(());
                return None;
            }
        };
        let pipeline_handle = pipeline_ctl.spawn();
        self.pipeline_handles.insert(pipeline_name, pipeline_handle);
        Some(events_tx)
    }
}
