//! Coordination utilities built around the `coordination.k8s.io/v1` API.
//!
//! This implementation is based upon the upstream Kubernetes implementation in Go which can be
//! found here: https://github.com/kubernetes/client-go/blob/master/tools/leaderelection/leaderelection.go
//!
//! The `coordination.k8s.io/v1` API does not guarantee that only one client is acting as leader.
//! However, before taking leadership actions, clients can use the `check_lease` function to
//! ensure that the lease is still held. As long as all clients behave according to this protocol,
//! then a strong degree of fencing can be achieved.

use anyhow::{ensure, Context, Result};
use chrono::{prelude::*, Duration};
use futures::prelude::*;
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::MicroTime;
use kube::api::{Api, ListParams, Patch, PatchParams};
use kube::client::Client;
use kube::runtime::{
    watcher,
    watcher::{Event, Result as WatcherResult},
};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_stream::wrappers::BroadcastStream;

type DateTimeUtc = DateTime<Utc>;

const JITTER_FACTOR: f64 = 1.2;

const METRIC_IS_LEADER: &str = "hadron_operator_is_leader";
const METRIC_LEADERSHIP_CHANGE: &str = "hadron_operator_num_leadership_changes";

/// Different states which a leader elector may be in.
#[derive(Clone, Debug, PartialEq)]
pub enum LeaderState {
    /// This client instance is the leader.
    Leading,
    /// A state indicating that a different client is currently the leader, identified by the
    /// encapsulated string.
    ///
    /// When a new leader is detected, this value will be updated with the leader's identity.
    Following(String),
    /// A state indicating that the lease state is unknown, does not exist, or that the
    /// corresponding leader elector task is starting or stopping.
    Standby,
}

/// Configuration for leader election.
pub struct LeaderElectionConfig {
    /// The name of the lease object.
    name: String,
    /// The namespace of the lease object.
    namespace: String,
    /// The identity to use when the lease is acquired.
    identity: String,
    /// The duration that non-leader candidates will wait to force acquire leadership.
    /// This is measured against time of last observed ack.
    ///
    /// A client needs to wait a full `lease_duration` without observing a change to
    /// the record before it can attempt to take over. When all clients are
    /// shutdown and a new set of clients are started with different names against
    /// the same leader record, they must wait the full `lease_duration` before
    /// attempting to acquire the lease. Thus `lease_duration` should be as short as
    /// possible (within your tolerance for clock skew rate) to avoid a possible
    /// long waits in such a scenario.
    ///
    /// Core clients default this value to 15 seconds.
    lease_duration: Duration,
    /// The duration that the current lease holder will retry refreshing the lease.
    ///
    /// Core clients default this value to 10 seconds.
    renew_deadline: Duration,
    /// The duration which leader elector clients should wait between tries of actions.
    ///
    /// Core clients default this value to 2 seconds.
    #[allow(dead_code)]
    retry_period: Duration,
}

impl LeaderElectionConfig {
    // Create a new `LeaderElectionConfig` instance, validating given inputs.
    pub fn new(namespace: impl AsRef<str>, name: impl AsRef<str>, identity: String, lease_duration: Duration, renew_deadline: Duration, retry_period: Duration) -> Result<Self> {
        ensure!(lease_duration > renew_deadline, "lease_duration must be greater than renew_deadline");
        ensure!(
            renew_deadline > Duration::seconds((JITTER_FACTOR * retry_period.num_seconds() as f64) as i64),
            "renew_deadline must be greater than retry_period*{}",
            JITTER_FACTOR,
        );
        ensure!(lease_duration.num_seconds() >= 1, "lease_duration must be at least 1 second");
        ensure!(renew_deadline.num_seconds() >= 1, "renew_deadline must be at least 1 second");
        ensure!(retry_period.num_seconds() >= 1, "retry_period must be at least 1 second");
        Ok(Self {
            name: name.as_ref().to_string(),
            namespace: namespace.as_ref().to_string(),
            identity,
            lease_duration,
            renew_deadline,
            retry_period,
        })
    }
}

/// A task which is responsible for acquiring and maintaining a `coordination.k8s.io/v1` `Lease`
/// to establish leadership.
pub struct LeaderElector {
    /// An K8s API wrapper around the client.
    api: Api<Lease>,
    /// The name to use for managing lease fields for Server-Side Apply.
    manager: String,
    /// Leader election config.
    config: LeaderElectionConfig,
    /// Sender for the current state of the leadership coordination system.
    state_tx: watch::Sender<LeaderState>,
    /// The last known leader state.
    state: LeaderState,
    /// A broadcast channel used to trigger task shutdown.
    shutdown: BroadcastStream<()>,

