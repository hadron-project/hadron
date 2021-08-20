use std::sync::Arc;
use std::time::Duration;

use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::{Secret, Service};
use kube::Resource;
use kube_runtime::watcher::Event;

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
            Some((key, old)) => {
                if old == &pipeline {
                    return;
                }
                Arc::clone(key) // No additional alloc.
            }
            None => Arc::new(name_str.clone()),
        };
        self.pipelines.insert(name.clone(), pipeline);
        self.spawn_scheduler_task_pub(SchedulerTask::PipelineUpdated(name));
    }

    #[tracing::instrument(level = "debug", skip(self, pipeline))]
    async fn pipeline_deleted(&mut self, pipeline: Pipeline) {
        let name_str = match pipeline.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, _pipeline) = match self.pipelines.remove_entry(name_str) {
            Some((name, pipeline)) => (name, pipeline),
            None => return,
        };
        self.spawn_scheduler_task_pub(SchedulerTask::PipelineDeleted(name, pipeline));
    }

    #[tracing::instrument(level = "debug", skip(self, pipelines))]
    async fn pipeline_restarted(&mut self, pipelines: Vec<Pipeline>) {
        for pipeline in pipelines {
            self.pipeline_applied(pipeline).await;
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
            Some((key, old)) => {
                if old == &secret {
                    return;
                }
                Arc::clone(key) // No additional alloc.
            }
            None => Arc::new(name_str.clone()),
        };
        self.secrets.insert(name.clone(), secret);
        self.spawn_scheduler_task_pub(SchedulerTask::SecretUpdated(name));
    }

    #[tracing::instrument(level = "debug", skip(self, secret))]
    async fn secret_deleted(&mut self, secret: Secret) {
        let name_str = match secret.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, _secret) = match self.secrets.remove_entry(name_str) {
            Some((name, secret)) => (name, secret),
            None => return,
        };
        self.spawn_scheduler_task_pub(SchedulerTask::SecretDeleted(name, secret));
    }

    #[tracing::instrument(level = "debug", skip(self, secrets))]
    async fn secret_restarted(&mut self, secrets: Vec<Secret>) {
        for secret in secrets {
            self.secret_applied(secret).await;
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
            Some((key, old)) => {
                if old == &service {
                    return;
                }
                Arc::clone(key) // No additional alloc.
            }
            None => Arc::new(name_str.clone()),
        };
        self.services.insert(name.clone(), service);
        self.spawn_scheduler_task_pub(SchedulerTask::ServiceUpdated(name));
    }

    #[tracing::instrument(level = "debug", skip(self, service))]
    async fn service_deleted(&mut self, service: Service) {
        let name_str = match service.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, _service) = match self.services.remove_entry(name_str) {
            Some((name, service)) => (name, service),
            None => return,
        };
        self.spawn_scheduler_task_pub(SchedulerTask::ServiceDeleted(name, service));
    }

    #[tracing::instrument(level = "debug", skip(self, services))]
    async fn service_restarted(&mut self, services: Vec<Service>) {
        for service in services {
            self.service_applied(service).await;
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
            Some((key, old)) => {
                if old == &set {
                    return;
                }
                Arc::clone(key) // No additional alloc.
            }
            None => Arc::new(name_str.clone()),
        };
        self.statefulsets.insert(name.clone(), set);
        self.spawn_scheduler_task_pub(SchedulerTask::StatefulSetUpdated(name));
    }

    #[tracing::instrument(level = "debug", skip(self, set))]
    async fn sts_deleted(&mut self, set: StatefulSet) {
        let name_str = match set.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, _set) = match self.statefulsets.remove_entry(name_str) {
            Some((name, set)) => (name, set),
            None => return,
        };
        self.spawn_scheduler_task_pub(SchedulerTask::StatefulSetDeleted(name, set));
    }

    #[tracing::instrument(level = "debug", skip(self, sets))]
    async fn sts_restarted(&mut self, sets: Vec<StatefulSet>) {
        for set in sets {
            self.sts_applied(set).await;
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
            Some((key, old)) => {
                if old == &stream {
                    return;
                }
                Arc::clone(key) // No additional alloc.
            }
            None => Arc::new(name_str.clone()),
        };
        self.streams.insert(name.clone(), stream);
        self.spawn_scheduler_task_pub(SchedulerTask::StreamUpdated(name));
    }

    #[tracing::instrument(level = "debug", skip(self, stream))]
    async fn stream_deleted(&mut self, stream: Stream) {
        let name_str = match stream.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, _stream) = match self.streams.remove_entry(name_str) {
            Some((name, stream)) => (name, stream),
            None => return,
        };
        self.spawn_scheduler_task_pub(SchedulerTask::StreamDeleted(name, stream));
    }

    #[tracing::instrument(level = "debug", skip(self, streams))]
    async fn stream_restarted(&mut self, streams: Vec<Stream>) {
        for stream in streams {
            self.stream_applied(stream).await;
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
            Some((key, old)) => {
                if old == &token {
                    return;
                }
                Arc::clone(key) // No additional alloc.
            }
            None => Arc::new(name_str.clone()),
        };
        self.tokens.insert(name.clone(), token);
        self.spawn_scheduler_task_pub(SchedulerTask::TokenUpdated(name));
    }

    #[tracing::instrument(level = "debug", skip(self, token))]
    async fn token_deleted(&mut self, token: Token) {
        let name_str = match token.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, _token) = match self.tokens.remove_entry(name_str) {
            Some((name, token)) => (name, token),
            None => return,
        };
        self.spawn_scheduler_task_pub(SchedulerTask::TokenDeleted(name, token));
    }

    #[tracing::instrument(level = "debug", skip(self, tokens))]
    async fn token_restarted(&mut self, tokens: Vec<Token>) {
        for token in tokens {
            self.token_applied(token).await;
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Utility Methods ///////////////////////////////////////////////////////////
impl Controller {
    /// Spawn a task which emits a new scheduler tasks.
    ///
    /// This indirection is used to ensure that we don't use an unlimited amount of memory with an
    /// unbounded queue, and also so that we do not block the controller from making progress and
    /// dead-locking when we hit the scheduler task queue cap.
    ///
    /// The runtime will stack up potentially lots of tasks, and memory will be consumed that way,
    /// but ultimately the controller will be able to begin processing scheduler tasks and will
    /// drain the scheduler queue and ultimately relieve the memory pressure of the tasks.
    fn spawn_scheduler_task_pub(&self, task: SchedulerTask) {
        let tx = self.scheduler_tasks_tx.clone();
        tokio::spawn(async move {
            let _res = tx.send(task).await;
        });
    }
}
