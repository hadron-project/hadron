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
//!
//! ## Object Updates
//! All scheduler tasks are broken up into "updated" & "deleted" tasks. Deletion tasks are easy.
//! Updated tasks on the other hand can be a bit tricky. In order to ensure that we do not have
//! stale data which needs to be deleted (for cases where the corresponding deleted event has been
//! missed), we check for the possibility of deletion being needed in all "updated" handlers.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetUpdateStrategy};
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, EnvVarSource, ObjectFieldSelector, PersistentVolumeClaim, PersistentVolumeClaimSpec, PodSpec, PodTemplateSpec,
    ResourceRequirements, Secret, Service, ServicePort, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{Api, ObjectMeta, Patch, PatchParams};
use kube::Resource;
use tokio::time::timeout;

use crate::k8s::LeaderState;
use crate::k8s::{Controller, APP_NAME};
use hadron_core::auth::TokenClaims;
use hadron_core::crd::RequiredMetadata;
use hadron_core::crd::{Pipeline, Stream, Token};

/// The default timeout to use for API calls.
const API_TIMEOUT: Duration = Duration::from_secs(5);
/// The pod container name of the Hadron Stream.
///
/// NOTE WELL: do not change the name of this container. It will cause breaking changes.
const CONTAINER_NAME_HADRON_STREAM: &str = "hadron-stream";
/// The canonical K8s label used for identifying a StatefulSet pod name.
const LABEL_K8S_STS_POD_NAME: &str = "statefulset.kubernetes.io/pod-name";
/// The canonical Hadron label identifying a Stream.
const LABEL_HADRON_RS_STREAM: &str = "hadron.rs/stream";
/// The canonical Hadron label identifying a StatefulSet.
const LABEL_HADRON_RS_STS: &str = "hadron.rs/statefulset";
/// The secret key used for storing a generated JWT.
const SECRET_KEY_TOKEN: &str = "token";
/// The location where stream controllers place their data.
const STREAM_DATA_PATH: &str = "/usr/local/hadron-stream/data";

/// A scheduling task to be performed.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // Arcs vs PodInfo.
pub enum SchedulerTask {
    PipelineUpdated(Arc<String>),
    PipelineDeleted(Arc<String>, Pipeline),
    SecretUpdated(Arc<String>),
    SecretDeleted(Arc<String>, Secret),
    ServiceUpdated(Arc<String>),
    ServiceDeleted(Arc<String>, Service),
    StatefulSetUpdated(Arc<String>),
    StatefulSetDeleted(Arc<String>, StatefulSet),
    StreamUpdated(Arc<String>),
    StreamDeleted(Arc<String>, Stream),
    TokenUpdated(Arc<String>),
    TokenDeleted(Arc<String>, Token),
}

