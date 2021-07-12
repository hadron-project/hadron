//! Hadron scheduler logic for assigning Hadron cluster objects to pods.
//!
//! ## Overview
//! There are a few nice properties we have available to work with here, and they are good to
//! keep in mind, as many of these properties are fundamental to the design of the scheduling
//! algorithm overall.
//!
//! - We leverage K8s [Server-Side Apply](https://kubernetes.io/docs/reference/using-api/server-side-apply/) (SSA).
//! What does this mean, what does this give us? K8s will reject a request to update a resource if
//! the resource presented is not the most up-to-date version known to the K8s API. This guards
//! against race conditions, as well as conditions where we might simply not have the latest info
//! form the K8s cluster for whatever reasons.
//! - Our workflow is uni-directional. The K8s controller — of which this scheduler is a part —
//! uses K8s watchers to observe a live stream of data of all pertinent Hadron cluster data. The
//! scheduler reacts to that data, and only makes updates according to that stream of data. Due to
//! the properties of SSA described above, we are able to confidently publish updates to a resource
//! and as long as the K8s API accepts the update, we know that our updates were the most up-to-date.

/*
### Scheduling Data Requirements
- table showing number of leader assignments per pod,
- table showing number of replica assignments per pod,

### Assign New Partitions
- find a viable pod to act as leader for a new partition,
    - no label selectors, affinit, anti-affiniti yet,
    - select a pod which has least leader assignments,
    - select a pod which is healthy & ready,
- find viable pods to act as replicas,
    - no label selectors, affinit, anti-affiniti yet,
    - select a pod which has least replica assignments,
    - select a pod which is healthy & ready,
- partition slots should always be filled,
    - if partition leaders & replicas can not be fully assigned, then go
    into an unschedulable state, and record this info in memory and k8s,

### Pod Updates
- when new healthy and ready, check for unschedulable objects and schedule,
- when pods become unhealthy, cordon the pod, add to in-mem list of cordoned pods,
and begin moving leaders and replicas off of the pod,
- when pod is deleted, same as cordon,
- when pod lists restart, we need to cross-reference all assignments with
pods, as if pods have been deleted or are unhealthy, we need to perform
standard pod checks,
*/

use std::cmp::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::api::core::v1::Secret;
use kube::api::{Api, Patch, PatchParams};
use kube::Resource;
use tokio::time::timeout;

use crate::auth::TokenClaims;
use crate::crd::RequiredMetadata;
use crate::crd::{Partition, Pipeline, RuntimeState, ScheduleState, Stream, Token};
use crate::k8s::coordination::LeaderState;
use crate::k8s::{Controller, PodInfo};

/// The pod `Running` lifecycle phase.
const POD_RUNNING: &str = "Running";
/// The secret key used for storing a generated JWT.
const SECRET_KEY_TOKEN: &str = "token";
/// The default timeout to use for API calls.
const API_TIMEOUT: Duration = Duration::from_secs(5);

/// A scheduling task to be performed.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // Arcs vs PodInfo.
pub(super) enum SchedulerTask {
    PipelineUpdated(Arc<String>),
    PipelineDeleted(Arc<String>, Arc<Pipeline>),
    PodUpdated(Arc<String>),
    PodDeleted(Arc<String>, PodInfo),
    StreamUpdated(Arc<String>),
    StreamDeleted(Arc<String>, Arc<Stream>),
    TokenUpdated(Arc<String>),
    TokenDeleted(Arc<String>, Arc<Token>),
    SecretUpdated(Arc<String>),
    SecretDeleted(Arc<String>, Arc<Secret>),
}

