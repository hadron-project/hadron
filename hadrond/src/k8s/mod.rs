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
pub mod events;
mod scheduler;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::prelude::*;
use k8s_openapi::api::coordination::v1::{Lease, LeaseSpec};
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta};
use kube::api::{Api, ListParams};
use kube::client::Client;
use kube_runtime::watcher::{watcher, Error as WatcherError, Event};
use maplit::btreemap;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, UnboundedReceiverStream, WatchStream};

use crate::config::Config;
use crate::crd::{Pipeline, Stream, Token};
use crate::k8s::coordination::{LeaderElectionConfig, LeaderElector};
use crate::k8s::events::CrdStateChange;
use crate::k8s::scheduler::SchedulerTask;

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
    /// A channel for emitting CRD state change events.
    events_tx: mpsc::Sender<CrdStateChange>,
    /// The configuration used to drive the leader election system, moved out after being spawned.
    leader_election_config: Option<LeaderElectionConfig>,

    /// A channel of scheduler tasks.
    scheduler_tasks_tx: mpsc::UnboundedSender<SchedulerTask>,
    /// A channel of scheduler tasks.
    scheduler_tasks_rx: UnboundedReceiverStream<SchedulerTask>,
    /// All known stream objects of this cluster.
    streams: HashMap<Arc<String>, Arc<Stream>>,
    /// All known pipeline objects of this cluster.
    pipelines: HashMap<Arc<String>, Arc<Pipeline>>,
    /// All known token objects of this cluster.
    tokens: HashMap<Arc<String>, Arc<Token>>,
    /// All known pods of this cluster.
    ///
    /// We are using a B-tree here as keys stay sorted.
    pods: BTreeMap<Arc<String>, PodInfo>,
    /// A scheduling scratch pad for tracking in-flight pod assignment info before it has
    /// been committed.
    ///
    /// In order to keep the scheduler accurate, this data should be cleared after each
    /// stream-scoped scheduling task.
    ///
    /// The layout is such that pod names are hashed for efficiency and to reduce
    /// data cloning/copying.
    pods_scratch: BTreeMap<u64, PodInfo>,
}

/// Info on a Hadron pod along with its cluster assignment info.
#[derive(Debug, Default)]
struct PodInfo {
    /// The corresponding pod data model from K8s.
    pub pod: Option<Pod>,
    /// All streams assigned to this pod for partition leadership.
    pub stream_leaders: BTreeSet<Arc<String>>,
    /// All streams assigned to this pod for partition replication.
    pub stream_replicas: BTreeMap<Arc<String>, u64>,
}

impl Controller {
    /// Create a new instance.
    pub fn new(client: Client, config: Arc<Config>, shutdown_tx: broadcast::Sender<()>, events_tx: mpsc::Sender<CrdStateChange>) -> Result<Self> {
        let elect_conf = LeaderElectionConfig::new(
            &config.namespace,
            &Self::generate_lease_name(&config),
            config.pod_name.as_ref().clone(),
            chrono::Duration::seconds(config.lease_duration_seconds as i64),
            chrono::Duration::seconds(config.lease_renew_seconds as i64),
            chrono::Duration::seconds(LEASE_RETRY_SECONDS),
        )
        .context("invalid lease coordination config")?;
        let (scheduler_tasks_tx, scheduler_tasks_rx) = mpsc::unbounded_channel();
        Ok(Self {
            client,
            config,
            shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
            shutdown_tx,
            scheduler_tasks_tx,
            scheduler_tasks_rx: UnboundedReceiverStream::new(scheduler_tasks_rx),
            events_tx,
            leader_election_config: Some(elect_conf),
            streams: Default::default(),
            pipelines: Default::default(),
            tokens: Default::default(),
            pods: Default::default(),
            pods_scratch: Default::default(),
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
            self.config.pod_name.as_ref(),
            self.client.clone(),
            self.shutdown_tx.subscribe(),
        );
        let (elector, mut state_rx) = (elector.spawn(), WatchStream::new(state_rx_raw.clone()));

        // // Perform an initial indexing of all pertinent data before opening watch streams.
        // self.index_k8s_data().await;

        // Build watcher streams.
        let params_labels = self.list_params_cluster_selector_labels();
        let params_spec = ListParams::default();
        let pipelines: Api<Pipeline> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let pipelines_watcher = watcher(pipelines, params_spec.clone());
        let streams: Api<Stream> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let streams_watcher = watcher(streams, params_spec.clone());
        let tokens: Api<Token> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let tokens_watcher = watcher(tokens, params_spec.clone());
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let pods_watcher = watcher(pods, params_labels);
        tokio::pin!(pipelines_watcher, streams_watcher, tokens_watcher, pods_watcher);

        tracing::info!("k8s controller initialized");
        loop {
            tokio::select! {
                Some(k8s_event_res) = pipelines_watcher.next() => self.handle_pipeline_event(k8s_event_res).await,
                Some(k8s_event_res) = streams_watcher.next() => self.handle_stream_event(k8s_event_res).await,
                Some(k8s_event_res) = tokens_watcher.next() => self.handle_token_event(k8s_event_res).await,
                Some(k8s_event_res) = pods_watcher.next() => self.handle_pod_event(k8s_event_res).await,
                Some(new_leader_state) = state_rx.next() => tracing::debug!(state = ?new_leader_state, "new leader state detected"),
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

    /// Create a list params object which selects only objects which matching Hadron labels.
    fn list_params_cluster_selector_labels(&self) -> ListParams {
        ListParams {
            label_selector: Some(format!("app=hadron,hadron.rs/cluster={}", self.config.cluster)),
            ..Default::default()
        }
    }

    /// Generate the name to be used for the lease.
    fn generate_lease_name(config: &Config) -> String {
        format!("hadron-cluster-{}", &config.cluster)
    }

    /// Generate a lease object to be used with the leader coordination system.
    fn generate_lease(config: &Config) -> Lease {
        let now = chrono::Utc::now();
        Lease {
            metadata: ObjectMeta {
                name: Some(Self::generate_lease_name(&config)),
                namespace: Some(config.namespace.clone()),
                labels: Some(btreemap! {
                    "hadron.rs/cluster".into() => config.cluster.clone(),
                    "app".into() => "hadron".into(),
                    "app.kubernetes.io/name".into() => "hadron".into(),
                }),
                ..Default::default()
            },
            spec: Some(LeaseSpec {
                acquire_time: Some(MicroTime(now)),
                holder_identity: Some(config.pod_name.as_ref().clone()),
                lease_duration_seconds: Some(config.lease_duration_seconds as i32),
                lease_transitions: Some(0),
                renew_time: Some(MicroTime(now)),
            }),
        }
    }
}
