use std::sync::Arc;
use std::time::Duration;

use k8s_openapi::api::core::v1::{Pod, Secret};
use kube::{Resource, ResourceExt};
use kube_runtime::watcher::Event;

use crate::crd::{Pipeline, Stream, Token};
use crate::k8s::events::CrdStateChange;
use crate::k8s::scheduler::SchedulerTask;
use crate::k8s::{Controller, EventResult};

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
        if stream.spec.cluster != self.config.cluster {
            return;
        }
        let name_str = match stream.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let name = match self.streams.get_key_value(name_str) {
            Some((key, old)) => {
                if old.as_ref() == &stream {
                    return;
                }
                Arc::clone(key) // No additional alloc.
            }
            None => Arc::new(name_str.clone()),
        };
        let stream = Arc::new(stream);
        let new_epoch = if let Some(old) = self.streams.insert(name.clone(), stream.clone()) {
            self.bookkeeping_stream_updated(old, stream.clone(), name.clone())
        } else {
            false
        };
        let _ = self.scheduler_tasks_tx.send(SchedulerTask::StreamUpdated(name.clone()));
        let _ = self
            .events_tx
            .send(CrdStateChange::StreamUpdated(name.clone(), stream.clone()))
            .await;
        if new_epoch {
            // For each pipeline of this stream, issue an update as pipelines mirror their
            // source stream's schedule.
            for pipeline in self
                .pipelines
                .values()
                .filter(|pipeline| &pipeline.spec.source_stream == name.as_ref())
            {
                let _ = self
                    .events_tx
                    .send(CrdStateChange::PipelineUpdated(name.clone(), pipeline.clone(), stream.clone()))
                    .await;
            }
        }
    }

    #[tracing::instrument(level = "debug", skip(self, stream))]
    async fn stream_deleted(&mut self, stream: Stream) {
        if stream.spec.cluster != self.config.cluster {
            return;
        }
        let name_str = match stream.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, stream) = match self.streams.remove_entry(name_str) {
            Some((name, stream)) => (name, stream),
            None => return,
        };
        self.bookkeeping_stream_deleted(stream.clone(), name.clone());
        let _ = self
            .scheduler_tasks_tx
            .send(SchedulerTask::StreamDeleted(name.clone(), stream.clone()));
        let _ = self
            .events_tx
            .send(CrdStateChange::StreamDeleted(name, stream.clone()))
            .await;
    }

    #[tracing::instrument(level = "debug", skip(self, streams))]
    async fn stream_restarted(&mut self, streams: Vec<Stream>) {
        for stream in streams {
            self.stream_applied(stream).await;
        }
    }

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
        if pipeline.spec.cluster != self.config.cluster {
            return;
        }
        let name_str = match pipeline.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let name = match self.pipelines.get_key_value(name_str) {
            Some((key, old)) => {
                if old.as_ref() == &pipeline {
                    return;
                }
                Arc::clone(key) // No additional alloc.
            }
            None => Arc::new(name_str.clone()),
        };
        let pipeline = Arc::new(pipeline);
        self.pipelines.insert(name.clone(), pipeline.clone());
        let _ = self.scheduler_tasks_tx.send(SchedulerTask::PipelineUpdated(name.clone()));
        if let Some(stream) = self.streams.get(&pipeline.spec.source_stream) {
            let _ = self
                .events_tx
                .send(CrdStateChange::PipelineUpdated(name, pipeline, stream.clone()))
                .await;
        }
    }

    #[tracing::instrument(level = "debug", skip(self, pipeline))]
    async fn pipeline_deleted(&mut self, pipeline: Pipeline) {
        if pipeline.spec.cluster != self.config.cluster {
            return;
        }
        let name_str = match pipeline.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, pipeline) = match self.pipelines.remove_entry(name_str) {
            Some((name, pipeline)) => (name, pipeline),
            None => return,
        };
        let _ = self
            .scheduler_tasks_tx
            .send(SchedulerTask::PipelineDeleted(name.clone(), pipeline.clone()));
        let _ = self
            .events_tx
            .send(CrdStateChange::PipelineDeleted(name.clone(), pipeline.clone()))
            .await;
    }

    #[tracing::instrument(level = "debug", skip(self, pipelines))]
    async fn pipeline_restarted(&mut self, pipelines: Vec<Pipeline>) {
        for pipeline in pipelines {
            self.pipeline_applied(pipeline).await;
        }
    }

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
                if old.as_ref() == &secret {
                    return;
                }
                Arc::clone(key) // No additional alloc.
            }
            None => Arc::new(name_str.clone()),
        };
        let secret = Arc::new(secret);
        self.secrets.insert(name.clone(), secret);
        let _ = self.scheduler_tasks_tx.send(SchedulerTask::SecretUpdated(name));
    }

    #[tracing::instrument(level = "debug", skip(self, secret))]
    async fn secret_deleted(&mut self, secret: Secret) {
        let name_str = match secret.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, secret) = match self.secrets.remove_entry(name_str) {
            Some((name, secret)) => (name, secret),
            None => return,
        };
        let _ = self.scheduler_tasks_tx.send(SchedulerTask::SecretDeleted(name, secret));
    }

    #[tracing::instrument(level = "debug", skip(self, secrets))]
    async fn secret_restarted(&mut self, secrets: Vec<Secret>) {
        for secret in secrets {
            self.secret_applied(secret).await;
        }
    }

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
        if token.spec.cluster != self.config.cluster {
            return;
        }
        let name_str = match token.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let name = match self.tokens.get_key_value(name_str) {
            Some((key, old)) => {
                if old.as_ref() == &token {
                    return;
                }
                Arc::clone(key) // No additional alloc.
            }
            None => Arc::new(name_str.clone()),
        };
        let token = Arc::new(token);
        self.tokens.insert(name.clone(), token.clone());
        let _ = self.scheduler_tasks_tx.send(SchedulerTask::TokenUpdated(name.clone()));
        let _ = self.events_tx.send(CrdStateChange::TokenUpdated(name, token)).await;
    }

    #[tracing::instrument(level = "debug", skip(self, token))]
    async fn token_deleted(&mut self, token: Token) {
        if token.spec.cluster != self.config.cluster {
            return;
        }
        let name_str = match token.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, token) = match self.tokens.remove_entry(name_str) {
            Some((name, token)) => (name, token),
            None => return,
        };
        let _ = self
            .scheduler_tasks_tx
            .send(SchedulerTask::TokenDeleted(name.clone(), token.clone()));
        let _ = self
            .events_tx
            .send(CrdStateChange::TokenDeleted(name.clone(), token.clone()))
            .await;
    }

    #[tracing::instrument(level = "debug", skip(self, tokens))]
    async fn token_restarted(&mut self, tokens: Vec<Token>) {
        for token in tokens {
            self.token_applied(token).await;
        }
    }

    /// Handle `Pod` watcher event.
    #[tracing::instrument(level = "debug", skip(self, res))]
    pub(super) async fn handle_pod_event(&mut self, res: EventResult<Pod>) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from Pod k8s watcher");
                let _ = tokio::time::sleep(Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(obj) => self.pod_applied(obj).await,
            Event::Deleted(obj) => self.pod_deleted(obj).await,
            Event::Restarted(objs) => self.pod_list_restarted(objs).await,
        }
    }

    #[tracing::instrument(level = "debug", skip(self, pod))]
    async fn pod_applied(&mut self, pod: Pod) {
        let name_str = match pod.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let name = match self.pods.get_key_value(name_str) {
            Some((key, _)) => Arc::clone(key), // No additional alloc.
            None => Arc::new(name_str.clone()),
        };
        let mut pod_info = self.pods.entry(name.clone()).or_insert_with(Default::default);
        if pod_info.pod_dns_name.is_none() {
            if let Some(subdomain) = pod.spec.as_ref().and_then(|spec| spec.subdomain.as_ref()) {
                pod_info.pod_dns_name = Some(format!("{}.{}.{}", pod.name(), subdomain, &self.config.namespace));
            }
        }
        pod_info.pod = Some(pod);
        let _ = self.scheduler_tasks_tx.send(SchedulerTask::PodUpdated(name));
    }

    /// Handle pod deletions.
    #[tracing::instrument(level = "debug", skip(self, pod))]
    async fn pod_deleted(&mut self, pod: Pod) {
        let name_str = match pod.meta().name.as_ref() {
            Some(name_str) => name_str,
            None => return, // Not actually possible as K8s requires name.
        };
        let (name, pod) = match self.pods.remove_entry(name_str) {
            Some((name, pod)) => (name, pod),
            None => return,
        };
        let _ = self.scheduler_tasks_tx.send(SchedulerTask::PodDeleted(name, pod));
    }

    /// Handle pod list restart; delegate calls to `pod_applied` handler.
    #[tracing::instrument(level = "debug", skip(self, pods))]
    async fn pod_list_restarted(&mut self, pods: Vec<Pod>) {
        for pod in pods {
            self.pod_applied(pod).await;
        }
    }

    /// Handle bookkeeping based on a newly observed stream epoch.
    ///
    /// This routine is not performing any scheduling updates. It is only updating our records,
    /// partly to ensure that the scheduler always has the most up-to-date info for its tasks.
    ///
    /// Returns `true` if a new epoch has been observed between the two given stream instances.
    fn bookkeeping_stream_updated(&mut self, old: Arc<Stream>, new: Arc<Stream>, name: Arc<String>) -> bool {
        // Check the epoch of both objects, if they are the same then this is a no-op.
        let (old_epoch, new_epoch) = (
            old.status.as_ref().map(|s| s.epoch).unwrap_or(0),
            new.status.as_ref().map(|s| s.epoch).unwrap_or(0),
        );
        if old_epoch == new_epoch {
            return false;
        }

        // Purge assignment info across all pods related to this stream's old data.
        if let Some(status) = old.status.as_ref() {
            status
                .partitions
                .iter()
                .filter(|prtn| prtn.is_scheduled())
                .for_each(|prtn| {
                    if let Some(leader) = prtn.leader() {
                        if let Some(pod) = self.pods.get_mut(leader) {
                            pod.stream_leaders.remove(&name);
                        }
                    }
                    if let Some(replicas) = prtn.replicas() {
                        replicas.iter().for_each(|repl| {
                            if let Some(pod) = self.pods.get_mut(repl) {
                                pod.stream_replicas.remove(&name);
                            }
                        });
                    }
                });
        }

        // Update assignment info across all pods related to this stream's new data.
        if let Some(status) = new.status.as_ref() {
            status
                .partitions
                .iter()
                .filter(|prtn| prtn.is_scheduled())
                .for_each(|prtn| {
                    if let Some(leader) = prtn.leader() {
                        if let Some(pod) = self.pods.get_mut(leader) {
                            pod.stream_leaders.insert(name.clone());
                        }
                    }
                    if let Some(replicas) = prtn.replicas() {
                        replicas.iter().for_each(|repl| {
                            if let Some(pod) = self.pods.get_mut(repl) {
                                *pod.stream_replicas.entry(name.clone()).or_insert(0) += 1;
                            }
                        });
                    }
                });
        }
        true
    }

    /// Handle bookkeeping based on a deleted stream.
    ///
    /// This routine is not performing any scheduling updates. It is only updating our records,
    /// partly to ensure that the scheduler always has the most up-to-date info for its tasks.
    fn bookkeeping_stream_deleted(&mut self, deleted_stream: Arc<Stream>, name: Arc<String>) {
        if let Some(status) = &deleted_stream.status {
            status
                .partitions
                .iter()
                .filter(|prtn| prtn.is_scheduled())
                .for_each(|prtn| {
                    if let Some(leader) = prtn.leader() {
                        if let Some(pod) = self.pods.get_mut(leader) {
                            pod.stream_leaders.remove(&name);
                        }
                    }
                    if let Some(replicas) = prtn.replicas() {
                        replicas.iter().for_each(|repl| {
                            if let Some(pod) = self.pods.get_mut(repl) {
                                pod.stream_replicas.remove(&name);
                            }
                        });
                    }
                });
        }
    }
}
