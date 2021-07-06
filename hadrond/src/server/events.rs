use std::sync::Arc;

use anyhow::Context;
use tokio::sync::mpsc;

use crate::crd::{Pipeline, RequiredMetadata, Stream};
use crate::k8s::events::CrdStateChange;
pub use crate::server::cache::MetadataCache;
use crate::server::pipeline::{PipelineCtl, PipelineCtlMsg};
use crate::server::pipeline_replica::PipelineReplicaCtlMsg;
use crate::server::stream::{StreamCtl, StreamCtlMsg};
use crate::server::stream_replica::StreamReplicaCtlMsg;
use crate::server::{PipelineHandle, Server, StreamHandle};

impl Server {
    /// Handle a system metadata event.
    #[tracing::instrument(level = "trace", skip(self, event))]
    pub(super) async fn handle_event(&mut self, event: CrdStateChange) {
        // TODO[orphanData]: as assignments come and go, we will eventually end up with data on
        // disk which is no longer needed, as the data will have been migrated elsewhere, or its
        // parent objects have been deleted and the data is orphaned. Clean up that data.
        match event {
            CrdStateChange::StreamUpdated(name, stream) => self.handle_event_stream_updated(name, stream).await,
            CrdStateChange::StreamDeleted(name, stream) => self.handle_event_stream_deleted(name, stream).await,
            CrdStateChange::PipelineUpdated(name, pipeline, source_stream) => self.handle_event_pipeline_updated(name, pipeline, source_stream).await,
            CrdStateChange::PipelineDeleted(name, pipeline) => self.handle_event_pipeline_deleted(name, pipeline).await,
            CrdStateChange::TokenUpdated(_name, token) => {
                self.cache.insert_token(token);
            }
            CrdStateChange::TokenDeleted(_name, token) => {
                self.cache.remove_token(token);
            }
        }
    }

    /// Handle an event indicating that a stream object CRD has been updated.
    #[tracing::instrument(level = "trace", skip(self, name, stream))]
    async fn handle_event_stream_updated(&mut self, name: Arc<String>, stream: Arc<Stream>) {
        // Handle stream partition leader updates.
        let stream_handles_opt = self.streams.get_mut(name.as_str());
        let aspired_partition_leader_opt = stream.get_pod_leader_partition(&self.config.pod_name);
        match (stream_handles_opt, aspired_partition_leader_opt) {
            // No stream handles exist & no partition is assigned for leadership. Nothing to do there.
            (None, None) => (),
            // No stream handles exist, but we have a target partition for leadership. Spawn the controller.
            (None, Some(target_partition)) => {
                self.spawn_stream_controller(name.clone(), stream.clone(), target_partition)
                    .await;
            }
            // We have a mapping of handles to stream partition leaders, but no assignment. Here
            // we will simply drop the handles after passing along the stream update.
            (Some(handles), None) => {
                for (_, handle) in handles.iter() {
                    let _ = handle.tx.send(StreamCtlMsg::StreamUpdated(stream.clone())).await;
                }
                self.streams.remove(name.as_str()); // Drops all & triggers shutdown on each.
            }
            // We have a mapping of handles to stream partition leaders, and an active assignment.
            // This will be the most common branch. First we iterate through all handles passing
            // along the update. If we found the target partition, and it was the only one, then done.
            // Else, if there were other partitions, drop them. If the target partition doesn't yet exist,
            // then spawn it.
            (Some(handles), Some(target_partition)) => {
                // Pass along update to all handles.
                let mut found_target = false;
                for (partition, handle) in handles.iter() {
                    let _ = handle.tx.send(StreamCtlMsg::StreamUpdated(stream.clone())).await;
                    if partition == &target_partition {
                        found_target = true;
                    }
                }

                // We did not find target, so drop all other handles & spawn new controller.
                if !found_target {
                    self.streams.remove(name.as_str()); // Drops all & triggers shutdown on each.
                    self.spawn_stream_controller(name.clone(), stream.clone(), target_partition)
                        .await;
                }
                // Else, we did find target. If there are more handles than just the target, then drop
                // the old handles.
                else if handles.len() > 1 {
                    handles.retain(|partition, _| partition == &target_partition);
                }
            }
        }

        // Handle stream replica updates.
        match self.stream_replicas.get_mut(name.as_str()) {
            // If we have active replica handles, then we pass the updated stream object first.
            // Next, we filter out any handles which are no longer scheduled as replicas.
            Some(handles) => {
                for (_, handle) in handles.iter() {
                    let _ = handle.tx.send(StreamReplicaCtlMsg::StreamUpdated(stream.clone())).await;
                }
                let pod_name = self.config.pod_name.clone();
                handles.retain(|partition, _| stream.has_pod_as_partition_replica(&pod_name, *partition));
            }
            None => {
                if let Some(partitions) = stream.scheduled_replica_partitions(&self.config.pod_name) {
                    for _partition in partitions {
                        // self.spawn_stream_replica_controller();
                        // TODO[replication]: spawn stream replica
                    }
                }
            }
        }
    }