impl Controller {
    pub(super) async fn handle_scheduler_task(&mut self, task: SchedulerTask, state: LeaderState) {
        if !matches!(state, LeaderState::Leading) {
            return;
        }
        match task {
            SchedulerTask::StreamUpdated(name) => self.scheduler_stream_updated(name).await,
            SchedulerTask::StreamDeleted(name, stream) => self.scheduler_stream_deleted(name, stream).await,
            SchedulerTask::PipelineUpdated(name) => self.scheduler_pipeline_updated(name).await,
            SchedulerTask::PipelineDeleted(name, pipeline) => self.scheduler_pipeline_deleted(name, pipeline).await,
            SchedulerTask::TokenUpdated(name) => self.scheduler_token_updated(name).await,
            SchedulerTask::TokenDeleted(name, token) => self.scheduler_token_deleted(name, token).await,
            SchedulerTask::PodUpdated(name) => self.scheduler_pod_updated(name).await,
            SchedulerTask::PodDeleted(name, info) => self.scheduler_pod_deleted(name, info).await,
            SchedulerTask::SecretUpdated(name) => self.scheduler_secret_updated(name).await,
            SchedulerTask::SecretDeleted(name, secret) => self.scheduler_secret_deleted(name, secret).await,
        }
    }

    /// Perform a reconciliation pass over a stream object.
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn scheduler_stream_updated(&mut self, name: Arc<String>) {
        if let Err(err) = self.reconcile_stream(name.clone()).await {
            tracing::error!(error = ?err, stream = name.as_str(), "error during stream reconciliation");
            let tx = self.scheduler_tasks_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let _ = tx.send(SchedulerTask::StreamUpdated(name));
            });
        }
    }

    /// Handle deletion of a stream object.
    ///
    /// ## Objectives
    /// - Clean-up any objects created in relation to the stream (services).
    #[tracing::instrument(level = "debug", skip(self, _name, _stream))]
    async fn scheduler_stream_deleted(&mut self, _name: Arc<String>, _stream: Arc<Stream>) {
        tracing::debug!("handling scheduler task");
    }

    #[tracing::instrument(level = "debug", skip(self, _name))]
    async fn scheduler_pipeline_updated(&mut self, _name: Arc<String>) {
        tracing::debug!("handling scheduler task");
    }

    #[tracing::instrument(level = "debug", skip(self, _name, _pipeline))]
    async fn scheduler_pipeline_deleted(&mut self, _name: Arc<String>, _pipeline: Arc<Pipeline>) {
        tracing::debug!("handling scheduler task");
    }

    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn scheduler_token_updated(&mut self, name: Arc<String>) {
        if self.secrets.get(&name).is_none() {
            if let Err(err) = self.ensure_secret(name.clone()).await {
                tracing::error!(error = ?err, "error while ensuring backing token secret");
                let tx = self.scheduler_tasks_tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let _ = tx.send(SchedulerTask::TokenUpdated(name));
                });
            }
        }
    }

    #[tracing::instrument(level = "debug", skip(self, _name, _token))]
    async fn scheduler_token_deleted(&mut self, _name: Arc<String>, _token: Arc<Token>) {
        tracing::debug!("handling scheduler task");
        // TODO: delete corresponding secret.
    }

    #[tracing::instrument(level = "debug", skip(self, _name))]
    async fn scheduler_secret_updated(&mut self, _name: Arc<String>) {
        tracing::debug!("handling scheduler task");
        // TODO: ensure the secret has the needed key & is a valid Hadron secret, else re-mint.
        // TODO: if token does not exist, then delete the secret.
    }

    #[tracing::instrument(level = "debug", skip(self, _name, _secret))]
    async fn scheduler_secret_deleted(&mut self, _name: Arc<String>, _secret: Arc<Secret>) {
        tracing::debug!("handling scheduler task");
        // TODO: recrete the secret if the token still exists.
    }

    /// Perform scheduling reconciliation for an updated pod.
    /// ### Healthy Pod
    /// - check for any streams which need reconciliation & emit scheduler tasks to reconcile them.
    ///
    /// ### Unhealthy Pod
    /// - reschedule objects on the pod.
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn scheduler_pod_updated(&mut self, name: Arc<String>) {
        tracing::debug!("handling scheduler task");
        let status_opt = self
            .pods
            .get(&name)
            .and_then(|info| info.pod.as_ref())
            .and_then(|pod| pod.status.as_ref());
        let status = match status_opt {
            Some(spec) => spec,
            None => return,
        };
        let is_running = status
            .phase
            .as_ref()
            .map(|phase| phase.as_str() == POD_RUNNING)
            .unwrap_or(false);
        if is_running {
            // If pod is healthy, then reconcile over any streams which need scheduling or which
            // have an unstable runtime state.
            let streams_to_reconcile =
                self.streams
                    .iter()
                    .filter_map(|(name, stream)| stream.status.as_ref().map(|status| (name, stream, status)))
                    .filter(|(_, _, status)| {
                        status.partitions.iter().any(|prtn| {
                            !matches!(prtn.schedule_state, ScheduleState::Scheduled) || !matches!(prtn.runtime_state, RuntimeState::Stable)
                        })
                    });
            for stream in streams_to_reconcile {
                let _ = self
                    .scheduler_tasks_tx
                    .send(SchedulerTask::StreamUpdated(stream.0.clone()));
            }
        }

        // TODO: handle unhealthy pod cases.
    }

    /// Handle pod deletions.
    ///
    /// ## Objectives
    /// - Find any streams with partitions which were scheduled on this pod, mark the partitions
    /// as unstable, and attempt to move workloads to healthy pods.
    #[tracing::instrument(level = "debug", skip(self, _name, _pod))]
    async fn scheduler_pod_deleted(&mut self, _name: Arc<String>, _pod: PodInfo) {
        tracing::debug!("handling scheduler task");
        // TODO: handle unhealthy pod cases.
    }
}