    /// The last observed renew time recorded in the lease.
    last_observed_lease: Lease,
    /// The last time when a change was observed on the lease.
    last_observed_change: DateTimeUtc,
}

impl LeaderElector {
    // Create a new `LeaderElector` instance.
    pub fn new(lease: Lease, config: LeaderElectionConfig, manager: impl AsRef<str>, client: Client, shutdown: broadcast::Receiver<()>) -> (Self, watch::Receiver<LeaderState>) {
        metrics::register_gauge!(METRIC_LEADERSHIP_CHANGE, metrics::Unit::Count, "the number of leadership changes in the operator consensus group");
        metrics::register_gauge!(
            METRIC_IS_LEADER,
            metrics::Unit::Count,
            "a gauge indicating if this node is the leader, where 1.0 indicates leadership, any other value does not"
        );
        let (state_tx, state_rx) = watch::channel(LeaderState::Standby);
        (
            LeaderElector {
                api: kube::Api::namespaced(client, &config.namespace),
                last_observed_lease: lease,
                last_observed_change: Utc::now(),
                config,
                manager: manager.as_ref().to_string(),
                state_tx,
                state: LeaderState::Standby,
                shutdown: BroadcastStream::new(shutdown),
            },
            state_rx,
        )
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        tracing::info!("leader elector task started");

        // Perform an initail pass at acquiring / renewing the lease.
        if let Err(err) = self.try_acquire_or_renew().await {
            tracing::error!(error = ?err, "error attempting to acquire/renew lease");
        }
        tracing::info!("finished initial call to try_acquire_or_renew");

        let lease_watcher = watcher(
            self.api.clone(),
            ListParams {
                field_selector: Some(format!("metadata.name={}", self.config.name)),
                ..Default::default()
            },
        );
        tokio::pin!(lease_watcher);

        loop {
            let delay_duration = self.get_next_acquire_renew_time();
            tracing::debug!("delaying for {}s", delay_duration.as_secs());
            let delay = tokio::time::sleep(delay_duration);
            tokio::pin!(delay);
            tokio::select! {
                Some(lease_change_res) = lease_watcher.next() => self.handle_lease_watcher_change(lease_change_res).await,
                _ = &mut delay => {
                    tracing::info!("delay elapsed, going to call try_acquire_or_renew");
                    if let Err(err) = self.try_acquire_or_renew().await {
                        tracing::error!(error = ?err, "error during call to try_acquire_or_renew");
                        if !matches!(&self.state, LeaderState::Standby) {
                            self.set_state(LeaderState::Standby);
                        }
                        self.last_observed_change = Utc::now();
                    }
                }
                _ = self.shutdown.next() => break,
            }
        }

        tracing::info!("leader elector task stopped");
    }

