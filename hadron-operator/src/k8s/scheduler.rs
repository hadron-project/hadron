//! Hadron scheduler logic for creating Hadron objects within a Kubernetes cluster.
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
//! from the K8s cluster for whatever reasons.
//! - Our workflow is uni-directional. The K8s controller — of which this scheduler is a part —
//! uses K8s watchers to observe a live stream of data of all pertinent Hadron cluster data. The
//! scheduler reacts to that data, and only makes updates according to that stream of data. Due to
//! the properties of SSA described above, we are able to confidently publish updates to a resource
//! and as long as the K8s API accepts the update, we know that our updates were the most up-to-date.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetUpdateStrategy};
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, EnvVarSource, ObjectFieldSelector, PersistentVolumeClaim, PersistentVolumeClaimSpec, PodSpec, PodTemplateSpec,
    ResourceRequirements, Secret, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use kube::api::{Api, ObjectMeta, Patch, PatchParams};
use kube::Resource;
use tokio::time::timeout;

use crate::config::Config;
use crate::k8s::coordination::LeaderState;
use crate::k8s::{Controller, APP_NAME};
use hadron_core::auth::TokenClaims;
use hadron_core::crd::RequiredMetadata;
use hadron_core::crd::{Pipeline, Stream, Token};

/// The secret key used for storing a generated JWT.
const SECRET_KEY_TOKEN: &str = "token";
/// The default timeout to use for API calls.
const API_TIMEOUT: Duration = Duration::from_secs(5);
/// The location where stream controllers place their data.
const STREAM_DATA_PATH: &str = "/usr/local/hadron-stream/data";

/// A scheduling task to be performed.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // Arcs vs PodInfo.
pub(super) enum SchedulerTask {
    PipelineUpdated(Arc<String>),
    PipelineDeleted(Arc<String>, Pipeline),
    SecretUpdated(Arc<String>),
    SecretDeleted(Arc<String>, Secret),
    StatefulSetUpdated(Arc<String>),
    StatefulSetDeleted(Arc<String>, StatefulSet),
    StreamUpdated(Arc<String>),
    StreamDeleted(Arc<String>, Stream),
    TokenUpdated(Arc<String>),
    TokenDeleted(Arc<String>, Token),
}