/// Reconciliation tasks.
impl Controller {
    /// Perform a reconciliation pass over the target stream.
    ///
    /// ## Objectives
    /// - Attempt to reschedule any partitions which were unschedulable.
    /// - Add new partitions if the current partition count is less than the target count.
    /// - Update replica counts if the target replica count has changed.
    /// - Check for any partitions with an unstable runtime state and attempt to move workloads.
    /// - Handle partition deletion protocol.
    ///
    /// ### Adding Partitions
    /// - Select a new pod as partition leader. Selection criteria will be as follows:
    ///     - Pod must not be currently operating as a partition leader for another partition of
    ///       the subject stream.
    ///     - Pod selection should favor pods with fewer leadership assignments.
    ///     - Stream affinity rules and label selector rules must also match pod's metadata.
    /// - Select a set of pods as replicas. Selectio criteria will be as follows:
    ///     - Pod must neither be the partition's leader nor a replica of the partition.
    ///     - Pod selection should favor pods with fewer replica assignments.
    ///     - Stream affinity rules and label selector rules must also match pod's metadata.
    ///
    /// ### Deleting Partitions
    /// - Scaling down partitions will mark partitions for removal, but will not actually delete
    ///   the partition. A user of the system must acknowledge the removal, which will then allow
    ///   the partition to be deleted. Once the approval has gone through, the deletion will commence.
    /// - The partition will be removed from the Stream record and the corresponding pods will
    ///   respond to this update by dropping their corresponding controllers and purging its data.
    /// - Partitions which are in an unschedulable state can be safely deleted as long as they do
    ///   not have a successfully scheduled partition at a subsequent ordinal.
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn reconcile_stream(&mut self, name: Arc<String>) -> Result<()> {
        tracing::debug!(%name, "reconciling stream");
        let stream = match self.streams.get(&name) {
            Some(stream) => stream.clone(),
            None => return Ok(()),
        };
        let mut updated_stream: Option<Stream> = None; // Only accumulate changes when needed.
        self.pods_scratch.clear(); // Ensure scratch pad is in a clean state for scheduling pass.

        // Check stream partitions for anything which is unschedulable, and do a pass to
        // see if scheduling is possible.
        if let Some(status) = stream.status.as_ref() {
            let has_unscheduled_partitions = status.partitions.iter().any(|prtn| !prtn.is_scheduled());
            if has_unscheduled_partitions {
                let updated_stream_ref = updated_stream.get_or_insert_with(|| stream.as_ref().clone());
                self.reconcile_stream_schedule_partitions(updated_stream_ref, &name);
            }
        }

        // Check replica counts per partition, and add or remove any as needed.
        if let Some(status) = stream.status.as_ref() {
            let needs_replica_balancing = status
                .partitions
                .iter()
                .filter(|prtn| prtn.is_scheduled())
                .map(|prtn| prtn.replicas().map(|repls| repls.len()).unwrap_or(0))
                .any(|repl_count| repl_count != stream.spec.replicas as usize);
            if needs_replica_balancing {
                let updated_stream_ref = updated_stream.get_or_insert_with(|| stream.as_ref().clone());
                self.reconcile_stream_add_or_remove_replicas(updated_stream_ref, &name);
            }
        }

        // If the stream's target partition count is greater than the current number of partitions,
        // then schedule new partitions.
        let active_partitions = stream.status.as_ref().map(|status| status.partitions.len()).unwrap_or(0);
        if active_partitions < stream.spec.partitions as usize {
            let updated_stream_ref = updated_stream.get_or_insert_with(|| stream.as_ref().clone());
            self.reconcile_stream_add_partitions(updated_stream_ref, &name);
        }

        // For any partitions with an unstable or unknown runtime state,
        // check backing pods & rebalance as needed.
        /* TODO: handle unstable runtime state.
        - leader can only move to a pod which was a replica, else data loss. If leader had no
          replicas, then it is not safe to move the leader.
        - leaders can make updates to their partition info to indicate the replication state of new
          replicas. Once a replica is "synced", it will no longer fall out of sync (as all data is
          fully replicated to all replicas & entries are not considered committed until all replicas
          have a copy).
        - the scheduler can use the info written by the partition leader to determine which replicas
          are actually viable for partition leadership under failure conditions.
        - replicas & leaders should not be moved when a pod deletion is detected, unless the pod
          is no longer covered by its statefulset's replicas count.
        - hadron should also watch statefulsets in the cluster which match the cluster name. This
          data will be used to help determine when workloads should actually be moved off of pods
          and only pods which will not come back due to statefulset replica count being too low
          should be moved. Temporary pod failures and deletions (which may just be updates) should
          be ignored.
        - temporarily unhealthy pods should not cause scheduling changes.
        */

        // TODO: handle deleting partitions.

        // If we have accumulated updates to the stream, then update K8s before we finish.
        match updated_stream {
            None => (),
            Some(updated_stream) if &updated_stream == stream.as_ref() => (),
            Some(mut updated_stream) => {
                updated_stream.status.get_or_insert_with(Default::default).epoch += 1;
                let updated_stream = self
                    .patch_stream_cr(updated_stream)
                    .await
                    .context("error patching stream for updated partitions")?;
                tracing::debug!(stream = updated_stream.name(), "stream partitions successfully patched");
                self.stream_applied(updated_stream).await;
            }
        }

        self.pods_scratch.clear(); // Ensure bookkeeping scratch pad is clear.
        Ok(())
    }