    /// Handle a change from the lease watcher.
    #[tracing::instrument(level = "debug", skip(self, res))]
    async fn handle_lease_watcher_change(&mut self, res: WatcherResult<Event<Lease>>) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from lease watcher stream");
                return;
            }
        };
        let lease = match event {
            Event::Applied(lease) => lease,
            _ => return,
        };
        if let Some(Some(transitions)) = lease.spec.as_ref().map(|spec| spec.lease_transitions) {
            metrics::gauge!(METRIC_LEADERSHIP_CHANGE, transitions as f64);
        }
        if lease != self.last_observed_lease {
            tracing::debug!("lease update observed from watcher stream");
            self.last_observed_change = Utc::now();
            self.update_lease_from_api(lease);
        }
    }

    /// Ensure that the target lease exists.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn ensure_lease(&mut self) -> Result<()> {
        tracing::debug!("ensure_lease");
        // Attempt to fetch the target lease, updating our last observed info on the lease.
        let now = Utc::now();
        let get_res = timeout(Self::timeout(), self.api.get(&self.config.name))
            .await
            .context("timeout fetching lease")?
            .context("error fetching lease");
        if let Ok(lease) = get_res {
            if self.last_observed_lease == lease {
                tracing::debug!("found lease, and is identical, no-op");
                return Ok(()); // Nothing to do.
            }

            // Changes in the lease have been detected, update observation info.
            self.last_observed_change = now;
            self.update_lease_from_api(lease);
            return Ok(());
        }

        // Attempt to create the lease if it does not already exist.
        let lease = timeout(Self::timeout(), self.api.create(&Default::default(), &self.last_observed_lease))
            .await
            .context("timeout creating lease")?
            .context("error creating lease")?;
        self.last_observed_change = now;
        self.update_lease_from_api(lease);
        Ok(())
    }

    /// Attempt to acquire or renew the target lease.
    #[tracing::instrument(level = "debug", skip(self), err)]
    async fn try_acquire_or_renew(&mut self) -> Result<()> {
        tracing::debug!("try_acquire_or_renew");
        // 1. Ensure lease exists and update observation info as needed.
        self.ensure_lease().await.context("error ensuring lease exists")?;

        // 2. Determine what type of update needs to be made to the lease. If following a
        // non-expired leader, then we are done here.
        let now = Utc::now();
        let deadline_as_follower = self.last_observed_change + self.config.lease_duration;
        let updated_lease = match &self.state {
            LeaderState::Following(other) if deadline_as_follower > now => {
                tracing::debug!("leadership lease is held by {} and has not yet expired", other);
                return Ok(());
            }
            state => {
                let mut lease = self.last_observed_lease.clone();
                let mut spec = lease.spec.get_or_insert_with(Default::default);
                spec.lease_duration_seconds = Some(self.config.lease_duration.num_seconds() as i32); // i64 as i32 will take only lower bits.
                spec.renew_time = Some(MicroTime(now));
                if !matches!(state, LeaderState::Leading) {
                    spec.holder_identity = Some(self.config.identity.clone());
                    spec.acquire_time = Some(MicroTime(now));
                    spec.lease_transitions = Some(spec.lease_transitions.map(|val| val + 1).unwrap_or(0));
                }
                lease.metadata.managed_fields = None; // Can not pass this along for update.
                lease
            }
        };

        // 3. Now we need to update the lease in K8s with the updated lease value here.
        let mut params = PatchParams::apply(&self.manager);
        params.force = true; // This will still be blocked by the server if we do not have the most up-to-date lease info.
        let lease = timeout(Self::timeout(), self.api.patch(&self.config.name, &params, &Patch::Apply(updated_lease)))
            .await
            .context("timeout while updating lease")?
            .context("error updating lease")?;
        self.last_observed_change = now;
        self.update_lease_from_api(lease);

        Ok(())
    }

    /// Update the lease object as observed from the API.
    ///
    /// This will also handle updating this object's leadership state and will emit
    /// events as needed.
    #[tracing::instrument(level = "debug", skip(self, lease))]
    fn update_lease_from_api(&mut self, lease: Lease) {
        tracing::debug!("update_lease_from_api");
        self.last_observed_lease = lease;
        let holder = self
            .last_observed_lease
            .spec
            .as_ref()
            .map(|spec| {
                if let Some(transitions) = spec.lease_transitions {
                    metrics::gauge!(METRIC_LEADERSHIP_CHANGE, transitions as f64);
                }
                spec.holder_identity.as_deref().unwrap_or_default()
            })
            .unwrap_or_default();
        let lease_is_held = holder == self.config.identity;
        let state_opt = match &self.state {
            LeaderState::Leading if lease_is_held => None,
            LeaderState::Following(id) if id == holder => None,
            LeaderState::Following(_) if lease_is_held => Some(LeaderState::Leading),
            LeaderState::Standby if lease_is_held => Some(LeaderState::Leading),
            LeaderState::Leading | LeaderState::Following(_) | LeaderState::Standby => Some(LeaderState::Following(holder.into())),
        };
        if let Some(state) = state_opt {
            self.set_state(state);
        }
        tracing::debug!("lease updated");
    }

    /// Get the duration to delay before attempting the next lease update.
    fn get_next_acquire_renew_time(&mut self) -> std::time::Duration {
        let now = Utc::now();
        let addend = match &self.state {
            LeaderState::Leading => self.config.renew_deadline,
            _ => self.config.lease_duration,
        };
        let deadline = self.last_observed_change + addend;
        if deadline > now {
            // Deadline is in the future, so delay until deadline.
            let delta = deadline - now;
            std::time::Duration::from_secs(delta.num_seconds() as u64)
        } else {
            std::time::Duration::from_secs(0)
        }
    }

    /// Set the current leader state & emit a state update.
    fn set_state(&mut self, state: LeaderState) {
        self.state = state;
        let _ = self.state_tx.send(self.state.clone());
        metrics::gauge!(METRIC_IS_LEADER, if matches!(self.state, LeaderState::Leading) { 1.0 } else { 0.0 });
    }

    /// The default timeout to use for interacting with the K8s API.
    fn timeout() -> std::time::Duration {
        std::time::Duration::from_secs(10)
    }
}