    /// Handle an event indicating that a stream object CRD has been deleted.
    #[tracing::instrument(level = "trace", skip(self, name, stream))]
    async fn handle_event_stream_deleted(&mut self, name: Arc<String>, stream: Arc<Stream>) {
        if let Some(handles) = self.streams.remove(&*name) {
            for (_, handle) in handles {
                let _ = handle.tx.send(StreamCtlMsg::StreamDeleted(stream.clone())).await;
            }
        }
        if let Some(handles) = self.stream_replicas.remove(&*name) {
            for (_, handle) in handles {
                let _ = handle.tx.send(StreamReplicaCtlMsg::StreamDeleted(stream.clone())).await;
            }
        }
    }

    /// Handle an event indicating that a pipeline object CRD has been updated.
    #[tracing::instrument(level = "trace", skip(self, name, pipeline))]
    async fn handle_event_pipeline_updated(&mut self, name: Arc<String>, pipeline: Arc<Pipeline>, stream: Arc<Stream>) {
        // Handle pipeline partition leader updates.
        let pipeline_handles_opt = self.pipelines.get_mut(name.as_str());
        let aspired_partition_leader_opt = stream.get_pod_leader_partition(&self.config.pod_name);
        match (pipeline_handles_opt, aspired_partition_leader_opt) {
            // No pipeline handles exist & no partition is assigned for leadership. Nothing to do there.
            (None, None) => (),
            // No pipeline handles exist, but we have a target partition for leadership. Spawn the controller.
            (None, Some(target_partition)) => {
                self.spawn_pipeline_controller(name.clone(), pipeline.clone(), target_partition)
                    .await;
            }
            // We have a mapping of handles to pipeline partition leaders, but no assignment. Here
            // we will simply drop the handles after passing along the pipeline update.
            (Some(handles), None) => {
                for (_, handle) in handles.iter() {
                    let _ = handle
                        .tx
                        .send(PipelineCtlMsg::PipelineUpdated(pipeline.clone(), stream.clone()))
                        .await;
                }
                self.pipelines.remove(name.as_str()); // Drops all & triggers shutdown on each.
            }
            // We have a mapping of handles to pipeline partition leaders, and an active assignment.
            // This will be the most common branch. First we iterate through all handles passing
            // along the update. If we found the target partition, and it was the only one, then done.
            // Else, if there were other partitions, drop them. If the target partition doesn't yet exist,
            // then spawn it.
            (Some(handles), Some(target_partition)) => {
                // Pass along update to all handles.
                let mut found_target = false;
                for (partition, handle) in handles.iter() {
                    let _ = handle
                        .tx
                        .send(PipelineCtlMsg::PipelineUpdated(pipeline.clone(), stream.clone()))
                        .await;
                    if partition == &target_partition {
                        found_target = true;
                    }
                }

                // We did not find target, so drop all other handles & spawn new controller.
                if !found_target {
                    self.pipelines.remove(name.as_str()); // Drops all & triggers shutdown on each.
                    self.spawn_pipeline_controller(name.clone(), pipeline.clone(), target_partition)
                        .await;
                }
                // Else, we did find target. If there are more handles than just the target, then drop
                // the old handles.
                else if handles.len() > 1 {
                    handles.retain(|partition, _| partition == &target_partition);
                }
            }
        }

        // Handle pipeline replica updates.
        match self.pipeline_replicas.get_mut(name.as_str()) {
            // If we have active replica handles, then we pass the updated pipeline object first.
            // Next, we filter out any handles which are no longer scheduled as replicas.
            Some(handles) => {
                for (_, handle) in handles.iter() {
                    let _ = handle
                        .tx
                        .send(PipelineReplicaCtlMsg::PipelineUpdated(pipeline.clone(), stream.clone()))
                        .await;
                }
                let pod_name = self.config.pod_name.clone();
                handles.retain(|partition, _| stream.has_pod_as_partition_replica(&pod_name, *partition));
            }
            None => {
                if let Some(partitions) = stream.scheduled_replica_partitions(&self.config.pod_name) {
                    for _partition in partitions {
                        // self.spawn_pipeline_replica_controller();
                        // TODO[replication]: spawn pipeline replica
                    }
                }
            }
        }
    }

