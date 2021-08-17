//! Client metadata system.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use futures::prelude::*;
use http::{request::Builder, Method};
use prost::Message;
use proto::v1::{
    ClusterMetadata, MetadataChange, MetadataChangeType, MetadataSubSetupResponse, MetadataSubSetupResponseResult, PipelineMetadata, StreamMetadata,
};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

pub use crate::client::pipeline::PipelineSubscription;
pub use crate::client::publisher::PublisherClient;
pub use crate::client::subscriber::{StreamSubscription, SubscriberConfig, SubscriptionStartingPoint};
use crate::client::{
    establish_h2_connection_with_backoff, spawn_connection_builder, ClientCore, ClientEvent, ConnectionMap, ConnectionState,
    PartitionConnectionsSignal, AUTH_HEADER,
};
use crate::common::{deserialize_response_or_error, ClientCreds};

impl ClientCore {
    #[tracing::instrument(level = "debug", skip(self, change))]
    pub(super) fn handle_metadata_change(&mut self, change: MetadataChangeType) {
        // TODO: critical path: finish this.
        match change {
            MetadataChangeType::Reset(full) => self.handle_metadata_change_reset(full),
            MetadataChangeType::StreamUpdated(update) => self.handle_metadata_stream_updated(update),
            MetadataChangeType::StreamRemoved(_name) => (), // TODO: just zero-out partition conns.
            MetadataChangeType::PipelineUpdated(update) => self.handle_metadata_pipeline_updated(update),
            MetadataChangeType::PipelineRemoved(_name) => (), // TODO: just zero-out partition conns.
            MetadataChangeType::PodAdded(_pod) => (),         // No-op at this point. May be used in the future.
            MetadataChangeType::PodRemoved(_pod) => (),       // No-op at this point. May be used in the future.
        }
    }

    /// Handle a pipeline metadata update.
    #[tracing::instrument(level = "debug", skip(self, pipeline))]
    pub(super) fn handle_metadata_pipeline_updated(&mut self, pipeline: PipelineMetadata) {
        self.cluster_metadata
            .get_or_insert_with(Default::default)
            .pipelines
            .insert(pipeline.name.clone(), pipeline);
    }

    /// Handle a stream metadata update.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    pub(super) fn handle_metadata_stream_updated(&mut self, stream: StreamMetadata) {
        Self::update_stream_subscribers_for_stream(
            &self.stream_subscribers,
            &stream,
            &mut self.connections,
            &self.events_tx,
            &self.shutdown_tx,
        );
        Self::update_stream_publishers_for_stream(
            &self.stream_publishers,
            &stream,
            &mut self.connections,
            &self.events_tx,
            &self.shutdown_tx,
        );
        if let Some(cluster_metadata) = &self.cluster_metadata {
            for (_name, pipeline) in cluster_metadata
                .pipelines
                .iter()
                .filter(|(_name, pipeline)| pipeline.source_stream.as_str() == stream.name.as_str())
            {
                Self::update_pipeline_subscribers_for_stream(
                    &self.pipeline_subscribers,
                    pipeline,
                    &stream,
                    &mut self.connections,
                    &self.events_tx,
                    &self.shutdown_tx,
                );
            }
        }
        self.cluster_metadata
            .get_or_insert_with(Default::default)
            .streams
            .insert(stream.name.clone(), stream);
    }

    /// Handle a full metadata reset.
    #[tracing::instrument(level = "debug", skip(self, new_meta))]
    pub(super) fn handle_metadata_change_reset(&mut self, new_meta: ClusterMetadata) {
        new_meta.streams.iter().for_each(|(_name, stream)| {
            Self::update_stream_subscribers_for_stream(
                &self.stream_subscribers,
                stream,
                &mut self.connections,
                &self.events_tx,
                &self.shutdown_tx,
            );
            Self::update_stream_publishers_for_stream(&self.stream_publishers, stream, &mut self.connections, &self.events_tx, &self.shutdown_tx);
        });
        new_meta.pipelines.iter().for_each(|(_name, pipeline)| {
            let stream = match new_meta.streams.get(&pipeline.source_stream) {
                Some(stream) => stream,
                None => return,
            };
            Self::update_pipeline_subscribers_for_stream(
                &self.pipeline_subscribers,
                pipeline,
                stream,
                &mut self.connections,
                &self.events_tx,
                &self.shutdown_tx,
            );
        });
        self.cluster_metadata = Some(new_meta);
    }

    #[tracing::instrument(level = "debug", skip(subscribers, stream, conns, events, shutdown))]
    pub(super) fn update_stream_subscribers_for_stream(
        subscribers: &HashMap<String, HashMap<Uuid, PartitionConnectionsSignal>>, stream: &StreamMetadata, conns: &mut ConnectionMap,
        events: &mpsc::Sender<ClientEvent>, shutdown: &broadcast::Sender<()>,
    ) {
        subscribers
            .iter()
            .filter(|(stream_name, _)| stream_name.as_str() == stream.name.as_str())
            .for_each(|(_, groups)| {
                groups.iter().for_each(|(_, chan)| {
                    let conns = Self::build_new_partition_connections_map(stream, conns, events, shutdown);
                    let _res = chan.send(conns);
                });
            });
    }

    #[tracing::instrument(level = "debug", skip(publishers, stream, conns, events, shutdown))]
    pub(super) fn update_stream_publishers_for_stream(
        publishers: &HashMap<String, HashMap<Uuid, PartitionConnectionsSignal>>, stream: &StreamMetadata, conns: &mut ConnectionMap,
        events: &mpsc::Sender<ClientEvent>, shutdown: &broadcast::Sender<()>,
    ) {
        publishers
            .iter()
            .filter(|(stream_name, _)| stream_name.as_str() == stream.name.as_str())
            .for_each(|(_, groups)| {
                groups.iter().for_each(|(_, chan)| {
                    let conns = Self::build_new_partition_connections_map(stream, conns, events, shutdown);
                    let _res = chan.send(conns);
                });
            });
    }

    #[tracing::instrument(level = "debug", skip(subscribers, stream, conns, events, shutdown))]
    pub(super) fn update_pipeline_subscribers_for_stream(
        subscribers: &HashMap<String, HashMap<Uuid, PartitionConnectionsSignal>>, pipeline: &PipelineMetadata, stream: &StreamMetadata,
        conns: &mut ConnectionMap, events: &mpsc::Sender<ClientEvent>, shutdown: &broadcast::Sender<()>,
    ) {
        subscribers
            .iter()
            .filter(|(pipeline_name, _)| pipeline_name.as_str() == pipeline.name.as_str())
            .for_each(|(_, groups)| {
                groups.iter().for_each(|(_, chan)| {
                    let conns = Self::build_new_partition_connections_map(stream, conns, events, shutdown);
                    let _res = chan.send(conns);
                });
            });
    }

    #[tracing::instrument(level = "debug", skip(stream, conns, events, shutdown))]
    pub(super) fn build_new_partition_connections_map(
        stream: &StreamMetadata, conns: &mut ConnectionMap, events: &mpsc::Sender<ClientEvent>, shutdown: &broadcast::Sender<()>,
    ) -> BTreeMap<u8, (Arc<String>, ConnectionState)> {
        let mut partition_conns = BTreeMap::default();
        stream.partitions.iter().for_each(|partition| {
            if let Some((pod, conn)) = conns.get_key_value(&partition.leader) {
                partition_conns.insert(partition.offset as u8, (pod.clone(), conn.state.clone()));
                return;
            }
            let pod = Arc::new(partition.leader.clone());
            let new_conn = spawn_connection_builder(conns, pod.clone(), events.clone(), shutdown.subscribe());
            partition_conns.insert(partition.offset as u8, (pod, new_conn));
        });
        partition_conns
    }

    /// Spawn a new metadata stream task.
    #[tracing::instrument(level = "debug", skip(self))]
    pub(super) fn spawn_metadata_stream(&mut self) {
        tokio::spawn(task_metadata_stream(
            self.cluster_service_url.clone(),
            self.credentials.clone(),
            self.events_tx.clone(),
            self.shutdown_tx.subscribe(),
        ));
    }
}

