//! Kubernetes controller.
//!
//! This controller is used to observe data in K8s, filter out data unrelated to this cluster,
//! cache data that does apply to this cluster, and spawn any objects which have been assigned to
//! this node.
//!
//! When this controller is the cluster leader, it will also perform various leadership functions,
//! including the assignment of objects to nodes of the cluster. Leaders are also responsible for
//! healthchecking cluster members and detecting partition leadership failures. When a partition
//! leader fails, the cluster leader will assign a new leader for the partition.

mod coordination;
mod data;
mod scheduler;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::prelude::*;
use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::coordination::v1::{Lease, LeaseSpec};
use k8s_openapi::api::core::v1::{Secret, Service};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta};
use kube::api::{Api, ListParams};
use kube::client::Client;
use kube_runtime::watcher::{watcher, Error as WatcherError, Event};
use maplit::btreemap;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream, WatchStream};

use crate::config::Config;
use crate::k8s::coordination::{LeaderElectionConfig, LeaderElector, LeaderState};
use crate::k8s::scheduler::SchedulerTask;
use hadron_core::crd::{Pipeline, Stream, Token};

/// The app name used by the operator.
const APP_NAME: &str = "hadron-operator";
/// The timeout duration used before rescheduling a scheduler task.
const RESCHEDULE_TIMEOUT: Duration = Duration::from_secs(5);

type EventResult<T> = std::result::Result<Event<T>, WatcherError>;

/// The duration which leader elector clients should wait between action retries.
///
/// Core K8s clients default this value to 2 seconds.
const LEASE_RETRY_SECONDS: i64 = 2;

/// Kubernetes controller for watching Hadron CRs.
pub struct Controller {
    /// K8s client.
    client: Client,
    /// Runtime config.
    config: Arc<Config>,
    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
    /// The configuration used to drive the leader election system, moved out after being spawned.
    leader_election_config: Option<LeaderElectionConfig>,
    /// The name of the lease used by this cluster.
    lease_name: String,
    /// The currently known leader state.
    leader_state: Option<LeaderState>,

    /// A channel of scheduler tasks.
    scheduler_tasks_tx: mpsc::Sender<SchedulerTask>,
    /// A channel of scheduler tasks.
    scheduler_tasks_rx: ReceiverStream<SchedulerTask>,

    /// All known statefulsets managed by this operator.
    statefulsets: HashMap<Arc<String>, StatefulSet>,
    /// All known stream objects managed by this operator.
    streams: HashMap<Arc<String>, Stream>,
    /// All known pipeline objects managed by this operator.
    pipelines: HashMap<Arc<String>, Pipeline>,
    /// All known token objects managed by this operator.
    tokens: HashMap<Arc<String>, Token>,
    /// All known K8s secrets corresponding to Hadron Tokens of this cluster.
    secrets: HashMap<Arc<String>, Secret>,
    /// All known K8s services corresponding to a StatefulSet backing a Stream and its pods.
    services: HashMap<Arc<String>, Service>,
}

impl Controller {
    /// Create a new instance.
    pub fn new(client: Client, config: Arc<Config>, shutdown_tx: broadcast::Sender<()>) -> Result<Self> {
        let lease_name = Self::generate_lease_name();
        let elect_conf = LeaderElectionConfig::new(
            &config.namespace,
            &lease_name,
            config.pod_name.clone(),
            chrono::Duration::seconds(config.lease_duration_seconds as i64),
            chrono::Duration::seconds(config.lease_renew_seconds as i64),
            chrono::Duration::seconds(LEASE_RETRY_SECONDS),
        )
        .context("invalid lease coordination config")?;
        let (scheduler_tasks_tx, scheduler_tasks_rx) = mpsc::channel(1000);
        Ok(Self {
            client,
            config,
            shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
            shutdown_tx,
            scheduler_tasks_tx,
            scheduler_tasks_rx: ReceiverStream::new(scheduler_tasks_rx),
            leader_election_config: Some(elect_conf),
            lease_name,
            leader_state: None,
            statefulsets: Default::default(),
            streams: Default::default(),
            pipelines: Default::default(),
            tokens: Default::default(),
            secrets: Default::default(),
            services: Default::default(),
        })
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        // Spawn leader elector.
        let elect_config = match self.leader_election_config.take() {
            Some(elect_config) => elect_config,
            None => {
                let _ = self.shutdown_tx.send(());
                anyhow::bail!("error accessing leader election config, this should never happen");
            }
        };
        let lease = Self::generate_lease(&self.config);
        let (elector, state_rx_raw) = LeaderElector::new(
            lease,
            elect_config,
            self.config.pod_name.as_str(),
            self.client.clone(),
            self.shutdown_tx.subscribe(),
        );
        let (elector, mut state_rx) = (elector.spawn(), WatchStream::new(state_rx_raw.clone()));

        // Build watcher streams.
        let params_labels = self.list_params_cluster_selector_labels();
        let params_spec = ListParams::default();
        let statefulsets: Api<StatefulSet> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let statefulsets_watcher = watcher(statefulsets, params_labels.clone());
        let pipelines: Api<Pipeline> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let pipelines_watcher = watcher(pipelines, params_spec.clone());
        let streams: Api<Stream> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let streams_watcher = watcher(streams, params_spec.clone());
        let tokens: Api<Token> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let tokens_watcher = watcher(tokens, params_spec.clone());
        let secrets: Api<Secret> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let secrets_watcher = watcher(secrets, params_labels.clone());
        let services: Api<Service> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let services_watcher = watcher(services, params_labels.clone());
        tokio::pin!(
            statefulsets_watcher,
            pipelines_watcher,
            streams_watcher,
            tokens_watcher,
            secrets_watcher,
            services_watcher
        );

        tracing::info!("k8s controller initialized");
        loop {
            tokio::select! {
                Some(k8s_event_res) = statefulsets_watcher.next() => self.handle_sts_event(k8s_event_res).await,
                Some(k8s_event_res) = pipelines_watcher.next() => self.handle_pipeline_event(k8s_event_res).await,
                Some(k8s_event_res) = services_watcher.next() => self.handle_service_event(k8s_event_res).await,
                Some(k8s_event_res) = streams_watcher.next() => self.handle_stream_event(k8s_event_res).await,
                Some(k8s_event_res) = secrets_watcher.next() => self.handle_secret_event(k8s_event_res).await,
                Some(k8s_event_res) = tokens_watcher.next() => self.handle_token_event(k8s_event_res).await,
                Some(new_leader_state) = state_rx.next() => {
                    // If just becoming a leader, then perform a full data reconciliation to ensure
                    // there are no outstanding tasks which need to be performed since the last leader.
                    //
                    // NOTE WELL: this routine will block this controller from making progress elsewhere.
                    // This is required as an up-to-date view of the system data is required.
                    tracing::debug!(state = ?new_leader_state, "new leader state detected");
                    self.leader_state = Some(new_leader_state.clone());
                    if matches!(&self.leader_state, Some(LeaderState::Leading)) {
                        loop {
                            if let Err(err) = self.full_data_reconciliation().await {
                                tracing::error!(error = ?err, "error performing full data reconciliation");
                                tokio::time::sleep(Duration::from_secs(5)).await;
                                continue;
                            }
                            break;
                        }
                    }
                }
                Some(scheduler_task) = self.scheduler_tasks_rx.next() => {
                    let state = { state_rx_raw.borrow().clone() }; // Ensure borrow ref doesn't leak read lock.
                    self.handle_scheduler_task(scheduler_task, state).await;
                }
                _ = self.shutdown_rx.next() => break,
            }
        }

        tracing::debug!("k8s controller shutting down");
        if let Err(err) = elector.await {
            tracing::error!(error = ?err, "error shutting down leader elector");
        }

        tracing::debug!("k8s controller shutdown");
        Ok(())
    }