impl Controller {
    /// Handle scheduler tasks.
    ///
    /// **NOTE WELL:** we keep the receiver **immutable** to enforce a uni-directional data flow,
    /// where the scheduler makes changes to the K8s API, and the watchers observe this data,
    /// indexes it, and then produces additional reconciliation tasks as needed.
    pub(super) async fn handle_scheduler_task(&self, task: SchedulerTask, state: LeaderState) {
        if !matches!(state, LeaderState::Leading) {
            return;
        }
        match task {
            SchedulerTask::PipelineUpdated(name) => self.scheduler_pipeline_updated(name).await,
            SchedulerTask::PipelineDeleted(name, pipeline) => self.scheduler_pipeline_deleted(name, pipeline).await,
            SchedulerTask::SecretUpdated(name) => self.scheduler_secret_updated(name).await,
            SchedulerTask::SecretDeleted(name, secret) => self.scheduler_secret_deleted(name, secret).await,
            SchedulerTask::ServiceUpdated(name) => self.scheduler_service_updated(name).await,
            SchedulerTask::ServiceDeleted(name, service) => self.scheduler_service_deleted(name, service).await,
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
    async fn scheduler_pipeline_updated(&self, _name: Arc<String>) {
        tracing::debug!("handling scheduler pipeline updated");
        // NOTE: nothing to do here currently. Stream controllers detect these events and handle as needed.
    }

    #[tracing::instrument(level = "debug", skip(self, _name, _pipeline))]
    async fn scheduler_pipeline_deleted(&self, _name: Arc<String>, _pipeline: Pipeline) {
        tracing::debug!("handling scheduler pipeline delete");
        // NOTE: nothing to do here currently. Stream controllers detect these events and handle as needed.
    }
}

//////////////////////////////////////////////////////////////////////////////
// Secret Reconciliation /////////////////////////////////////////////////////
impl Controller {
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn scheduler_secret_updated(&self, name: Arc<String>) {
        tracing::debug!("handling scheduler secret updated");
        // If this object's parent Token does not exist, then delete this object.
        if !self.tokens.contains_key(&name) {
            if let Err(err) = self.delete_token_secret(name.clone()).await {
                tracing::error!(error = ?err, "error while deleting backing secret for token");
                self.spawn_scheduler_task(SchedulerTask::SecretUpdated(name), true);
            }
        }

        // NOTE: If a secret enters into a bad state due to being manually manipulated,
        // then delete it, and the operator will re-create it as needed.
    }

    #[tracing::instrument(level = "debug", skip(self, name, secret))]
    async fn scheduler_secret_deleted(&self, name: Arc<String>, secret: Secret) {
        tracing::debug!("handling scheduler secret deleted");
        if !self.tokens.contains_key(name.as_ref()) {
            return;
        }
        if let Err(err) = self.ensure_token_secret(name.clone()).await {
            tracing::error!(error = ?err, name = %name, "error creating backing secret for token");
            self.spawn_scheduler_task(SchedulerTask::SecretDeleted(name, secret), true);
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Service Reconciliation ////////////////////////////////////////////////////
impl Controller {
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn scheduler_service_updated(&self, name: Arc<String>) {
        tracing::debug!("handling scheduler service updated");
        let _service = match self.services.get(&name) {
            Some(service) => service,
            None => return,
        };

        // If this object's parent STS or STS Pod does not exist, then delete this object.
        // let is_pod_svc = service.metadata.annotations.map(|anns| anns.contains_key(LABEL_K8S_STS_POD_NAME)).unwrap_or(false);
        // TODO: if sts does not exist, or if STS Pod doesn't exist for a pod service, then delete this object.

        // If a service enters into a bad state due to being manually manipulated, then delete it,
        // and the operator will re-create it as needed.
    }

    #[tracing::instrument(level = "debug", skip(self, _name, _service))]
    async fn scheduler_service_deleted(&self, _name: Arc<String>, _service: Service) {
        tracing::debug!("handling scheduler service deleted");
        // TODO:
        // - check the service's labels to see if it corresponds to a STS or a STS pod.
        // - once determined, check to see if the corresponding object still exists.
        // - if still exists, then re-create the service, else no-op.
    }
}

//////////////////////////////////////////////////////////////////////////////
// StatefulSet Reconciliation ////////////////////////////////////////////////
impl Controller {
    #[tracing::instrument(level = "debug", skip(self, _name))]
    async fn scheduler_sts_updated(&self, _name: Arc<String>) {
        tracing::debug!("handling scheduler statefulset updated");
        // TODO: if this object's parent (Stream) does not exist, then delete this object.
        // TODO: create any needed services, ingresses &c for this object.
    }

    #[tracing::instrument(level = "debug", skip(self, _name, _sts))]
    async fn scheduler_sts_deleted(&self, _name: Arc<String>, _sts: StatefulSet) {
        tracing::debug!("handling scheduler statefulset deleted");
        // TODO: clean-up any associated services, ingresses &c for this object.
    }
}

//////////////////////////////////////////////////////////////////////////////
// Stream Reconciliation /////////////////////////////////////////////////////
impl Controller {
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn scheduler_stream_updated(&self, name: Arc<String>) {
        tracing::debug!("handling scheduler stream updated");
        let stream = match self.streams.get(name.as_ref()) {
            Some(stream) => stream,
            None => return,
        };

        // Check to see if we need to create a StatefulSet for this stream.
        let mut needs_retry = false;
        let sts_res = if let Some(sts) = self.statefulsets.get(name.as_ref()) {
            self.reconcile_stream_changes(stream, sts)
                .await
                .context("error reconciling Stream changes")
        } else {
            let sts = self.build_stream_statefulset(stream);
            self.create_statefulset(sts)
                .await
                .context("error creating new backing StatefulSet for Stream")
                .map(|_sts| ())
        };
        if let Err(err) = sts_res {
            needs_retry = true;
            tracing::error!(error = ?err, stream = %name, "error reconciling StatefulSet for updated Stream");
        }

        // Ensure the STS Service exists.
        if let Err(err) = self.ensure_sts_service(&name, stream).await {
            needs_retry = true;
            tracing::error!(error = ?err, stream = %name, "error ensuring StatefulSet Service for updated Stream");
        }
        // Ensure the STS Services exist for StatefulSet pod replicas.
        if let Err(err) = self.ensure_sts_pods_services(&name, stream).await {
            needs_retry = true;
            tracing::error!(error = ?err, stream = %name, "error ensuring StatefulSet Pod Services for updated Stream");
        }

        // Spawn a retry if needed.
        if needs_retry {
            self.spawn_scheduler_task(SchedulerTask::StreamUpdated(name), true);
        }

        // NOTE: the creation/update of the StatefulSet above will trigger reconciliation events
        // which will be used to index the newly created object and to create the various
        // K8s Services and other such resources.
    }

    #[tracing::instrument(level = "debug", skip(self, name, stream))]
    async fn scheduler_stream_deleted(&self, name: Arc<String>, stream: Stream) {
        tracing::debug!("handling scheduler stream deleted");
        if let Err(err) = self.delete_statefulset(name.as_str()).await {
            tracing::error!(error = ?err, stream = %name, "error deleting backing StatefulSet for Stream");
            self.spawn_scheduler_task(SchedulerTask::StreamDeleted(name, stream), true);
        }
    }

    /// Ensure the K8s Service exists for the given Stream's StatefulSet.
    #[tracing::instrument(level = "debug", skip(self, name, stream))]
    async fn ensure_sts_service(&self, name: &Arc<String>, stream: &Stream) -> Result<()> {
        if let Some(_service) = self.services.get(name) {
            return Ok(()); // No-op, service exists.
        }

        // Service does not exist in cache, so create it.
        let service = self.build_sts_service(stream);
        let api: Api<Service> = Api::namespaced(self.client.clone(), &self.config.namespace);
        self.fence().await?;
        timeout(API_TIMEOUT, api.create(&Default::default(), &service))
            .await
            .context("timeout while creating frontend Service for Stream StatefulSet")?
            .context("error creating frontend Service for Stream StatefulSet")?;
        Ok(())
    }

    /// Build a frontend Service for a Stream's StatefulSet.
    fn build_sts_service(&self, stream: &Stream) -> Service {
        tracing::debug!(name = stream.name(), "creating a new service for stream statefulset");

        // Build metadata.
        let mut service = Service::default();
        let labels = service.meta_mut().labels.get_or_insert_with(Default::default);
        set_cannonical_labels(labels);
        labels.insert(LABEL_HADRON_RS_STS.into(), stream.name().into());
        service.meta_mut().namespace = self.config.namespace.clone().into();
        service.meta_mut().name = stream.name().to_string().into();

        // Build spec.
        let spec = service.spec.get_or_insert_with(Default::default);
        let selector = spec.selector.get_or_insert_with(Default::default);
        set_cannonical_labels(selector);
        selector.insert(LABEL_HADRON_RS_STREAM.into(), stream.name().into());
        spec.ports = Some(vec![
            ServicePort {
                name: Some("client-port".into()),
                port: 7000,
                protocol: Some("TCP".into()),
                target_port: Some(IntOrString::Int(7000)),
                ..Default::default()
            },
            ServicePort {
                name: Some("server-port".into()),
                port: 7001,
                protocol: Some("TCP".into()),
                target_port: Some(IntOrString::Int(7001)),
                ..Default::default()
            },
        ]);

        service
    }

    /// Ensure the K8s Services exists for the Pods of the given Stream's StatefulSet.
    #[tracing::instrument(level = "debug", skip(self, name, stream))]
    async fn ensure_sts_pods_services(&self, name: &Arc<String>, stream: &Stream) -> Result<()> {
        let mut pod_services = vec![];
        for replica in 0..stream.spec.partitions {
            let service_name = format!("{}-{}", name, replica);
            if let Some(_service) = self.services.get(&service_name) {
                continue;
            }
            let pod_service = self.build_sts_pod_service(stream, service_name);
            pod_services.push(pod_service);
        }

        // Service does not exist in cache, so create it.
        for service in pod_services {
            let api: Api<Service> = Api::namespaced(self.client.clone(), &self.config.namespace);
            self.fence().await?;
            timeout(API_TIMEOUT, api.create(&Default::default(), &service))
                .await
                .context("timeout while creating frontend Service for Stream StatefulSet Pod")?
                .context("error creating frontend Service for Stream StatefulSet Pod")?;
        }
        Ok(())
    }

    /// Build a frontend Service for a Stream StatefulSet Pod.
    fn build_sts_pod_service(&self, stream: &Stream, service_name: String) -> Service {
        tracing::debug!(name = stream.name(), "creating a new service for stream statefulset pod");

        // Build metadata.
        let mut service = Service::default();
        let labels = service.meta_mut().labels.get_or_insert_with(Default::default);
        set_cannonical_labels(labels);
        labels.insert(LABEL_HADRON_RS_STS.into(), stream.name().into());
        service.meta_mut().namespace = self.config.namespace.clone().into();
        service.meta_mut().name = Some(service_name.clone());

        // Build spec.
        let spec = service.spec.get_or_insert_with(Default::default);
        let selector = spec.selector.get_or_insert_with(Default::default);
        set_cannonical_labels(selector);
        selector.insert(LABEL_HADRON_RS_STREAM.into(), stream.name().into());
        selector.insert(LABEL_K8S_STS_POD_NAME.into(), service_name);
        spec.ports = Some(vec![
            ServicePort {
                name: Some("client-port".into()),
                port: 7000,
                protocol: Some("TCP".into()),
                target_port: Some(IntOrString::Int(7000)),
                ..Default::default()
            },
            ServicePort {
                name: Some("server-port".into()),
                port: 7001,
                protocol: Some("TCP".into()),
                target_port: Some(IntOrString::Int(7001)),
                ..Default::default()
            },
        ]);

        service
    }

    /// Delete the target StatefulSet.
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn delete_statefulset(&self, name: &str) -> Result<()> {
        let api: Api<StatefulSet> = Api::namespaced(self.client.clone(), &self.config.namespace);
        self.fence().await?;
        let res = timeout(API_TIMEOUT, api.delete(name, &Default::default()))
            .await
            .context("timeout while deleting backing secret for token")?;
        match res {
            Ok(_val) => Ok(()),
            Err(err) => match err {
                kube::Error::Api(api_err) if api_err.code == http::StatusCode::NOT_FOUND => Ok(()),
                _ => Err(err).context("error deleting backing secret for token"),
            },
        }
    }

    /// Update the given StatefulSet according to the state of the given Stream.
    #[tracing::instrument(level = "debug", skip(self, stream, sts))]
    async fn reconcile_stream_changes(&self, stream: &Stream, sts: &StatefulSet) -> Result<()> {
        tracing::debug!("reconciling stream changes");
        // Construct an updated StatefulSet object, and if it differs from the currently recorded
        // StatefulSet, then issue an update.
        let mut updated_sts = self.build_stream_statefulset(stream);
        updated_sts.metadata = sts.metadata.clone();
        if updated_sts.spec != sts.spec {
            let _updated_sts = self.patch_statefulset(updated_sts).await?;
        }
        Ok(())
    }

    /// Patch the given Stream in K8s using Server-Side Apply.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    async fn patch_stream_cr(&self, mut stream: Stream) -> Result<Stream> {
        self.fence().await?; // Ensure we still hold the lease.
        let api: Api<Stream> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let mut params = PatchParams::apply(APP_NAME);
        params.force = true; // This will still be blocked by the server if we do not have the most up-to-date object info.
        stream.metadata.managed_fields = None;
        timeout(API_TIMEOUT, api.patch_status(stream.name(), &params, &Patch::Apply(&stream)))
            .await
            .context("timeout while updating stream")?
            .context("error updating stream")
    }

    /// Patch the given StatefulSet in K8s using Server-Side Apply.
    #[tracing::instrument(level = "debug", skip(self, sts))]
    async fn patch_statefulset(&self, mut sts: StatefulSet) -> Result<StatefulSet> {
        self.fence().await?; // Ensure we still hold the lease.
        let api: Api<StatefulSet> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let mut params = PatchParams::apply(APP_NAME);
        params.force = true; // This will still be blocked by the server if we do not have the most up-to-date object info.
        sts.metadata.managed_fields = None;
        let name = sts.metadata.name.as_deref().unwrap_or("");
        timeout(API_TIMEOUT, api.patch(name, &params, &Patch::Apply(&sts)))
            .await
            .context("timeout while updating StatefulSet for Stream")?
            .context("error updating StatefulSet for Stream")
    }

    /// Create the given StatefulSet in K8s.
    #[tracing::instrument(level = "debug", skip(self, sts))]
    async fn create_statefulset(&self, sts: StatefulSet) -> Result<StatefulSet> {
        let api: Api<StatefulSet> = Api::namespaced(self.client.clone(), &self.config.namespace);
        self.fence().await?;
        let params = kube::api::PostParams::default();
        timeout(API_TIMEOUT, api.create(&params, &sts))
            .await
            .context("timeout while creating backing StatefulSet for Stream")?
            .context("error creating backing StatefulSet for Stream")
    }

    /// Build a new StatefulSet for the given stream.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    fn build_stream_statefulset(&self, stream: &Stream) -> StatefulSet {
        tracing::debug!(name = stream.name(), "creating a new statefulset for stream");

        // Build metadata.
        let mut sts = StatefulSet::default();
        let labels = sts.meta_mut().labels.get_or_insert_with(Default::default);
        set_cannonical_labels(labels);
        labels.insert(LABEL_HADRON_RS_STREAM.into(), stream.name().into());
        let labels = labels.clone(); // Used below.
        sts.meta_mut().namespace = self.config.namespace.clone().into();
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
        let rust_log = if stream.spec.debug {
            "error,hadron_stream=debug"
        } else {
            "error,hadron_stream=info"
        };
        spec.template = PodTemplateSpec {
            metadata: Some(ObjectMeta { labels: Some(labels), ..Default::default() }),
            spec: Some(PodSpec {
                termination_grace_period_seconds: Some(30),
                service_account_name: Some("hadron-stream".into()),
                automount_service_account_token: Some(true),
                containers: vec![Container {
                    // NOTE WELL: do not change the name of this container. It will cause breaking changes.
                    name: CONTAINER_NAME_HADRON_STREAM.into(),
                    image: Some(stream.spec.image.clone()),
                    image_pull_policy: Some("IfNotPresent".into()),
                    command: Some(vec!["/bin/hadron-stream".into()]),
                    ports: Some(vec![
                        ContainerPort {
                            name: Some("client-port".into()),
                            container_port: 7000,
                            protocol: Some("TCP".into()),
                            ..Default::default()
                        },
                        ContainerPort {
                            name: Some("server-port".into()),
                            container_port: 7001,
                            protocol: Some("TCP".into()),
                            ..Default::default()
                        },
                    ]),
                    env: Some(vec![
                        EnvVar {
                            name: "RUST_LOG".into(),
                            value: Some(rust_log.into()),
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
                            name: "NAMESPACE".into(),
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
                            value: Some(self.config.jwt_decoding_key.1.clone()),
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
                access_modes: stream
                    .spec
                    .pvc_access_modes
                    .clone()
                    .or_else(|| Some(vec!["ReadWriteOnce".into()])),
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
}

//////////////////////////////////////////////////////////////////////////////
// Token Reconciliation //////////////////////////////////////////////////////
impl Controller {
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn scheduler_token_updated(&self, name: Arc<String>) {
        tracing::debug!("handling scheduler token updated");
        if self.secrets.get(&name).is_none() {
            if let Err(err) = self.ensure_token_secret(name.clone()).await {
                tracing::error!(error = ?err, "error while ensuring backing secret for token");
                self.spawn_scheduler_task(SchedulerTask::TokenUpdated(name), true);
            }
        }
    }

    #[tracing::instrument(level = "debug", skip(self, name, token))]
    async fn scheduler_token_deleted(&self, name: Arc<String>, token: Token) {
        tracing::debug!("handling scheduler token deleted");
        if let Err(err) = self.delete_token_secret(name.clone()).await {
            tracing::error!(error = ?err, "error while deleting backing secret for token");
            self.spawn_scheduler_task(SchedulerTask::TokenDeleted(name, token), true);
        }
    }

    /// Delete the target token secret.
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn delete_token_secret(&self, name: Arc<String>) -> Result<()> {
        let api: Api<Secret> = Api::namespaced(self.client.clone(), &self.config.namespace);
        self.fence().await?;
        timeout(API_TIMEOUT, api.delete(name.as_str(), &Default::default()))
            .await
            .context("timeout while deleting backing secret for token")?
            .context("error deleting backing secret for token")
            .map(|_| ())
    }

    /// Ensure the given secret exists.
    #[tracing::instrument(level = "debug", skip(self, name))]
    async fn ensure_token_secret(&self, name: Arc<String>) -> Result<Secret> {
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
        let claims = TokenClaims::new(name.as_str());
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

        self.fence().await?;
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
    async fn fence(&self) -> Result<()> {
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
    labels.insert("hadron.rs/controlled-by".into(), "hadron-operator".into());
}