impl Controller {
    pub(super) async fn handle_scheduler_task(&mut self, task: SchedulerTask, state: LeaderState) {
        if !matches!(state, LeaderState::Leading) {
            return;
        }
        match task {
            SchedulerTask::PipelineUpdated(name) => self.scheduler_pipeline_updated(name).await,
            SchedulerTask::PipelineDeleted(name, pipeline) => self.scheduler_pipeline_deleted(name, pipeline).await,
            SchedulerTask::SecretUpdated(name) => self.scheduler_secret_updated(name).await,
            SchedulerTask::SecretDeleted(name, secret) => self.scheduler_secret_deleted(name, secret).await,
            SchedulerTask::StatefulSetUpdated(name) => self.scheduler_sts_updated(name).await,
            SchedulerTask::StatefulSetDeleted(name, set) => self.scheduler_sts_deleted(name, set).await,
            SchedulerTask::StreamUpdated(name) => self.scheduler_stream_updated(name).await,
            SchedulerTask::StreamDeleted(name, stream) => self.scheduler_stream_deleted(name, stream).await,
            SchedulerTask::TokenUpdated(name) => self.scheduler_token_updated(name).await,
            SchedulerTask::TokenDeleted(name, token) => self.scheduler_token_deleted(name, token).await,
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Pipeline Reconciliation ///////////////////////////////////////////////////
impl Controller {
    #[tracing::instrument(level = "debug", skip(self, _name))]
    async fn scheduler_pipeline_updated(&mut self, _name: Arc<String>) {
        tracing::debug!("handling scheduler pipeline updated");
        // NOTE: nothing to do here currently. Stream controllers detect these events and handle as needed.
    }

    #[tracing::instrument(level = "debug", skip(self, _name, _pipeline))]
    async fn scheduler_pipeline_deleted(&mut self, _name: Arc<String>, _pipeline: Pipeline) {
        tracing::debug!("handling scheduler pipeline delete");
        // NOTE: nothing to do here currently. Stream controllers detect these events and handle as needed.
    }
}

//////////////////////////////////////////////////////////////////////////////
// Secret Reconciliation /////////////////////////////////////////////////////
impl Controller {
    #[tracing::instrument(level = "debug", skip(self, _name))]
    async fn scheduler_secret_updated(&mut self, _name: Arc<String>) {
        tracing::debug!("handling scheduler secret updated");
        // TODO: ensure the secret has the needed key & is a valid Hadron secret, else re-mint.
        // TODO: if token does not exist, then delete the secret.
    }

    #[tracing::instrument(level = "debug", skip(self, _name, _secret))]
    async fn scheduler_secret_deleted(&mut self, _name: Arc<String>, _secret: Secret) {
        tracing::debug!("handling scheduler secret deleted");
        // TODO: recrete the secret if the token still exists.
    }
}

//////////////////////////////////////////////////////////////////////////////
// StatefulSet Reconciliation ////////////////////////////////////////////////
impl Controller {
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn scheduler_sts_updated(&mut self, name: Arc<String>) {
        tracing::debug!("handling scheduler statefulset updated");
        // TODO: create any needed services, ingresses &c for this object.
    }

    #[tracing::instrument(level = "debug", skip(self, _name, _sts))]
    async fn scheduler_sts_deleted(&mut self, _name: Arc<String>, _sts: StatefulSet) {
        tracing::debug!("handling scheduler statefulset deleted");
        // TODO: clean-up any associated services, ingresses &c for this object.
    }
}

//////////////////////////////////////////////////////////////////////////////
// Stream Reconciliation /////////////////////////////////////////////////////
impl Controller {
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn scheduler_stream_updated(&mut self, name: Arc<String>) {
        tracing::debug!("handling scheduler stream updated");
        let stream = match self.streams.get(name.as_ref()) {
            Some(stream) => stream,
            None => return,
        };

        // Check to see if we need to create a StatefulSet for this stream.
        if let Some(sts) = self.statefulsets.get(name.as_ref()) {
            reconcile_stream_changes(stream, sts).await;
        } else {
            // TODO: fetch tags, and determine latest release per semver compat group. Pass in latest release.
            let sts = build_stream_statefulset(stream, self.config.as_ref(), "ghcr.io/hadron-project/hadron/hadron-stream:0.1.0").await;
            let sts = match self.create_stateful_set(sts).await {
                Ok(sts) => sts,
                Err(err) => {
                    tracing::error!(error = ?err, stream = %name, "error creating backing StatefulSet for Stream");
                    let tx = self.scheduler_tasks_tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        let _res = tx.send(SchedulerTask::StreamUpdated(name)).await;
                    });
                    return;
                }
            };
            self.statefulsets.insert(name, sts);
            // NOTE: the creation of the StatefulSet above will trigger reconciliation events
            // which will be used to create the various K8s Services and other such resources.
        }
    }

    #[tracing::instrument(level = "debug", skip(self, _name, _stream))]
    async fn scheduler_stream_deleted(&mut self, _name: Arc<String>, _stream: Stream) {
        tracing::debug!("handling scheduler stream deleted");
        // TODO: delete StatefulSet.
    }

    /// Patch the given stream in K8s using Server-Side Apply.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    async fn patch_stream_cr(&mut self, mut stream: Stream) -> Result<Stream> {
        self.fence().await?; // Ensure we still hold the lease.
        let api: Api<Stream> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let mut params = PatchParams::apply(APP_NAME);
        params.force = true; // This will still be blocked by the server if we do not have the most up-to-date object info.
        stream.metadata.managed_fields = None;
        let stream = timeout(API_TIMEOUT, api.patch_status(stream.name(), &params, &Patch::Apply(&stream)))
            .await
            .context("timeout while updating stream")?
            .context("error updating stream")?;
        Ok(stream)
    }

    /// Create the given StatefulSet in K8s.
    #[tracing::instrument(level = "debug", skip(self, sts))]
    async fn create_stateful_set(&mut self, sts: StatefulSet) -> Result<StatefulSet> {
        let api: Api<StatefulSet> = Api::namespaced(self.client.clone(), &self.config.namespace);
        self.fence().await?;
        let params = kube::api::PostParams::default();
        timeout(API_TIMEOUT, api.create(&params, &sts))
            .await
            .context("timeout while creating backing StatefulSet for Stream")?
            .context("error creating backing StatefulSet for Stream")
    }
}

/// Update the given StatefulSet according to the state of the given Stream.
#[tracing::instrument(level = "debug", skip(stream, sts))]
async fn reconcile_stream_changes(stream: &Stream, sts: &StatefulSet) {
    tracing::debug!("reconciling stream changes");
    // TODO: construct a spec update & apply if spec is different.
    // TODO: if the changes are only related to ingress & services then construct changes for those.
}