    /// Fetch all needed data from the backing K8s cluster.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn full_data_reconciliation(&mut self) -> Result<()> {
        let params_spec = ListParams::default();
        let api: Api<Pipeline> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let pipelines = api.list(&params_spec).await.context("error fetching pipelines")?;
        let api: Api<Stream> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let streams = api.list(&params_spec).await.context("error fetching streams")?;
        let api: Api<Token> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let tokens = api.list(&params_spec).await.context("error fetching tokens")?;

        let params_labels = self.list_params_cluster_selector_labels();
        let api: Api<StatefulSet> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let statefulsets = api.list(&params_labels).await.context("error fetching statefulsets")?;
        let api: Api<Secret> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let secrets = api.list(&params_labels).await.context("error fetching secrets")?;
        let api: Api<Service> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let services = api.list(&params_labels).await.context("error fetching services")?;

        self.handle_pipeline_event(Ok(Event::Restarted(pipelines.items))).await;
        self.handle_stream_event(Ok(Event::Restarted(streams.items))).await;
        self.handle_token_event(Ok(Event::Restarted(tokens.items))).await;
        self.handle_sts_event(Ok(Event::Restarted(statefulsets.items))).await;
        self.handle_secret_event(Ok(Event::Restarted(secrets.items))).await;
        self.handle_service_event(Ok(Event::Restarted(services.items))).await;

        Ok(())
    }

    /// Spawn a task which emits a new scheduler tasks.
    ///
    /// This indirection is used to ensure that we don't use an unlimited amount of memory with an
    /// unbounded queue, and also so that we do not block the controller from making progress and
    /// dead-locking when we hit the scheduler task queue cap.
    ///
    /// The runtime will stack up potentially lots of tasks, and memory will be consumed that way,
    /// but ultimately the controller will be able to begin processing scheduler tasks and will
    /// drain the scheduler queue and ultimately relieve the memory pressure of the tasks.
    fn spawn_scheduler_task(&self, task: SchedulerTask, is_retry: bool) {
        let tx = self.scheduler_tasks_tx.clone();
        tokio::spawn(async move {
            if is_retry {
                tokio::time::sleep(RESCHEDULE_TIMEOUT).await;
            }
            let _res = tx.send(task).await;
        });
    }

    /// Create a list params object which selects only objects which matching Hadron labels.
    fn list_params_cluster_selector_labels(&self) -> ListParams {
        ListParams {
            label_selector: Some("app=hadron,hadron.rs/controlled-by=hadron-operator".into()),
            ..Default::default()
        }
    }

    /// Generate the name to be used for the lease.
    fn generate_lease_name() -> String {
        APP_NAME.into()
    }

    /// Generate a lease object to be used with the leader coordination system.
    fn generate_lease(config: &Config) -> Lease {
        let now = chrono::Utc::now();
        Lease {
            metadata: ObjectMeta {
                name: Some(Self::generate_lease_name()),
                namespace: Some(config.namespace.clone()),
                labels: Some(btreemap! {
                    "app".into() => "hadron".into(),
                    "hadron.rs/controlled-by".into() => APP_NAME.into(),
                }),
                ..Default::default()
            },
            spec: Some(LeaseSpec {
                acquire_time: Some(MicroTime(now)),
                holder_identity: Some(config.pod_name.clone()),
                lease_duration_seconds: Some(config.lease_duration_seconds as i32),
                lease_transitions: Some(0),
                renew_time: Some(MicroTime(now)),
            }),
        }
    }
}