    /// Add or remove partition replicas as needed.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    fn reconcile_stream_add_or_remove_replicas(&mut self, stream: &mut Stream, name: &Arc<String>) {
        let status = match stream.status.as_mut() {
            Some(status) => status,
            None => return,
        };
        let target_repl_count = stream.spec.replicas as usize;
        status
            .partitions
            .iter_mut()
            .filter(|prtn| prtn.is_scheduled())
            .for_each(|prtn| {
                let repls: usize = prtn.replicas().map(|repls| repls.len()).unwrap_or(0);
                match repls.cmp(&target_repl_count) {
                    // If we need to add replicas, then select pods which are neither leader
                    // of this partition, nor one of its current replicas.
                    Ordering::Less => {
                        for _ in repls..target_repl_count {
                            let pod_opt = self
                                .find_available_pod(|| prtn.replicas().unwrap_or(&[]).iter().chain(prtn.leader()))
                                .cloned();
                            if let Some(pod) = pod_opt {
                                let pod_hash = Self::hash_name(pod.as_str());
                                prtn.add_replica(pod);
                                let pod_info = self.pods_scratch.entry(pod_hash).or_insert_with(Default::default);
                                *pod_info.stream_replicas.entry(name.clone()).or_insert(0) += 1;
                            }
                        }
                        if prtn.replicas().map(|repls| repls.len()).unwrap_or(0) == target_repl_count {
                            prtn.schedule_state = ScheduleState::Scheduled;
                        } else {
                            prtn.schedule_state = ScheduleState::NeedsReplicas;
                        }
                    }
                    // Else, if we need to remove replicas, then simply pop from then end until we're
                    // down to the target count.
                    Ordering::Greater => {
                        for _ in target_repl_count..repls {
                            if let Some(pod) = prtn.pop_replica() {
                                let pod_hash = Self::hash_name(pod.as_str());
                                let pod_info = self.pods_scratch.entry(pod_hash).or_insert_with(Default::default);
                                let count = pod_info.stream_replicas.entry(name.clone()).or_insert(1);
                                if let Some(new_count) = count.checked_sub(1) {
                                    *count = new_count;
                                }
                            }
                        }
                        if prtn.replicas().map(|repls| repls.len()).unwrap_or(0) == target_repl_count {
                            prtn.schedule_state = ScheduleState::Scheduled;
                        } else {
                            prtn.schedule_state = ScheduleState::NeedsReplicas;
                        }
                    }
                    // Else, no-op.
                    Ordering::Equal => (),
                }
            });
    }

    /// Add new stream partitions as part of a reconciliation pass.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    fn reconcile_stream_add_partitions(&mut self, stream: &mut Stream, name: &Arc<String>) {
        let active_partitions = stream.status.as_ref().map(|status| status.partitions.len()).unwrap_or(0);
        for _ in (active_partitions as u8)..stream.spec.partitions {
            let partition = self.assign_stream_partition(&stream, &[]);
            tracing::debug!(name = stream.name(), ?partition, "partition assigned");
            self.update_pod_scratch_from_partition(&partition, name.clone());
            stream
                .status
                .get_or_insert_with(Default::default)
                .partitions
                .push(partition);
        }
    }

    /// Update any unscheduled partitions.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    fn reconcile_stream_schedule_partitions(&mut self, stream: &mut Stream, name: &Arc<String>) {
        // TODO[bug]: don't schedule a partition if the stream calls for fewer partitions.
        // If the partition is unscheduled and can be safely removed, then remove it.
        let offsets = stream
            .status
            .as_ref()
            .map(|status| status.partitions.as_slice())
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .filter(|(_, prtn)| !prtn.is_scheduled())
            .map(|(offset, _)| offset)
            .collect::<Vec<_>>();
        for offset in offsets {
            let partition = self.assign_stream_partition(&stream, &[]);
            self.update_pod_scratch_from_partition(&partition, name.clone());
            tracing::debug!(name = stream.name(), ?partition, "partition assigned");
            if let Some(status) = stream.status.as_mut() {
                status.partitions.remove(offset);
                status.partitions.insert(offset, partition);
            }
        }
    }
}