/// The task responsible for establishing a cluster metadata connection and streaming in changes.
#[tracing::instrument(level = "debug", skip(target, creds, events, shutdown))]
async fn task_metadata_stream(target: Arc<String>, creds: Arc<ClientCreds>, events: mpsc::Sender<ClientEvent>, shutdown: broadcast::Receiver<()>) {
    if let Err(err) = try_task_metadata_stream(target, creds, events.clone(), shutdown).await {
        tracing::error!(error = ?err, "error from metadata stream task");
        let _res = events.send(ClientEvent::MetadataTaskTerminated).await;
        return;
    }
}

#[tracing::instrument(level = "debug", skip(target, creds, events, shutdown))]
async fn try_task_metadata_stream(
    target: Arc<String>, creds: Arc<ClientCreds>, events: mpsc::Sender<ClientEvent>, mut shutdown: broadcast::Receiver<()>,
) -> Result<()> {
    // Establish H2 connection.
    let h2_res = tokio::select! {
        h2_res = establish_h2_connection_with_backoff(target.clone()) => h2_res,
        _ = shutdown.recv() => return Ok(()), // Client is shutting down. Nothing else to do.
    };
    // Unpack the connection result.
    let h2conn = h2_res.context("could not establish cluster metadata connection")?;
    // We have a live connection, now await an open H2 stream slot.
    let mut h2conn = h2conn.ready().await.context("error opening H2 stream")?;

    // We have a live connection and an open H2 stream slot. Establish the metadata connection.
    let (res, _req) = h2conn
        .send_request(
            Builder::new()
                .method(Method::GET)
                .uri(proto::v1::ENDPOINT_METADATA_SUBSCRIBE)
                .header(AUTH_HEADER, creds.header())
                .body(())
                .context("error building head request")?,
            false,
        )
        .context("error sending request to cluster")?;
    let resp = res.await.context("error awaiting server response")?;

    // Unpack the initial frame, which will be a full metadata payload on success.
    let status = resp.status();
    let mut stream = resp.into_body();
    let data = stream
        .data()
        .await
        .context("non-200 response received and no body returned")?
        .context("error awaiting body after reciving non-200")?;
    let response: MetadataSubSetupResponse = deserialize_response_or_error(status, data)?;
    let full_meta = match response.result {
        Some(MetadataSubSetupResponseResult::Ok(full_meta)) => full_meta,
        Some(MetadataSubSetupResponseResult::Err(err)) => bail!("error from server while setting up metadata stream: {}", err.message),
        _ => bail!("invalid response from server while setting up metadata stream"),
    };
    let _res = events
        .send(ClientEvent::MetadataChange(MetadataChangeType::Reset(full_meta)))
        .await;

    // Stream in subsequent metadata changes on the stream and send to the client.
    let mut shutdown = BroadcastStream::new(shutdown);
    loop {
        tokio::select! {
            frame_opt = stream.data() => {
                let frame = frame_opt.context("metadata stream closed")?.context("error while streaming in metadata changes")?;
                let change = MetadataChange::decode(frame.as_ref()).context("error decoding metadata changeset")?;
                if let Some(change) = change.change {
                    let _res = events.send(ClientEvent::MetadataChange(change)).await;
                }
            },
            _ = shutdown.next() => break,
        }
    }

    Ok(())
}