    /// Handle an event indicating that a pipeline object CRD has been deleted.
    #[tracing::instrument(level = "trace", skip(self, name, pipeline))]
    async fn handle_event_pipeline_deleted(&mut self, name: Arc<String>, pipeline: Arc<Pipeline>) {
        if let Some(handles) = self.pipelines.remove(&*name) {
            for (_, handle) in handles {
                let _ = handle.tx.send(PipelineCtlMsg::PipelineDeleted(pipeline.clone())).await;
            }
        }
        if let Some(handles) = self.pipeline_replicas.remove(&*name) {
            for (_, handle) in handles {
                let _ = handle
                    .tx
                    .send(PipelineReplicaCtlMsg::PipelineDeleted(pipeline.clone()))
                    .await;
            }
        }
    }

    /// Spawn a stream controller for the given stream partition.
    #[tracing::instrument(level = "trace", skip(self, name, stream, partition))]
    async fn spawn_stream_controller(&mut self, name: Arc<String>, stream: Arc<Stream>, partition: u8) {
        let (tx, rx) = mpsc::channel(10_000);
        let stream_res = StreamCtl::new(
            self.config.clone(),
            self.db.clone(),
            self.cache.clone(),
            stream.clone(),
            partition,
            self.shutdown_tx.clone(),
            rx,
        )
        .await
        .context("error spawning stream controller");
        let (ctl, offset_signal) = match stream_res {
            Ok((ctl, offset_signal)) => (ctl, offset_signal),
            Err(err) => {
                tracing::error!(
                    error = ?err,
                    "error spawning stream controller for {}/{}",
                    stream.name(),
                    partition,
                );
                let _ = self.shutdown_tx.send(());
                return;
            }
        };
        self.streams
            .entry(name.as_ref().clone())
            .or_insert_with(Default::default)
            .insert(
                partition,
                StreamHandle {
                    stream,
                    partition,
                    tx,
                    last_offset: offset_signal,
                    handle: ctl.spawn(),
                },
            );
    }

    /// Spawn a pipeline controller.
    #[tracing::instrument(level = "trace", skip(self, name, pipeline, partition))]
    async fn spawn_pipeline_controller(&mut self, name: Arc<String>, pipeline: Arc<Pipeline>, partition: u8) {
        // Grab the source stream's offset channel.
        let stream_partition_handle = self
            .streams
            .get(&pipeline.spec.source_stream)
            .and_then(|handles| handles.get(&partition));
        let last_offset = match stream_partition_handle {
            Some(handle) => handle.last_offset.clone(),
            None => {
                tracing::error!(
                    "source stream for pipeline {}/{} not found, this should never happen",
                    pipeline.name(),
                    partition,
                );
                let _ = self.shutdown_tx.send(());
                return;
            }
        };

        // Instantiate the pipeline controller.
        let (tx, rx) = mpsc::channel(10_000);
        let pipeline_res = PipelineCtl::new(
            self.config.clone(),
            self.db.clone(),
            self.cache.clone(),
            pipeline.clone(),
            partition,
            last_offset,
            self.shutdown_tx.clone(),
            tx.clone(),
            rx,
        )
        .await
        .context("error spawning pipeline controller");

        // Handle error cases and spawn the controller.
        let ctl = match pipeline_res {
            Ok(ctl) => ctl,
            Err(err) => {
                tracing::error!(
                    error = ?err,
                    "error spawning pipeline controller for {}/{}",
                    pipeline.name(),
                    partition,
                );
                let _ = self.shutdown_tx.send(());
                return;
            }
        };
        self.pipelines
            .entry(name.as_ref().clone())
            .or_insert_with(Default::default)
            .insert(partition, PipelineHandle { pipeline, partition, tx, handle: ctl.spawn() });
    }
}