/// K8s API interaction.
impl Controller {
    /// Ensure the given secret exists.
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn ensure_secret(&mut self, name: Arc<String>) -> Result<Secret> {
        // Attempt to fetch the target secret.
        let api: Api<Secret> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let secret_opt = timeout(API_TIMEOUT, api.get(name.as_str()))
            .await
            .context("timeout while fetching secret")?
            .map(Some)
            .or_else(|err| match err {
                // If the object was not found, then treat it as an Option::None.
                kube::Error::Api(err) if err.code == http::StatusCode::NOT_FOUND => Ok(None),
                _ => Err(err),
            })
            .context("error fetching secret")?;
        if let Some(secret) = secret_opt {
            return Ok(secret);
        }

        // Secret does not exist. Mint a new JWT & create the backing secret.
        let claims = TokenClaims::new(&self.config.cluster);
        let jwt = claims.encode(&self.config).context("error encoding claims as JWT")?;
        let mut secret = Secret::default();
        secret
            .string_data
            .get_or_insert_with(Default::default)
            .insert(SECRET_KEY_TOKEN.into(), jwt);
        secret.meta_mut().name = Some(name.as_ref().clone());
        secret.meta_mut().namespace = Some(self.config.namespace.clone());
        let labels = secret.meta_mut().labels.get_or_insert_with(Default::default);
        labels.insert("hadron.rs/cluster".into(), self.config.cluster.clone());
        labels.insert("app".into(), "hadron".into());
        let params = kube::api::PostParams::default();
        timeout(API_TIMEOUT, api.create(&params, &secret))
            .await
            .context("timeout while creating backing secret for token")?
            .context("error creating backing secret for token")
    }

