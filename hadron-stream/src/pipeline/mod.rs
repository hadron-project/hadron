//! Pipeline controller.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::StreamExt;
use prost::Message;
use rand::seq::IteratorRandom;
use sled::Tree;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::{
    wrappers::{BroadcastStream, ReceiverStream, WatchStream},
    StreamMap,
};
use tonic::{Status, Streaming};
use uuid::Uuid;

use crate::config::Config;
use crate::database::Database;
use crate::error::{RpcResult, ShutdownError, ShutdownResult, ERR_DB_FLUSH, ERR_ITER_FAILURE};
use crate::futures::LivenessStream;
use crate::grpc::{Event, PipelineSubscribeRequest, PipelineSubscribeRequestAction, PipelineSubscribeResponse};
use crate::stream::PREFIX_STREAM_EVENT;
use crate::utils;
use hadron_core::crd::{Pipeline, PipelineStage, PipelineStartPointLocation, RequiredMetadata};

const DEFAULT_MAX_PARALLEL: u32 = 50;
/// A pipeline metadata key used to track the last offset of the source stream to have been
/// transformed into a pipeline instance for pipeline processing.
///
/// NOTE: this does not necessarily indicate that the pipeline for this key has actually been
/// executed, but only that it has been prepared for execution.
const KEY_LAST_OFFSET_PROCESSED: &[u8; 1] = b"l";
/// A key prefix used for tracking active pipeline instances.
///
/// Active instances are keyed as `a{offset}` where `{offset}` is the offset
/// of the event from the source stream. The value is the corresponding root event.
const PREFIX_ACTIVE_INSTANCES: &[u8; 1] = b"a";
/// The key prefix under which pipeline stage outputs are stored.
///
/// Outputs are keyed as `o{offset}{stage_name}`.
const PREFIX_PIPELINE_STAGE_OUTPUTS: &[u8; 1] = b"o";

/// The liveness stream type used by a pipeline controller.
type PipelineLivenessStream = LivenessStream<RpcResult<PipelineSubscribeResponse>, PipelineSubscribeRequest>;
/// The client channel type used by this controller.
type ClientChannel = (mpsc::Sender<RpcResult<PipelineSubscribeResponse>>, Streaming<PipelineSubscribeRequest>);

/// A pipeline controller for managing a pipeline.
pub struct PipelineCtl {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    _db: Database,
    /// The database tree for storing this pipeline's instance records.
    tree: Tree,
    /// The database tree of this pipeline's source stream; which is only ever used for reading.
    tree_stream: Tree,
    /// The data model of the pipeline with which this controller is associated.
    pipeline: Arc<Pipeline>,
    /// The pipeline partition of this controller.
    partition: u32,

    /// A channel of inbound client requests.
    events_tx: mpsc::Sender<PipelineCtlMsg>,
    /// A channel of inbound client requests.
    events_rx: ReceiverStream<PipelineCtlMsg>,
    /// A mapping of all active pipeline instances.
    active_pipelines: BTreeMap<u64, ActivePipelineInstance>,
    /// A mapping of stage subscribers by stage name.
    stage_subs: HashMap<Arc<String>, SubscriptionGroup>,
    /// A stream of liveness checks on the active subscriber channels.
    liveness_checks: StreamMap<Uuid, PipelineLivenessStream>,

    /// A signal of the parent stream's last written offset.
    stream_signal: WatchStream<u64>,
    /// The last known offset of the input stream.
    stream_offset: u64,
    /// The last offset of the source stream to have been processed for pipline instantiation.
    last_offset_processed: u64,
    /// A bool indicating if data is currently being fetched from the input stream.
    is_fetching_stream_data: bool,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
    /// A bool indicating that this controller has been descheduled and needs to shutdown.
    descheduled: bool,
}

