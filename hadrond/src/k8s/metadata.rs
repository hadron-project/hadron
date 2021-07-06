//! Metadata handler for metadata stream requests.

use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::Bytes;
use h2::SendStream;
use http::{Method, StatusCode};
use kube::{Resource, ResourceExt};
use proto::v1::{ClusterMetadata, MetadataSubSetupResponse, MetadataSubSetupResponseResult, PartitionMetadata, PipelineMetadata, StreamMetadata};
use uuid::Uuid;

use crate::crd::Token;
use crate::error::AppError;
use crate::futures::LivenessStreamMetadata;
use crate::k8s::Controller;
use crate::server::H2Channel;
use crate::server::{must_get_token, require_method, send_error};

impl Controller {
    /// Handle a metadata stream request.
    #[tracing::instrument(level = "debug", skip(self, req))]
    pub(super) async fn handle_metadata_request(&mut self, mut req: H2Channel) {
        /* TODO:
        - deliver an initial full payload,
        - poll and handle liveness stream in parent controller.
        - as changes are detected, the K8s controller will pass the event data to a handler here for
          for sending out changeset data,
        */

        // Validate the inbound subscriber request.
        let chan = match self.validate_metadata_request(&mut req) {
            Ok(chan) => chan,
            Err(err) => {
                send_error(&mut req, self.buf.split(), err, |err| MetadataSubSetupResponse {
                    result: Some(MetadataSubSetupResponseResult::Err(err)),
                });
                return;
            }
        };

        // Accumulate a full metadata payload and emit to the subscriber.
        let meta = self.snapshot_cluster_meta();
        let res = MetadataSubSetupResponse {
            result: Some(MetadataSubSetupResponseResult::Ok(meta)),
        };

        // Roll a new ID for the channel & add it to the liveness stream.
        let h2_data_chan = (req.0.into_body(), chan);
        let id = Uuid::new_v4();
        self.liveness_checks
            .insert(id, LivenessStreamMetadata { chan: Some(h2_data_chan), chan_id: id });
    }

    /// Validate the given request and respond with headers upon success.
    #[tracing::instrument(level = "debug", skip(self, req))]
    fn validate_metadata_request(&mut self, req: &mut H2Channel) -> Result<SendStream<Bytes>> {
        require_method(&req.0, Method::POST)?;
        let creds = must_get_token(&req.0, self.config.as_ref())?;
        let _claims = self.must_get_token_claims(&creds.claims.id)?;

        // Respond to subscriber to let them know that the channel is ready for use.
        let mut res = http::Response::new(());
        *res.status_mut() = StatusCode::OK;
        let res_chan = req
            .1
            .send_response(res, false)
            .context("error returning response to metadata subscriber for channel setup")?;
        Ok(res_chan)
    }

    /// Snapshot the current cluster metadata.
    fn snapshot_cluster_meta(&self) -> ClusterMetadata {
        let pods = self.pods.values().filter_map(|pod| pod.pod_dns_name.clone()).collect();
        let streams = self
            .streams
            .values()
            .filter_map(|stream| {
                let partitions = stream.status.as_ref()?.partitions.as_slice();
                let partitions_meta = partitions
                    .iter()
                    .enumerate()
                    .fold(Vec::with_capacity(partitions.len()), |mut acc, (offset, partition)| {
                        let leader = partition
                            .leader()
                            .and_then(|leader| self.pods.get(leader))
                            .and_then(|pod| pod.pod_dns_name.clone())
                            .unwrap_or_default();
                        let replicas = partition
                            .replicas()
                            .map(|replicas| {
                                replicas
                                    .iter()
                                    .fold(Vec::with_capacity(replicas.len()), |mut acc, replica| {
                                        acc.push(
                                            self.pods
                                                .get(replica)
                                                .and_then(|pod| pod.pod_dns_name.clone())
                                                .unwrap_or_default(),
                                        );
                                        acc
                                    })
                            })
                            .unwrap_or_default();
                        acc.push(PartitionMetadata {
                            offset: offset as u32,
                            leader,
                            replicas,
                            schedule_state: partition.schedule_state.to_string(),
                            runtime_state: partition.runtime_state.to_string(),
                        });
                        acc
                    });
                Some((stream.name(), StreamMetadata { name: stream.name(), partitions: partitions_meta }))
            })
            .collect();
        let pipelines = self
            .pipelines
            .values()
            .map(|pipeline| {
                (
                    pipeline.name(),
                    PipelineMetadata {
                        name: pipeline.name(),
                        source_stream: pipeline.spec.source_stream.clone(),
                    },
                )
            })
            .collect();
        ClusterMetadata {
            cluster_name: self.config.cluster.clone(),
            pods,
            streams,
            pipelines,
        }
    }

    /// Get the given token's claims, else return an auth error.
    #[allow(clippy::ptr_arg)]
    fn must_get_token_claims(&self, token_id: &String) -> anyhow::Result<Arc<Token>> {
        match self.tokens.get(token_id).cloned() {
            Some(claims) => Ok(claims),
            None => Err(AppError::UnknownToken.into()),
        }
    }
}