    /// Ensure we have ownership of the lease, else return an error.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn fence(&mut self) -> Result<()> {
        let api: Api<Lease> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let lease = timeout(API_TIMEOUT, api.get(&self.lease_name))
            .await
            .context("timeout while fetching lease")?
            .context("error updating lease")?;
        let is_lease_holder = lease
            .spec
            .as_ref()
            .and_then(|spec| {
                spec.holder_identity
                    .as_deref()
                    .map(|holder_id| holder_id == self.config.pod_name.as_str())
            })
            .unwrap_or(false);
        if !is_lease_holder {
            bail!("lease is no longer held by this pod");
        }
        Ok(())
    }

    /// Patch the given stream in K8s using Server-Side Apply.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    async fn patch_stream_cr(&mut self, mut stream: Stream) -> Result<Stream> {
        self.fence().await?; // Ensure we still hold the lease.
        let api: Api<Stream> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let mut params = PatchParams::apply(&self.config.pod_name);
        params.force = true; // This will still be blocked by the server if we do not have the most up-to-date object info.
        stream.metadata.managed_fields = None;
        let stream = timeout(API_TIMEOUT, api.patch_status(stream.name(), &params, &Patch::Apply(&stream)))
            .await
            .context("timeout while updating stream")?
            .context("error updating stream")?;
        Ok(stream)
    }
}

/// Scheduler utility methods.
impl Controller {
    /// Assign a new partition for the given stream.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    fn assign_stream_partition(&mut self, stream: &Stream, new_partitions: &[Partition]) -> Partition {
        // Find a new pod to be leader.
        tracing::debug!(name = stream.name(), "selecting new partition leader");
        let partitions = stream
            .status
            .as_ref()
            .map(|status| status.partitions.as_slice())
            .unwrap_or(&[]);

        let mut blacklist: Vec<&String> = vec![];
        blacklist.extend(partitions.iter().map(|prtn| prtn.leader()).flatten());
        blacklist.extend(new_partitions.iter().map(|prtn| prtn.leader()).flatten());

        let new_leader = match self.find_available_pod(|| blacklist.iter().copied()).cloned() {
            Some(new_leader) => new_leader,
            None => return Partition::new(ScheduleState::Unschedulable, RuntimeState::Unknown, None, None),
        };
        tracing::debug!(name = stream.name(), %new_leader, "new partition leader selected");