impl PipelineCtl {
    /// Create a new instance.
    pub async fn new(
        config: Arc<Config>, db: Database, pipeline: Arc<Pipeline>, partition: u32, stream_signal: watch::Receiver<u64>, shutdown_tx: broadcast::Sender<()>, events_tx: mpsc::Sender<PipelineCtlMsg>,
        events_rx: mpsc::Receiver<PipelineCtlMsg>,
    ) -> Result<Self> {
        let tree = db.get_pipeline_tree(pipeline.name()).await?;
        let stream_tree = db.get_stream_tree().await?;
        let stream_offset = *stream_signal.borrow();
        let (last_offset_processed, active_pipelines) = recover_pipeline_state(tree.clone(), pipeline.clone(), stream_offset).await?;

        Ok(Self {
            config,
            _db: db,
            tree,
            tree_stream: stream_tree,
            pipeline,
            partition,
            events_tx,
            events_rx: ReceiverStream::new(events_rx),
            active_pipelines,
            stage_subs: Default::default(),
            liveness_checks: Default::default(),
            stream_signal: WatchStream::new(stream_signal),
            stream_offset,
            last_offset_processed,
            is_fetching_stream_data: false,
            shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
            shutdown_tx,
            descheduled: false,
        })
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!(
            last_offset_processed = self.last_offset_processed,
            "pipeline controller {}/{}/{} has started",
            self.config.stream,
            self.partition,
            self.pipeline.name()
        );

        // Check for active pipelines which need to be removed.
        let (events_tx, pipeline) = (self.events_tx.clone(), &self.pipeline);
        self.active_pipelines.retain(|offset, inst| {
            if pipeline.spec.stages.iter().all(|stage| inst.outputs.contains_key(&stage.name)) {
                tracing::debug!(offset, "pruning old finished pipeline instance");
                let (events_tx, offset) = (events_tx.clone(), *offset);
                tokio::spawn(async move {
                    let _res = events_tx.send(PipelineCtlMsg::PipelineInstanceComplete(offset)).await;
                });
                false
            } else {
                true
            }
        });

        loop {
            if self.descheduled {
                break;
            }
            tokio::select! {
                msg_opt = self.events_rx.next() => self.handle_pipeline_msg(msg_opt).await,
                Some(dead_chan) = self.liveness_checks.next() => self.handle_dead_subscriber(dead_chan.1).await,
                Some(offset) = self.stream_signal.next() => self.handle_input_stream_offset_update(offset).await,
                _ = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        tracing::debug!(
            last_offset_processed = self.last_offset_processed,
            "pipeline controller {}/{}/{} has shutdown",
            self.config.stream,
            self.partition,
            self.pipeline.name()
        );
        Ok(())
    }

    /// A getter for the Pipeline's max parallel config, defaulting to `DEFAULT_MAX_PARALLEL` if set to `0`.
    fn max_parallel(&self) -> u32 {
        if self.pipeline.spec.max_parallel == 0 {
            DEFAULT_MAX_PARALLEL
        } else {
            self.pipeline.spec.max_parallel
        }
    }

    /// Handle any dead channels.
    #[tracing::instrument(level = "trace", skip(self, dead_chan))]
    async fn handle_dead_subscriber(&mut self, dead_chan: (Arc<String>, Uuid, ClientChannel)) {
        let (stage_name, id, _chan) = (dead_chan.0, dead_chan.1, dead_chan.2);
        tracing::debug!(?id, group_name = ?&*stage_name, "dropping pipeline stage subscriber channel");
        self.liveness_checks.remove(&id);
        let mut group = match self.stage_subs.remove(&*stage_name) {
            Some(group) => group,
            None => return,
        };
        group.active_channels.remove(&id);
        if !group.active_channels.is_empty() {
            self.stage_subs.insert(group.stage_name.clone(), group);
        }
    }

    /// Handle an update from the input stream indicating that new data is available.
    #[tracing::instrument(level = "trace", skip(self, offset))]
    async fn handle_input_stream_offset_update(&mut self, offset: u64) {
        // Track the offset update.
        self.stream_offset = offset;
        // Only fetch new data if we are not already at capacity.
        if self.active_pipelines.len() >= self.max_parallel() as usize {
            return;
        }
        // Fetch new data from the input stream & create new pipeline instances if triggers match.
        if !self.is_fetching_stream_data {
            self.fetch_stream_data();
        }
    }

    /// Handle a pipeline controller message.
    #[tracing::instrument(level = "trace", skip(self, msg_opt))]
    async fn handle_pipeline_msg(&mut self, msg_opt: Option<PipelineCtlMsg>) {
        let msg = match msg_opt {
            Some(msg) => msg,
            None => {
                self.descheduled = true;
                return;
            }
        };
        match msg {
            PipelineCtlMsg::Request { tx, rx, stage_name } => self.handle_request((tx, rx), stage_name).await,
            PipelineCtlMsg::FetchStreamRecords(res) => self.handle_fetch_stream_records_response(res).await,
            PipelineCtlMsg::DeliveryResponse(res) => self.handle_delivery_response(res).await,
            PipelineCtlMsg::PipelineUpdated(pipeline) => self.handle_pipeline_updated(pipeline),
            PipelineCtlMsg::PipelineDeleted(pipeline) => self.handle_pipeline_deleted(pipeline),
            PipelineCtlMsg::PipelineInstanceComplete(offset) => self.handle_pipeline_instance_complete(offset).await,
        }
    }

    /// Handle an update to the pipeline model.
    ///
    /// ## Adding New Stages
    /// - Adding new stages to a pipeline will cause no negative effect, and as soon as subscribers
    /// are online and able to handle the new stages, the system will converge and the new stages
    /// will be processed.
    /// - In order to ensure new stages being added does not cause processing to be eventually
    /// halted due to unfished stages across multiple pipelines, it is recommended that clients
    /// be updated and deployed before the new stages are added. Clients will be rejected until
    /// the new stages are added, but clients can implement simple backoff algorithms to ease the
    /// transitional phase.
    ///
    /// ## Changing Dependencies
    /// - No expected negative side-effects here given the assumption of a valid pipeline object.
    ///
    /// ## Removing Stages
    /// - Data loss will be incurred when stages are removed. The VAW blocks this unless it is
    /// expressly requested by the user.
    /// - When a stage is removed, we will prune the corresponding subscription group for that stage.
    #[tracing::instrument(level = "trace", skip(self, new))]
    fn handle_pipeline_updated(&mut self, new: Arc<Pipeline>) {
        self.pipeline = new.clone();
        self.stage_subs.retain(|group, _| new.spec.stages.iter().any(|stage| stage.name.as_str() == group.as_str()));
    }

    #[tracing::instrument(level = "trace", skip(self, new))]
    fn handle_pipeline_deleted(&mut self, new: Arc<Pipeline>) {
        self.pipeline = new;
        self.descheduled = true;
    }

    /// Handle a response from fetching records from the input stream for pipeline instance creation.
    #[tracing::instrument(level = "trace", skip(self, res))]
    async fn handle_fetch_stream_records_response(&mut self, res: ShutdownResult<FetchStreamRecords>) {
        tracing::debug!("handling fetch stream records response");
        self.is_fetching_stream_data = false;
        let data = match res {
            Ok(data) => data,
            Err(err) => {
                tracing::error!(error = ?err, "error fetching input stream data for pipeline");
                let _ = self.shutdown_tx.send(());
                return;
            }
        };
        self.last_offset_processed = data.last_offset_processed;
        tracing::debug!(data.last_offset_processed, "response from pipeline data fetch");
        self.active_pipelines.extend(data.new_pipeline_instances.into_iter().map(|inst| (inst.root_event_offset, inst)));

        // Drive another delivery pass.
        self.execute_delivery_pass().await;
    }

    /// Handle a pipeline stage delivery response.
    #[tracing::instrument(level = "trace", skip(self, res))]
    async fn handle_delivery_response(&mut self, res: DeliveryResponse) {
        tracing::debug!("received pipeline stage delivery response");
        if let Err(err) = self.try_handle_delivery_response(res).await {
            tracing::error!(error = ?err, "error handling pipeline subscriber ack/nack response");
            if err.downcast_ref::<ShutdownError>().is_some() {
                let _ = self.shutdown_tx.send(());
                return;
            }
        }
        // Drive another delivery pass.
        self.execute_delivery_pass().await;
    }

    /// Handle a request which has been sent to this controller.
    #[tracing::instrument(level = "trace", skip(self, tx, rx))]
    async fn handle_request(&mut self, (tx, rx): ClientChannel, stage_name: String) {
        tracing::debug!("request received on pipeline controller");
        // Validate contents of setup request.
        if !self.pipeline.spec.stages.iter().any(|stage| stage.name == stage_name) {
            let _res = tx
                .send(Err(Status::invalid_argument(format!(
                    "pipeline subscriber stage name '{}' is unknown for pipeline {}",
                    stage_name,
                    self.pipeline.name(),
                ))))
                .await;
            return;
        }

        // Add the new stage subscriber to its corresponding group.
        let stage_name = Arc::new(stage_name.clone());
        let group = self.stage_subs.entry(stage_name.clone()).or_insert_with(|| SubscriptionGroup::new(stage_name.clone()));

        // Roll a new ID for the channel & add it to the group's active channels.
        let id = Uuid::new_v4();
        group.active_channels.insert(id, SubChannelState::MonitoringLiveness);
        self.liveness_checks.insert(
            id,
            LivenessStream {
                chan: Some((tx, rx)),
                chan_id: id,
                group: stage_name,
            },
        );
        self.execute_delivery_pass().await;
    }

    /// Handle an event indicating that the pipeline instance at the given
    /// offset is ready to be deleted.
    #[tracing::instrument(level = "trace", skip(self))]
    async fn handle_pipeline_instance_complete(&mut self, offset: u64) {
        // Build up delete op.
        let (tree, mut batch) = (self.tree.clone(), sled::Batch::default());
        batch.remove(&utils::encode_byte_prefix(PREFIX_ACTIVE_INSTANCES, offset));
        for stage_name in self.pipeline.spec.stages.iter().map(|stage| stage.name.as_str()) {
            let key = utils::ivec_from_iter(
                PREFIX_PIPELINE_STAGE_OUTPUTS
                    .iter()
                    .copied()
                    .chain(utils::encode_u64(offset))
                    .chain(stage_name.as_bytes().iter().copied()),
            );
            batch.remove(key);
        }

        // Apply batch.
        let res = Database::spawn_blocking(move || -> Result<()> {
            tree.apply_batch(batch).context("error applying batch delete on finished pipeline")?;
            tree.flush().context(ERR_DB_FLUSH)?;
            Ok(())
        })
        .await
        .map_err(ShutdownError::from)
        .and_then(|res| res.map_err(ShutdownError::from));

        // Shutdown if needed.
        if let Err(err) = res {
            tracing::error!(error = ?err, "error deleting finished pipeline data, shutting down");
            let _ = self.shutdown_tx.send(());
        }
    }

    /// Execute a loop over all active pipeline stage subscription groups, delivering data if possible.
    ///
    /// ### Notes
    /// - The database page cache will stay hot when subscribers are running line-rate. As such,
    ///   cache updates will stay in memory for up-to-date subscriptions.
    #[tracing::instrument(level = "trace", skip(self))]
    async fn execute_delivery_pass(&mut self) {
        tracing::debug!("executing pipeline delivery pass");
        if let Err(err) = self.try_execute_delivery_pass().await {
            tracing::error!(error = ?err, "error during pipeline delivery pass");
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn try_execute_delivery_pass(&mut self) -> Result<()> {
        for inst in self.active_pipelines.values_mut() {
            for stage in self.pipeline.spec.stages.iter() {
                // Skip stages with an active delivery.
                if inst.active_deliveries.contains_key(&stage.name) {
                    continue;
                }
                // Skip stages which have already been complete.
                if inst.outputs.contains_key(&stage.name) {
                    continue;
                }
                // Skip stages which do not have all dependencies met.
                if !stage.dependencies.iter().all(|dep| inst.outputs.contains_key(dep)) {
                    continue;
                }
                // Skip stages which must be ordered later.
                if !stage.after.iter().all(|dep| inst.outputs.contains_key(dep)) {
                    continue;
                }
                // Get a handle to the stage subs.
                let sub_group = match self.stage_subs.get_mut(&stage.name) {
                    Some(stage) => stage,
                    None => continue, // Just skip.
                };

                // Create a delivery payload and send it to a randomly selected subscriber.
                Self::spawn_payload_delivery(stage, inst, sub_group, &mut self.liveness_checks, self.events_tx.clone()).await?;
            }
        }

        // If number of active instances is < the pipeline's max parallel settings, then fetch
        // more data from the source stream to find matching records for pipeline instantiation.
        if self.active_pipelines.len() < self.max_parallel() as usize && self.stream_offset > self.last_offset_processed && !self.is_fetching_stream_data {
            tracing::info!(
                active_pipelines = self.active_pipelines.len(),
                stream_offset = self.stream_offset,
                last_offset_processed = self.last_offset_processed,
                "executing fetch_stream_data from try_execute_delivery_pass"
            );
            self.fetch_stream_data();
        }
        Ok(())
    }

    /// Deliver a payload to a stage consumer & spawn a task to await its response.
    #[tracing::instrument(level = "debug", skip(stage, inst, sub_group, liveness_checks, events_tx))]
    async fn spawn_payload_delivery(
        stage: &PipelineStage, inst: &mut ActivePipelineInstance, sub_group: &mut SubscriptionGroup, liveness_checks: &mut StreamMap<Uuid, PipelineLivenessStream>,
        events_tx: mpsc::Sender<PipelineCtlMsg>,
    ) -> Result<()> {
        // Randomly select one of the available subscriptions for the stage.
        let chan_key_opt = sub_group
            .active_channels
            .iter()
            .filter(|(_, val)| matches!(val, SubChannelState::MonitoringLiveness))
            .choose(&mut rand::thread_rng())
            .map(|(key, _val)| *key);
        let chan_id = match chan_key_opt {
            Some(chan_key) => chan_key,
            None => return Ok(()), // This would only mean that all channels are busy.
        };
        let _old_state = match sub_group.active_channels.remove(&chan_id) {
            Some(chan_data) => chan_data,
            None => return Ok(()), // Unreachable.
        };
        let mut chan = match liveness_checks.remove(&chan_id) {
            Some(chan_opt) => match chan_opt.chan {
                Some(chan) => chan,
                None => return Ok(()),
            },
            None => {
                tracing::error!(?chan_id, "pipeline subscription channel was not properly held in liveness stream");
                return Ok(());
            }
        };

        // Accumulate delivery payload with all needed inputs.
        let mut payload = PipelineSubscribeResponse {
            stage: stage.name.clone(),
            root_event: Some(inst.root_event.clone()),
            inputs: Default::default(),
        };
        stage.dependencies.iter().for_each(|dep| match inst.outputs.get(dep) {
            Some(input) => {
                payload.inputs.insert(dep.clone(), input.clone());
            }
            None => tracing::error!("bug: failed to accumulate stage dependencies even though all were accounted for"),
        });

        // Payload is ready, send it.
        let _res = chan.0.send(Ok(payload)).await.context("error sending pipeline delivery payload")?;

        // Spawn off a task to await the response from the client.
        inst.active_deliveries.insert(sub_group.stage_name.clone(), chan_id);
        sub_group.active_channels.insert(chan_id, SubChannelState::OutForDelivery);
        let (tx, group_name, id, offset) = (events_tx, sub_group.stage_name.clone(), chan_id, inst.root_event_offset);
        tokio::spawn(async move {
            // FUTURE: add optional timeouts here based on pipeline config.
            let output = chan.1.next().await.map(|res| res.map_err(anyhow::Error::from).map(|data| (chan, data)));
            let _ = tx
                .send(PipelineCtlMsg::DeliveryResponse(DeliveryResponse {
                    id,
                    offset,
                    stage_name: group_name,
                    output,
                }))
                .await;
        });
        Ok(())
    }

    /// Handle a pipeline stage delivery response.
    #[tracing::instrument(level = "trace", skip(self, res))]
    async fn try_handle_delivery_response(&mut self, res: DeliveryResponse) -> Result<()> {
        // Unpack basic info of res & remove this stage from the pipeline instance's active deliveries.
        let (chan_id, stage_name, offset) = (&res.id, &*res.stage_name, res.offset);
        let inst = self
            .active_pipelines
            .get_mut(&offset)
            .context("response from pipeline subscription delivery dropped as pipeline instance is no longer active")?;
        inst.active_deliveries.remove(stage_name);

        // Get a mutable handle to subscription group to which this response applies.
        let group = self
            .stage_subs
            .get_mut(stage_name)
            .context("response from pipeline subscription delivery dropped as stage subscription group no longer exists")?;

        // Unpack response body.
        let res = res.output.context("subscriber channel closed while awaiting delivery response").map_err(|err| {
            let _ = group.active_channels.remove(chan_id);
            err
        })?;
        let (client_chan, body) = res.context("error returned while awaiting subscriber delivery response").map_err(|err| {
            let _ = group.active_channels.remove(chan_id);
            err
        })?;
        if let Some(chan_wrapper) = group.active_channels.get_mut(chan_id) {
            *chan_wrapper = SubChannelState::MonitoringLiveness;
            self.liveness_checks.insert(
                *chan_id,
                LivenessStream {
                    chan: Some(client_chan),
                    group: group.stage_name.clone(),
                    chan_id: *chan_id,
                },
            );
        }

        // Decode the response body and record the result.
        let record_res = match body.action {
            Some(PipelineSubscribeRequestAction::Ack(output)) => {
                let output = output.output.unwrap_or_default();
                inst.outputs.insert(stage_name.clone(), output.clone());
                Ok(output)
            }
            Some(PipelineSubscribeRequestAction::Nack(err)) => Err(err),
            _ => Err("malformed response returned from pipeline subscriber, unknown result variant".into()),
        };
        Self::try_record_delivery_response(record_res, offset, group.stage_name.clone(), self.tree.clone())
            .await
            .context("error while recording subscriber delivery response")?;

        // Finally, if this was the last outstanding stage of the pipeline instance, then remove
        // it from the active instances set.
        if self.pipeline.spec.stages.iter().all(|stage| inst.outputs.contains_key(&stage.name)) {
            self.active_pipelines.remove(&offset);
            tracing::debug!(offset, "pipeline workflow finished");
            let events_tx = self.events_tx.clone();
            tokio::spawn(async move {
                let _res = events_tx.send(PipelineCtlMsg::PipelineInstanceComplete(offset)).await;
            });
        }

        Ok(())
    }

    /// Record the ack/nack response from a subscriber delivery.
    #[tracing::instrument(level = "trace", skip(res, offset, stage_name, pipeline_tree))]
    async fn try_record_delivery_response(res: std::result::Result<Event, String>, offset: u64, stage_name: Arc<String>, pipeline_tree: Tree) -> ShutdownResult<()> {
        let event = match res {
            Ok(event) => event,
            Err(_err) => {
                // FUTURE: record this error info for observability system.
                return Ok(());
            }
        };
        let key = utils::ivec_from_iter(
            PREFIX_PIPELINE_STAGE_OUTPUTS
                .iter()
                .copied()
                .chain(utils::encode_u64(offset))
                .chain(stage_name.as_bytes().iter().copied()),
        );
        let event_bytes = utils::encode_model(&event)?;
        pipeline_tree
            .insert(key, event_bytes.as_slice())
            .context("error recording pipeline stage output on disk")
            .map_err(ShutdownError::from)?;
        pipeline_tree.flush_async().await.context(ERR_DB_FLUSH).map_err(ShutdownError::from)?;
        Ok(())
    }

    /// Fetch data from the input stream, filtering out any records which do not match the
    /// pipeline triggers, and transactionally record them as pipeline instances.
    #[tracing::instrument(level = "trace", skip(self))]
    fn fetch_stream_data(&mut self) {
        self.is_fetching_stream_data = true;
        tokio::spawn(Self::try_fetch_stream_data(
            self.tree_stream.clone(),
            self.tree.clone(),
            self.last_offset_processed,
            self.pipeline.clone(),
            self.max_parallel(),
            self.events_tx.clone(),
        ));
    }

    #[tracing::instrument(level = "trace", skip(tree_stream, tree_pipeline, last_offset_processed, pipeline, max_parallel, tx))]
    async fn try_fetch_stream_data(tree_stream: Tree, tree_pipeline: Tree, last_offset_processed: u64, pipeline: Arc<Pipeline>, max_parallel: u32, tx: mpsc::Sender<PipelineCtlMsg>) {
        tracing::debug!("fetching stream data for pipeline");
        let data_res = Database::spawn_blocking(move || -> Result<FetchStreamRecords> {
            // Iterate over the records of the stream up to the maximum parallel allowed.
            let (mut pipeline_batch, mut new_instances) = (sled::Batch::default(), Vec::with_capacity(max_parallel as usize));
            let (mut last_processed, mut count) = (last_offset_processed, 0);
            let (start, stop) = (
                &utils::encode_byte_prefix(PREFIX_STREAM_EVENT, last_offset_processed + 1),
                &utils::encode_byte_prefix(PREFIX_STREAM_EVENT, u64::MAX),
            );
            let kv_iter = tree_stream.range::<_, std::ops::RangeInclusive<&[u8]>>(start..=stop);
            for event_res in kv_iter {
                // Decode the records offset.
                let (key, root_event_bytes) = event_res.context(ERR_ITER_FAILURE)?;
                let offset = utils::decode_u64(&key[1..])?;
                last_processed = offset;
                let root_event: Event = utils::decode_model(root_event_bytes.as_ref()).context("error decoding event from storage")?;

                // Check the event's type to ensure it matches the pipeline's matcher patterns, else skip.
                if !Pipeline::event_type_matches_triggers(pipeline.spec.triggers.as_slice(), &root_event.r#type) {
                    continue;
                }

                // Construct pipeline instance & add to batch.
                pipeline_batch.insert(&utils::encode_byte_prefix(PREFIX_ACTIVE_INSTANCES, offset), &root_event_bytes);
                let inst = ActivePipelineInstance {
                    root_event,
                    root_event_offset: offset,
                    outputs: Default::default(),
                    active_deliveries: Default::default(),
                };
                new_instances.push(inst);
                count += 1;

                // If we've colleced the slots available, then break.
                if count == max_parallel {
                    break;
                }
            }

            // Apply the batch of changes.
            pipeline_batch.insert(KEY_LAST_OFFSET_PROCESSED, &utils::encode_u64(last_processed));
            let _res = tree_pipeline
                .apply_batch(pipeline_batch)
                .context("error applying metadata batch while fetching stream data for pipeline")?;
            let _res = tree_pipeline.flush().context(ERR_DB_FLUSH)?;

            Ok(FetchStreamRecords {
                last_offset_processed: last_processed,
                new_pipeline_instances: new_instances,
            })
        })
        .await
        .map_err(ShutdownError::from)
        .and_then(|res| res.map_err(ShutdownError::from));

        let _ = tx.send(PipelineCtlMsg::FetchStreamRecords(data_res)).await;
    }
}

/// Recover this pipeline's last recorded state.
///
/// The pipeline tree records pipeline instances/executions based on the input stream's
/// offset, which provides easy "exactly once" consumption of the input stream. In the pipeline
/// tree:
/// - The key for a pipeline instance will be roughly `a{offset}/`, where `{offset}` is the
/// source stream record's offset. The value stored here is a copy of the root event from the stream.
/// - The output of each stage of a pipeline instance is recorded under `o{instance}{stage}` where
/// `{instance}` is the source stream record's offset and `{stage}` is the name of the pipeline stage.
#[tracing::instrument(level = "debug", skip(pipeline_tree, pipeline, stream_latest_offset))]
async fn recover_pipeline_state(pipeline_tree: Tree, pipeline: Arc<Pipeline>, stream_latest_offset: u64) -> Result<(u64, BTreeMap<u64, ActivePipelineInstance>)> {
    let val = Database::spawn_blocking(move || -> Result<(u64, BTreeMap<u64, ActivePipelineInstance>)> {
        // Fetch last source stream offset to have been processed by this pipeline.
        let last_offset = pipeline_tree
            .get(KEY_LAST_OFFSET_PROCESSED)
            .context("error fetching pipeline last offset processed key")?
            .map(|val| utils::decode_u64(&val))
            .transpose()
            .context("error decoding pipeline last offset processed key")?
            // If no data recorded, then this is pristine, so start at the configured starting location.
            .unwrap_or_else(|| match &pipeline.spec.start_point.location {
                PipelineStartPointLocation::Beginning => 0,
                PipelineStartPointLocation::Latest => stream_latest_offset,
                PipelineStartPointLocation::Offset => pipeline.spec.start_point.offset.unwrap_or(0),
            });

        // Fetch active instances.
        let active_instances = pipeline_tree
            .scan_prefix(PREFIX_ACTIVE_INSTANCES)
            .try_fold(BTreeMap::new(), |mut acc, kv_res| -> Result<BTreeMap<u64, ActivePipelineInstance>> {
                let (key, val) = kv_res.context(ERR_ITER_FAILURE)?;
                let offset = utils::decode_u64(&key[1..]).context("error decoding active pipeline offset")?;
                let root_event: Event = utils::decode_model(&val).context("error decoding event from storage")?;

                // Iterate over all outputs currently recorded for this pipeline instance.
                // See `try_record_delivery_response`, these are keyed as `o{offset}{stage}`.
                let mut outputs = HashMap::new();
                for iter_res in pipeline_tree.scan_prefix(&utils::encode_byte_prefix(PREFIX_PIPELINE_STAGE_OUTPUTS, offset)) {
                    let (key, val) = iter_res.context(ERR_ITER_FAILURE)?;
                    let stage = std::str::from_utf8(&key[9..]).context("data corruption: stage name for pipeline stage output is not valid utf8")?;
                    let output = Event::decode(val.as_ref()).context("error decoding pipeline stage output")?;
                    outputs.insert(stage.into(), output);
                }
                let inst = ActivePipelineInstance {
                    root_event,
                    root_event_offset: offset,
                    outputs,
                    active_deliveries: Default::default(),
                };
                acc.insert(offset, inst);
                Ok(acc)
            })?;

        Ok((last_offset, active_instances))
    })
    .await??;
    tracing::debug!(last_offset = val.0, active_instances = val.1.len(), "recovered pipeline state");
    Ok(val)
}

//////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////

/// A message bound for a pipeline controller.
#[allow(clippy::large_enum_variant)]
pub enum PipelineCtlMsg {
    /// A client request being routed to the controller.
    Request {
        tx: mpsc::Sender<RpcResult<PipelineSubscribeResponse>>,
        rx: Streaming<PipelineSubscribeRequest>,
        stage_name: String,
    },
    /// A response from a subscriber following a delivery of data for processing.
    DeliveryResponse(DeliveryResponse),
    /// A result from fetching records from the stream for pipeline instantiation.
    FetchStreamRecords(ShutdownResult<FetchStreamRecords>),
    /// An update to the controller's pipeline object.
    PipelineUpdated(Arc<Pipeline>),
    /// An update indicating that this pipeline has been deleted.
    PipelineDeleted(Arc<Pipeline>),
    /// The pipeline instance at the given offset is complete and can be deleted.
    PipelineInstanceComplete(u64),
}

/// A result from fetching stream records for pipeline creation.
pub struct FetchStreamRecords {
    /// The last offset to be processed as part of this fetch routine.
    last_offset_processed: u64,
    /// New pipeline instances.
    new_pipeline_instances: Vec<ActivePipelineInstance>,
}

/// A pipeline delivery response.
pub struct DeliveryResponse {
    /// The ID of the subscription channel.
    id: Uuid,
    /// The offset of the pipeline instance to which this response corresponds.
    offset: u64,
    /// The name of the pipeline stage to which this response applies.
    stage_name: Arc<String>,
    /// The output of awaiting for the subscriber's response.
    output: Option<Result<(ClientChannel, PipelineSubscribeRequest)>>,
}

/// Data on a subscription group for a specific pipeline stage.
struct SubscriptionGroup {
    /// An Arc'd copy of the stage's name for easy sharing across threads
    /// without the need for additional allocations.
    stage_name: Arc<String>,
    /// A mapping of all active subscribers of this group.
    active_channels: HashMap<Uuid, SubChannelState>,
}

impl SubscriptionGroup {
    /// Create a new instance.
    pub fn new(stage_name: Arc<String>) -> Self {
        Self {
            stage_name,
            active_channels: Default::default(),
        }
    }
}

/// A type representing the various states/locations of a pipeline stage subscription channel.
enum SubChannelState {
    /// The channel is currently out as it is being used to deliver data.
    OutForDelivery,
    /// The channel is currently held in a stream monitoring its liveness.
    MonitoringLiveness,
}

/// A handle to a live pipeline controller.
#[derive(Clone)]
pub struct PipelineHandle {
    /// A pointer to the pipeline object.
    ///
    /// This is always kept up-to-date as data flows in from K8s.
    pub pipeline: Arc<Pipeline>,
    /// The controller's communication channel.
    pub tx: mpsc::Sender<PipelineCtlMsg>,
}

/// An active pipeline instance along with all outputs from any completed stages.
pub struct ActivePipelineInstance {
    /// A copy of the stream event which triggered this instance.
    pub root_event: Event,
    /// The offset of the root event on its source Stream.
    pub root_event_offset: u64,
    /// A mapping of stage names to their completion outputs.
    pub outputs: HashMap<String, Event>,
    /// A mapping of active deliveries by stage name to the channel ID currently processing
    /// the stage's delivery.
    pub active_deliveries: HashMap<Arc<String>, Uuid>,
}