/// Build a new StatefulSet for the given stream.
#[tracing::instrument(level = "debug", skip(stream, config, latest_hadron_stream_image))]
async fn build_stream_statefulset(stream: &Stream, config: &Config, latest_hadron_stream_image: &str) -> StatefulSet {
    tracing::debug!(name = stream.name(), "creating a new statefulset for stream");

    // Build metadata.
    let mut sts = StatefulSet::default();
    let labels = sts.meta_mut().labels.get_or_insert_with(Default::default);
    set_cannonical_labels(labels);
    labels.insert("hadron.rs/stream".into(), stream.name().into());
    let labels = labels.clone(); // Used below.
    sts.meta_mut().namespace = config.namespace.clone().into();
    sts.meta_mut().name = stream.name().to_string().into();

    // Build spec.
    let spec = sts.spec.get_or_insert_with(Default::default);
    spec.update_strategy = Some(StatefulSetUpdateStrategy {
        type_: Some("RollingUpdate".into()),
        rolling_update: None,
    });
    spec.replicas = Some(stream.spec.partitions as i32);
    spec.selector = LabelSelector {
        match_labels: Some(labels.clone()),
        ..Default::default()
    };
    let image = stream
        .spec
        .image
        .clone()
        .unwrap_or_else(|| latest_hadron_stream_image.into());
    spec.template = PodTemplateSpec {
        metadata: Some(ObjectMeta { labels: Some(labels), ..Default::default() }),
        spec: Some(PodSpec {
            containers: vec![Container {
                name: "hadron-stream".into(),
                image: Some(image),
                command: Some(vec!["/bin/hadron-stream".into()]),
                ports: Some(vec![
                    ContainerPort {
                        name: Some("client-port".into()),
                        container_port: 7000,
                        ..Default::default()
                    },
                    ContainerPort {
                        name: Some("server-port".into()),
                        container_port: 7001,
                        ..Default::default()
                    },
                ]),
                env: Some(vec![
                    EnvVar {
                        name: "RUST_LOG".into(),
                        value: Some("error,hadron_stream=info".into()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "CLIENT_PORT".into(),
                        value: Some("7000".into()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "SERVER_PORT".into(),
                        value: Some("7001".into()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "CLUSTER_NAME".into(),
                        value: Some(stream.spec.cluster_name.clone()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "NAMEPSACE".into(),
                        value_from: Some(EnvVarSource {
                            field_ref: Some(ObjectFieldSelector {
                                field_path: "metadata.namespace".into(),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "STREAM".into(),
                        value: Some(stream.name().into()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "STATEFULSET".into(),
                        value: Some(stream.name().into()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "POD_NAME".into(),
                        value_from: Some(EnvVarSource {
                            field_ref: Some(ObjectFieldSelector {
                                field_path: "metadata.name".into(),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "STORAGE_DATA_PATH".into(),
                        value: Some(STREAM_DATA_PATH.into()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "JWT_DECODING_KEY".into(),
                        value: Some(config.jwt_decoding_key.1.clone()),
                        ..Default::default()
                    },
                ]),
                volume_mounts: Some(vec![VolumeMount {
                    name: "data".into(),
                    mount_path: STREAM_DATA_PATH.into(),
                    ..Default::default()
                }]),
                ..Default::default()
            }],
            ..Default::default()
        }),
    };

    // Build volume claim templates.
    spec.volume_claim_templates = Some(vec![PersistentVolumeClaim {
        metadata: ObjectMeta { name: Some("data".into()), ..Default::default() },
        spec: Some(PersistentVolumeClaimSpec {
            access_modes: stream.spec.pvc_access_modes.clone(),
            storage_class_name: stream.spec.pvc_storage_class.clone(),
            resources: Some(ResourceRequirements {
                requests: Some(maplit::btreemap! {
                    "storage".into() => Quantity(stream.spec.pvc_volume_size.clone()),
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }]);

    sts
}

//////////////////////////////////////////////////////////////////////////////
// Token Reconciliation //////////////////////////////////////////////////////
impl Controller {
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn scheduler_token_updated(&mut self, name: Arc<String>) {
        tracing::debug!("handling scheduler token updated");
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
    async fn scheduler_token_deleted(&mut self, _name: Arc<String>, _token: Token) {
        tracing::debug!("handling scheduler token deleted");
        // TODO: delete corresponding secret.
    }

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
        let claims = TokenClaims::new();
        let jwt = claims
            .encode(&self.config.jwt_encoding_key)
            .context("error encoding claims as JWT")?;

        let mut secret = Secret::default();
        secret
            .string_data
            .get_or_insert_with(Default::default)
            .insert(SECRET_KEY_TOKEN.into(), jwt);
        secret.meta_mut().name = Some(name.as_ref().clone());
        secret.meta_mut().namespace = Some(self.config.namespace.clone());

        let labels = secret.meta_mut().labels.get_or_insert_with(Default::default);
        set_cannonical_labels(labels);

        let params = kube::api::PostParams::default();
        timeout(API_TIMEOUT, api.create(&params, &secret))
            .await
            .context("timeout while creating backing secret for token")?
            .context("error creating backing secret for token")
    }
}

//////////////////////////////////////////////////////////////////////////////
// K8s API Methods ///////////////////////////////////////////////////////////
impl Controller {
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
}

/// Set the cannonical labels on an object controlled by Hadron.
fn set_cannonical_labels(labels: &mut BTreeMap<String, String>) {
    labels.insert("app".into(), "hadron".into());
    labels.insert("hadron.rs/controlled-by".into(), "hadron".into());
}