        // As long as we were able to schedule a new leader, then assign the needed set of replicas.
        // Our blacklist filter here only excludes this new partition's leader (just selected above),
        // and excludes any pods already selected as replicas for this new partition.
        blacklist.clear();
        blacklist.push(&new_leader);
        let mut new_partition_replicas = Vec::with_capacity(stream.spec.replicas as usize);
        for _ in 0..stream.spec.replicas as usize {
            tracing::debug!(name = stream.name(), "selecting new partition replica");

            let replica_opt = self.find_available_pod(|| blacklist.iter().copied().chain(new_partition_replicas.iter()));
            let replica = match replica_opt.cloned() {
                Some(replica) => replica,
                None => return Partition::new(ScheduleState::Unschedulable, RuntimeState::Unknown, None, None),
            };
            tracing::debug!(name = stream.name(), %replica, "new partition replica found");
            new_partition_replicas.push(replica);
        }

        Partition::new(
            ScheduleState::Scheduled,
            RuntimeState::Stable,
            Some(new_leader),
            Some(new_partition_replicas),
        )
    }

    /// Find an available pod not in the given blacklist, which matches the given affinity and
    /// label selectors, and which has the fewest leadership assignments.
    #[tracing::instrument(level = "debug", skip(self, blacklist))]
    fn find_available_pod<'b, 'a: 'b, F, I>(&'a self, blacklist: F) -> Option<&'b String>
    where
        F: Fn() -> I,
        I: Iterator<Item = &'b String>,
    {
        self.pods
            .iter()
            // Filter out pods in the blacklist.
            .filter(|(name, _pod)| !blacklist().any(|blk| name.as_str() == blk.as_str()))
            // Filter out pods which are not currently healthy.
            .filter(|(_name, pod)| {
                pod.pod.as_ref()
                    .and_then(|pod| pod.status.as_ref())
                    .and_then(|status| status.phase.as_ref())
                    .map(|phase| phase == POD_RUNNING)
                    .unwrap_or(false)
            })
            // Select a pod with the least assignments.
            .fold(None, |acc, (name, pod)| -> Option<(u64, &Arc<String>, &PodInfo)> {
                // Here we sum all assignments, and then compare against `acc`. If the item has
                // fewer assignments, we take it.
                let pod_hash = Self::hash_name(name.as_str());
                let (in_flight_leader_total, in_flight_repl_total) = self.pods_scratch.get(&pod_hash).map(|pod| {
                    (pod.stream_leaders.len() as u64, pod.stream_replicas.values().sum())
                }).unwrap_or((0, 0));
                let leader_total = pod.stream_leaders.len() as u64;
                let repl_total: u64 = pod.stream_replicas.values().sum();
                let total = leader_total + repl_total + in_flight_leader_total + in_flight_repl_total;
                match acc {
                    Some((acc_total, _, _)) if acc_total < total => acc,
                    _ => Some((total, name, pod)),
                }
            })
            .map(|(_, name, _pod)| name.as_ref())
    }

    /// Hash the given str.
    fn hash_name(name: &str) -> u64 {
        seahash::hash(name.as_bytes())
    }

    /// Update pod_scratch bookkeeping for an in-flight partition assignment which has not yet
    /// been committed to K8s.
    fn update_pod_scratch_from_partition(&mut self, prtn: &Partition, stream_name: Arc<String>) {
        if !prtn.is_scheduled() {
            return;
        }
        if let Some(leader) = prtn.leader() {
            let pod_hash = Self::hash_name(leader.as_str());
            let pod = self.pods_scratch.entry(pod_hash).or_insert_with(Default::default);
            pod.stream_leaders.insert(stream_name.clone());
        }
        if let Some(repls) = prtn.replicas() {
            repls.iter().for_each(|repl| {
                let pod_hash = Self::hash_name(repl.as_str());
                let pod = self.pods_scratch.entry(pod_hash).or_insert_with(Default::default);
                *pod.stream_replicas.entry(stream_name.clone()).or_insert(0) += 1;
            })
        }
    }
}
