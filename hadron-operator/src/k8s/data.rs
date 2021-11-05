//! Data ingest logic for the controller.
//!
//! ## Important Implementation Notes
//! - When watcher streams restart, it means we could have stale data of objects which have been
//! deleted.
//! - As such, we diff the current data with the new data, and emit deletion events for stale data.
//! - We do not attempt to reduce scheduler load by checking the equality of data as it comes in.
//! This is because when a node is not the leader, it accepts data as is, and does not perform any
//! reconciliation. In cases where the cluster leader was down, this means we will have a build-up
//! of unreconciled data. Thus, when a leader comes to power, it will fetch a full update of data,
//! which ultimately will cause all objects to be fully reconciled.

use std::sync::Arc;
use std::time::Duration;

use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::{Secret, Service};
use kube::runtime::watcher::Event;
use kube::Resource;

use crate::k8s::scheduler::SchedulerTask;
use crate::k8s::{Controller, EventResult};
use hadron_core::crd::{Pipeline, Stream, Token};

//////////////////////////////////////////////////////////////////////////////
// Pipeline Events ///////////////////////////////////////////////////////////
impl Controller {
    /// Handle `Pipeline` watcher event.
    #[tracing::instrument(level = "debug", skip(self, res))]
    pub(super) async fn handle_pipeline_event(&mut self, res: EventResult<Pipeline>) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from Pipeline k8s watcher");
                let _ = tokio::time::sleep(Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(obj) => self.pipeline_applied(obj).await,
            Event::Deleted(obj) => self.pipeline_deleted(obj).await,
            Event::Restarted(objs) => self.pipeline_restarted(objs).await,
        }
    }

    #[tracing::instrument(level = "debug", skip(self, pipeline))]
    async fn pipeline_applied(&mut self, pipeline: Pipeline) {
        let name_str = match pipeline.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let name = match self.pipelines.get_key_value(name_str) {
            Some((key, _old)) => Arc::clone(key), // No additional alloc.
            None => Arc::new(name_str.clone()),
        };
        self.pipelines.insert(name.clone(), pipeline);
        self.spawn_scheduler_task(SchedulerTask::PipelineUpdated(name), false);
    }

    #[tracing::instrument(level = "debug", skip(self, pipeline))]
    async fn pipeline_deleted(&mut self, pipeline: Pipeline) {
        let name_str = match pipeline.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, pipeline) = match self.pipelines.remove_entry(name_str) {
            Some((name, pipeline)) => (name, pipeline),
            None => (Arc::new(name_str.clone()), pipeline),
        };
        self.spawn_scheduler_task(SchedulerTask::PipelineDeleted(name, pipeline), false);
    }

    #[tracing::instrument(level = "debug", skip(self, pipelines))]
    async fn pipeline_restarted(&mut self, pipelines: Vec<Pipeline>) {
        let old = self.pipelines.clone();
        for pipeline in pipelines {
            self.pipeline_applied(pipeline).await;
        }
        for (name, pipeline) in old {
            if !self.pipelines.contains_key(name.as_ref()) {
                self.pipeline_deleted(pipeline).await;
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Secret Events /////////////////////////////////////////////////////////////
impl Controller {
    /// Handle `Secret` watcher event.
    #[tracing::instrument(level = "debug", skip(self, res))]
    pub(super) async fn handle_secret_event(&mut self, res: EventResult<Secret>) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from Secret k8s watcher");
                let _ = tokio::time::sleep(Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(obj) => self.secret_applied(obj).await,
            Event::Deleted(obj) => self.secret_deleted(obj).await,
            Event::Restarted(objs) => self.secret_restarted(objs).await,
        }
    }

    #[tracing::instrument(level = "debug", skip(self, secret))]
    async fn secret_applied(&mut self, secret: Secret) {
        let name_str = match secret.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let name = match self.secrets.get_key_value(name_str) {
            Some((key, _old)) => Arc::clone(key), // No additional alloc.
            None => Arc::new(name_str.clone()),
        };
        self.secrets.insert(name.clone(), secret);
        self.spawn_scheduler_task(SchedulerTask::SecretUpdated(name), false);
    }

    #[tracing::instrument(level = "debug", skip(self, secret))]
    async fn secret_deleted(&mut self, secret: Secret) {
        let name_str = match secret.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, secret) = match self.secrets.remove_entry(name_str) {
            Some((name, secret)) => (name, secret),
            None => (Arc::new(name_str.clone()), secret),
        };
        self.spawn_scheduler_task(SchedulerTask::SecretDeleted(name, secret), false);
    }

    #[tracing::instrument(level = "debug", skip(self, secrets))]
    async fn secret_restarted(&mut self, secrets: Vec<Secret>) {
        let old = self.secrets.clone();
        for secret in secrets {
            self.secret_applied(secret).await;
        }
        for (name, secret) in old {
            if !self.secrets.contains_key(name.as_ref()) {
                self.secret_deleted(secret).await;
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Service Events ////////////////////////////////////////////////////////////
impl Controller {
    /// Handle `Service` watcher event.
    #[tracing::instrument(level = "debug", skip(self, res))]
    pub(super) async fn handle_service_event(&mut self, res: EventResult<Service>) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from Service k8s watcher");
                let _ = tokio::time::sleep(Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(obj) => self.service_applied(obj).await,
            Event::Deleted(obj) => self.service_deleted(obj).await,
            Event::Restarted(objs) => self.service_restarted(objs).await,
        }
    }

    #[tracing::instrument(level = "debug", skip(self, service))]
    async fn service_applied(&mut self, service: Service) {
        let name_str = match service.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let name = match self.services.get_key_value(name_str) {
            Some((key, _old)) => Arc::clone(key), // No additional alloc.
            None => Arc::new(name_str.clone()),
        };
        self.services.insert(name.clone(), service);
        self.spawn_scheduler_task(SchedulerTask::ServiceUpdated(name), false);
    }

    #[tracing::instrument(level = "debug", skip(self, service))]
    async fn service_deleted(&mut self, service: Service) {
        let name_str = match service.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, service) = match self.services.remove_entry(name_str) {
            Some((name, service)) => (name, service),
            None => (Arc::new(name_str.clone()), service),
        };
        self.spawn_scheduler_task(SchedulerTask::ServiceDeleted(name, service), false);
    }

    #[tracing::instrument(level = "debug", skip(self, services))]
    async fn service_restarted(&mut self, services: Vec<Service>) {
        let old = self.services.clone();
        for service in services {
            self.service_applied(service).await;
        }
        for (name, service) in old {
            if !self.services.contains_key(name.as_ref()) {
                self.service_deleted(service).await;
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// StatefulSet Events ////////////////////////////////////////////////////////
impl Controller {
    /// Handle `Stream` watcher event.
    #[tracing::instrument(level = "debug", skip(self, res))]
    pub(super) async fn handle_sts_event(&mut self, res: EventResult<StatefulSet>) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from StatefulSet k8s watcher");
                let _ = tokio::time::sleep(Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(obj) => self.sts_applied(obj).await,
            Event::Deleted(obj) => self.sts_deleted(obj).await,
            Event::Restarted(objs) => self.sts_restarted(objs).await,
        }
    }

    #[tracing::instrument(level = "debug", skip(self, set))]
    pub(super) async fn sts_applied(&mut self, set: StatefulSet) {
        let name_str = match set.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let name = match self.statefulsets.get_key_value(name_str) {
            Some((key, _old)) => Arc::clone(key), // No additional alloc.
            None => Arc::new(name_str.clone()),
        };
        self.statefulsets.insert(name.clone(), set);
        self.spawn_scheduler_task(SchedulerTask::StatefulSetUpdated(name), false);
    }

    #[tracing::instrument(level = "debug", skip(self, set))]
    async fn sts_deleted(&mut self, set: StatefulSet) {
        let name_str = match set.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, set) = match self.statefulsets.remove_entry(name_str) {
            Some((name, set)) => (name, set),
            None => (Arc::new(name_str.clone()), set),
        };
        self.spawn_scheduler_task(SchedulerTask::StatefulSetDeleted(name, set), false);
    }

    #[tracing::instrument(level = "debug", skip(self, sets))]
    async fn sts_restarted(&mut self, sets: Vec<StatefulSet>) {
        let old = self.statefulsets.clone();
        for set in sets {
            self.sts_applied(set).await;
        }
        for (name, sts) in old {
            if !self.statefulsets.contains_key(name.as_ref()) {
                self.sts_deleted(sts).await;
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Stream Events /////////////////////////////////////////////////////////////
impl Controller {
    /// Handle `Stream` watcher event.
    #[tracing::instrument(level = "debug", skip(self, res))]
    pub(super) async fn handle_stream_event(&mut self, res: EventResult<Stream>) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from Stream k8s watcher");
                let _ = tokio::time::sleep(Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(obj) => self.stream_applied(obj).await,
            Event::Deleted(obj) => self.stream_deleted(obj).await,
            Event::Restarted(objs) => self.stream_restarted(objs).await,
        }
    }

    #[tracing::instrument(level = "debug", skip(self, stream))]
    pub(super) async fn stream_applied(&mut self, stream: Stream) {
        let name_str = match stream.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let name = match self.streams.get_key_value(name_str) {
            Some((key, _old)) => Arc::clone(key), // No additional alloc.
            None => Arc::new(name_str.clone()),
        };
        self.streams.insert(name.clone(), stream);
        self.spawn_scheduler_task(SchedulerTask::StreamUpdated(name), false);
    }

    #[tracing::instrument(level = "debug", skip(self, stream))]
    async fn stream_deleted(&mut self, stream: Stream) {
        let name_str = match stream.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, stream) = match self.streams.remove_entry(name_str) {
            Some((name, stream)) => (name, stream),
            None => (Arc::new(name_str.clone()), stream),
        };
        self.spawn_scheduler_task(SchedulerTask::StreamDeleted(name, stream), false);
    }

    #[tracing::instrument(level = "debug", skip(self, streams))]
    async fn stream_restarted(&mut self, streams: Vec<Stream>) {
        let old = self.streams.clone();
        for stream in streams {
            self.stream_applied(stream).await;
        }
        for (name, stream) in old {
            if !self.streams.contains_key(name.as_ref()) {
                self.stream_deleted(stream).await;
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Token Events //////////////////////////////////////////////////////////////
impl Controller {
    /// Handle `Token` watcher event.
    #[tracing::instrument(level = "debug", skip(self, res))]
    pub(super) async fn handle_token_event(&mut self, res: EventResult<Token>) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from Token k8s watcher");
                let _ = tokio::time::sleep(Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(obj) => self.token_applied(obj).await,
            Event::Deleted(obj) => self.token_deleted(obj).await,
            Event::Restarted(objs) => self.token_restarted(objs).await,
        }
    }

    #[tracing::instrument(level = "debug", skip(self, token))]
    async fn token_applied(&mut self, token: Token) {
        let name_str = match token.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let name = match self.tokens.get_key_value(name_str) {
            Some((key, _old)) => Arc::clone(key), // No additional alloc.
            None => Arc::new(name_str.clone()),
        };
        self.tokens.insert(name.clone(), token);
        self.spawn_scheduler_task(SchedulerTask::TokenUpdated(name), false);
    }

    #[tracing::instrument(level = "debug", skip(self, token))]
    async fn token_deleted(&mut self, token: Token) {
        let name_str = match token.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, token) = match self.tokens.remove_entry(name_str) {
            Some((name, token)) => (name, token),
            None => (Arc::new(name_str.clone()), token),
        };
        self.spawn_scheduler_task(SchedulerTask::TokenDeleted(name, token), false);
    }

    #[tracing::instrument(level = "debug", skip(self, tokens))]
    async fn token_restarted(&mut self, tokens: Vec<Token>) {
        let old = self.tokens.clone();
        for token in tokens {
            self.token_applied(token).await;
        }
        for (name, token) in old {
            if !self.tokens.contains_key(name.as_ref()) {
                self.token_deleted(token).await;
            }
        }
    }
}
