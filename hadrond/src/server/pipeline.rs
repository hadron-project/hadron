//! Pipeline controller.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

use anyhow::{bail, Context, Result};
use bytes::{Bytes, BytesMut};
use futures::stream::StreamExt;
use http::Method;
use prost::Message;
use proto::v1::{
    PipelineSubDelivery, PipelineSubDeliveryResponse, PipelineSubDeliveryResponseResult, PipelineSubSetupRequest, PipelineSubSetupResponse,
    PipelineSubSetupResponseResult, Record,
};
use rand::seq::IteratorRandom;
use sled::{transaction::abort as tx_abort, IVec, Transactional, Tree};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::{
    wrappers::{BroadcastStream, ReceiverStream, WatchStream},
    StreamMap,
};
use uuid::Uuid;

use crate::config::Config;
use crate::database::Database;
use crate::error::{AppError, ShutdownError, ShutdownResult, ERR_DB_FLUSH, ERR_ITER_FAILURE};
use crate::futures::LivenessStream;
use crate::models::{pipeline::ActivePipelineInstance, pipeline::PipelineInstance, pipeline::PipelineStageOutput, prelude::*, schema::Pipeline};
use crate::server::{must_get_token, require_method, send_error, H2Channel, H2DataChannel, MetadataCache};
use crate::utils;

type TxResult = sled::transaction::ConflictableTransactionResult<(), anyhow::Error>;

/// A pipeline metadata key used to track the last offset of the source stream to have been
/// processed by this pipeline.
const KEY_LAST_OFFSET_PROCESSED: &str = "/meta/last_offset_processed";
/// A metadata key prefix used for tracking active pipeline instances.
const PREFIX_META_ACTIVE_INSTANCES: &str = "/meta/active_instances/";
/// The key prefix under which pipeline instances are stored.
const PREFIX_PIPELINE_INSTANCES: &[u8; 3] = b"/i/";
/// The key prefix under which pipeline stage outputs are stored.
const PREFIX_PIPELINE_STAGE_OUTPUTS: &[u8; 3] = b"/s/";
/// The root event dependency identifier.
const ROOT_EVENT: &str = "root_event";

/// A pipeline controller for managing a pipeline.
pub struct PipelineCtl {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,
    /// The database tree for storing this pipeline's instance records.
    tree: Tree,
    /// The database tree for this pipeline's metadata storage.
    tree_metadata: Tree,
    /// The database tree of this pipeline's source stream; which is only ever used for reading.
    tree_stream: Tree,
    /// The system metadata cache.
    metadata: Arc<MetadataCache>,
    /// The data model of the pipeline with which this controller is associated.
    pipeline: Arc<Pipeline>,

    /// A channel of inbound client requests.
    events_tx: mpsc::Sender<PipelineSubCtlMsg>,
    /// A channel of inbound client requests.
    events_rx: ReceiverStream<PipelineSubCtlMsg>,
    /// A mapping of all active pipeline instances.
    active_pipelines: BTreeMap<u64, ActivePipelineInstance>,
    /// A mapping of stage subscribers by stage name.
    stage_subs: HashMap<Arc<String>, SubscriptionGroup>,
    /// A stream of liveness checks on the active subscriber channels.
    liveness_checks: StreamMap<Uuid, LivenessStream>,

    /// A signal describing the source stream's current `next_offset` value.
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

    /// A general purpose reusable bytes buffer, safe for concurrent use.
    buf: BytesMut,
}

impl PipelineCtl {
    /// Create a new instance.
    pub async fn new(
        config: Arc<Config>, db: Database, metadata: Arc<MetadataCache>, pipeline: Arc<Pipeline>, stream_signal: watch::Receiver<u64>,
        shutdown_tx: broadcast::Sender<()>, events_tx: mpsc::Sender<PipelineSubCtlMsg>, events_rx: mpsc::Receiver<PipelineSubCtlMsg>,
    ) -> Result<Self> {
        let tree = db.get_pipeline_tree(&pipeline).await?;
        let tree_metadata = db.get_pipeline_tree_metadata(&pipeline).await?;
        let stream_tree = db.get_stream_tree(pipeline.namespace(), &pipeline.input_stream).await?;
        let (last_offset_processed, active_pipelines) = recover_pipeline_state(&tree, &tree_metadata, &stream_tree, pipeline.clone()).await?;
        let stream_offset = *stream_signal.borrow();

        Ok(Self {
            config,
            db,
            tree,
            tree_metadata,
            tree_stream: stream_tree,
            metadata,
            pipeline,
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
            buf: BytesMut::with_capacity(5000),
        })
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!(
            "pipeline controller {}/{} has started",
            self.pipeline.metadata.namespace,
            self.pipeline.metadata.name
        );

        loop {
            tokio::select! {
                Some(request) = self.events_rx.next() => self.handle_pipeline_msg(request).await,
                Some(dead_chan) = self.liveness_checks.next() => self.handle_dead_subscriber(dead_chan.1).await,
                Some(offset) = self.stream_signal.next() => self.handle_input_stream_offset_update(offset).await,
                Some(_) = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        tracing::debug!(
            "pipeline controller {}/{} has shutdown",
            self.pipeline.metadata.namespace,
            self.pipeline.metadata.name
        );
        Ok(())
    }

    /// Handle any dead channels.
    #[tracing::instrument(level = "trace", skip(self, dead_chan))]
    async fn handle_dead_subscriber(&mut self, dead_chan: (Arc<String>, Uuid, H2DataChannel)) {
        let (group_name, id, chan) = (dead_chan.0, dead_chan.1, dead_chan.2);
        tracing::debug!(?id, group_name = ?&*group_name, "dropping pipeline subscriber channel");
        self.liveness_checks.remove(&id);
        let mut group = match self.stage_subs.remove(&*group_name) {
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
        if self.active_pipelines.len() >= self.pipeline.max_parallel as usize {
            return;
        }
        // Fetch new data from the input stream & create new pipeline instances if triggers match.
        if !self.is_fetching_stream_data {
            self.fetch_stream_data();
        }
    }

    /// Handle a pipeline controller message.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_pipeline_msg(&mut self, mut req: PipelineSubCtlMsg) {
        match req {
            PipelineSubCtlMsg::Request(req) => self.handle_request(req).await,
            PipelineSubCtlMsg::FetchStreamRecords(res) => self.handle_fetch_stream_records_response(res).await,
            PipelineSubCtlMsg::DeliveryResponse(res) => self.handle_delivery_response(res).await,
        }
    }

    /// Handle a response from fetching records from the input stream for pipeline instance creation.
    #[tracing::instrument(level = "trace", skip(self, res))]
    async fn handle_fetch_stream_records_response(&mut self, mut res: ShutdownResult<FetchStreamRecords>) {
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
        tracing::debug!(?data, "response from pipeline data fetch");
        self.last_offset_processed = data.last_offset_processed;
        self.active_pipelines
            .extend(data.new_pipeline_instances.into_iter().map(|inst| (inst.instance.id, inst)));

        // Drive another delivery pass.
        self.execute_delivery_pass().await;
    }

    /// Handle a pipeline stage delivery response.
    #[tracing::instrument(level = "trace", skip(self, res))]
    async fn handle_delivery_response(&mut self, mut res: DeliveryResponse) {
        tracing::info!("received pipeline stage delivery response");
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
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_request(&mut self, mut req: H2Channel) {
        tracing::info!("request received on pipeline controller");
        // Validate the inbound subscriber request.
        let (chan, sub) = match self.validate_subscriber_channel(&mut req).await {
            Ok(chan_and_sub) => chan_and_sub,
            Err(err) => {
                send_error(&mut req, self.buf.split(), err, |e| PipelineSubSetupResponse {
                    result: Some(PipelineSubSetupResponseResult::Err(e)),
                });
                return;
            }
        };

        // Add the new stage subscriber to its corresponding group.
        let group = self
            .stage_subs
            .entry(Arc::new(sub.stage_name.clone()))
            .or_insert_with(|| SubscriptionGroup::new(&sub.stage_name));

        // Roll a new ID for the channel & add it to the group's active channels.
        let h2_data_chan = (req.0.into_body(), chan);
        let id = Uuid::new_v4();
        group.active_channels.insert(id, SubChannel::MonitoringLiveness);
        self.liveness_checks.insert(
            id,
            LivenessStream {
                chan: Some(h2_data_chan),
                chan_id: id,
                group: Arc::new(sub.stage_name),
            },
        );
        self.execute_delivery_pass().await;
    }

    /// Validate a pipeline stage subscriber channel before full setup.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn validate_subscriber_channel(&mut self, req: &mut H2Channel) -> Result<(h2::SendStream<Bytes>, PipelineSubSetupRequest)> {
        require_method(&req.0, Method::POST)?;
        let creds = must_get_token(&req.0, self.config.as_ref())?;
        let claims = self.metadata.must_get_token_claims(&creds.claims.id.as_u128())?;
        // Pipeline subscription auth is based on the source stream of the pipeline.
        claims.check_stream_sub_auth(&self.pipeline.metadata.namespace, &self.pipeline.input_stream)?;

        // Read initial body so that we know which pipeline stage this subscriber is targeting.
        let mut body_req = req
            .0
            .body_mut()
            .data()
            .await
            .context("no body received while setting up pipeline subscriber channel")?
            .context("error awaiting request body while setting up pipeline subscriber channel")?;
        let body = PipelineSubSetupRequest::decode(&mut body_req).context("error decoding pipeline sub setup request")?;
        tracing::debug!("finished validating subscriber setup for pipeline subscriber: {:?}", body);

        // Validate contents of setup request.
        if !self.pipeline.stages.iter().any(|stage| stage.name == body.stage_name) {
            bail!(AppError::InvalidInput(format!(
                "pipeline subscriber stage name '{}' is unknown for pipeline {}/{}",
                body.stage_name, self.pipeline.metadata.namespace, self.pipeline.metadata.name
            )));
        }

        // Respond to subscriber to let them know that the channel is ready for use.
        let mut res = http::Response::new(());
        *res.status_mut() = http::StatusCode::OK;
        let mut res_chan = req
            .1
            .send_response(res, false)
            .context("error returning response to pipeline subscriber for channel setup")?;
        let setup_res = PipelineSubSetupResponse {
            result: Some(PipelineSubSetupResponseResult::Ok(Default::default())),
        };
        let mut setup_res_buf = self.buf.split();
        setup_res
            .encode(&mut setup_res_buf)
            .context("error encoding pipeline sub setup response")?;
        res_chan
            .send_data(setup_res_buf.freeze(), false)
            .context("error sending pipeline sub setup response")?;
        Ok((res_chan, body))
    }

    /// Execute a loop over all active pipeline stage subscription groups, delivering data if possible.
    ///
    /// ### Notes
    /// - The database page cache will stay hot when subscribers are running line-rate. As such,
    ///   cache updates will stay in memory for up-to-date subscriptions.
    #[tracing::instrument(level = "trace", skip(self))]
    async fn execute_delivery_pass(&mut self) {
        tracing::info!("executing pipeline delivery pass");
        if let Err(err) = self.try_execute_delivery_pass().await {
            tracing::error!(error = ?err, "error during pipeline delivery pass");
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn try_execute_delivery_pass(&mut self) -> Result<()> {
        let mut rng = rand::thread_rng();
        for inst in self.active_pipelines.values_mut() {
            for stage in self.pipeline.stages.iter() {
                // Skip stages with an active delivery.
                if inst.active_deliveries.contains_key(&stage.name) {
                    continue;
                }
                // Skip stages which have already been complete.
                if inst.outputs.contains_key(&stage.name) {
                    continue;
                }
                // Skip stages which do not have all dependencies met.
                if !stage.dependencies.iter().all(|dep| inst.outputs.contains_key(dep) || dep == ROOT_EVENT) {
                    continue;
                }
                // Get a handle to the stage subs.
                let sub_group = match self.stage_subs.get_mut(&stage.name) {
                    Some(stage) => stage,
                    None => continue, // Just skip.
                };

                // Randomly select one of the available subscriptions for the stage.
                let chan_key_opt = sub_group
                    .active_channels
                    .iter()
                    .filter(|(_, val)| matches!(val, SubChannel::MonitoringLiveness))
                    .choose(&mut rng)
                    .map(|(key, _val)| *key);
                let chan_id = match chan_key_opt {
                    Some(chan_key) => chan_key,
                    None => continue, // This would only mean that all channels are busy.
                };
                let chan_data = match sub_group.active_channels.remove(&chan_id) {
                    Some(chan_data) => chan_data,
                    None => continue, // Unreachable.
                };
                let mut chan = match self.liveness_checks.remove(&chan_id) {
                    Some(chan_opt) => match chan_opt.chan {
                        Some(chan) => chan,
                        None => continue,
                    },
                    None => {
                        tracing::error!(?chan_id, "pipeline subscription channel was not properly held in liveness stream");
                        continue;
                    }
                };

                // We are ready to delivery some data to the target channel. Accumulate delivery
                // payload with all needed inputs.
                let mut payload = PipelineSubDelivery {
                    stage: stage.name.clone(),
                    offset: inst.instance.id,
                    inputs: Default::default(),
                };
                stage.dependencies.iter().for_each(|dep| {
                    if dep == ROOT_EVENT {
                        payload.inputs.insert(ROOT_EVENT.into(), inst.root_event.to_vec());
                    } else {
                        match inst.outputs.get(dep) {
                            Some(input) => {
                                payload.inputs.insert(dep.clone(), input.data.clone());
                            }
                            None => tracing::error!("failed to accumulate stage dependencies even though all were accounted for"),
                        }
                    }
                });

                // Payload is ready, send it.
                let mut buf = self.buf.split();
                payload.encode(&mut buf).context("error encoding pipeline delivery payload")?;
                chan.1.send_data(buf.freeze(), false).context("error sending pipeline delivery payload")?;

                // Spawn off a task to await the response from the client.
                inst.active_deliveries.insert(sub_group.stage_name.clone(), chan_id);
                sub_group.active_channels.insert(chan_id, SubChannel::OutForDelivery);
                let (tx, group_name, id, offset) = (self.events_tx.clone(), sub_group.stage_name.clone(), chan_id, inst.instance.id);
                tokio::spawn(async move {
                    // TODO: add optional timeouts here based on pipeline config.
                    let output = chan.0.data().await.map(|res| res.map_err(anyhow::Error::from).map(|data| (chan, data)));
                    let _ = tx
                        .send(PipelineSubCtlMsg::DeliveryResponse(DeliveryResponse {
                            id,
                            offset,
                            stage_name: group_name,
                            output,
                        }))
                        .await;
                });
            }
        }

        // If number of active instances is < the pipeline's max parallel settings, then fetch
        // more data from the source stream to find matching records for pipeline instantiation.
        if self.active_pipelines.len() < self.pipeline.max_parallel as usize
            && self.stream_offset > self.last_offset_processed
            && !self.is_fetching_stream_data
        {
            self.fetch_stream_data();
        }
        Ok(())
    }

    /// Handle a pipeline stage delivery response.
    #[tracing::instrument(level = "trace", skip(self, res))]
    async fn try_handle_delivery_response(&mut self, mut res: DeliveryResponse) -> Result<()> {
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
        let res = res
            .output
            .context("subscriber channel closed while awaiting delivery response")
            .map_err(|err| {
                let _ = group.active_channels.remove(chan_id);
                err
            })?;
        let (data_chan, mut body) = res.context("error returned while awaiting subscriber delivery response").map_err(|err| {
            let _ = group.active_channels.remove(chan_id);
            err
        })?;
        if let Some(chan_wrapper) = group.active_channels.get_mut(chan_id) {
            *chan_wrapper = SubChannel::MonitoringLiveness;
            self.liveness_checks.insert(
                *chan_id,
                LivenessStream {
                    chan: Some(data_chan),
                    group: group.stage_name.clone(),
                    chan_id: *chan_id,
                },
            );
        }

        // Decode the response body and record the result.
        let response_msg = PipelineSubDeliveryResponse::decode(&mut body).context("malformed response returned from pipeline subscriber")?;
        let record_res = match response_msg.result {
            Some(PipelineSubDeliveryResponseResult::Ack(output)) => {
                inst.outputs.insert(
                    stage_name.clone(),
                    PipelineStageOutput {
                        id: offset,
                        pipeline_id: self.pipeline.id,
                        stage: stage_name.clone(),
                        data: output.output.clone(),
                    },
                );
                Ok(output.output)
            }
            Some(PipelineSubDeliveryResponseResult::Nack(err)) => Err(err.message),
            None => Err("malformed response returned from pipeline subscriber, unknown result variant".into()),
        };
        Self::try_record_delivery_response(record_res, offset, group.stage_name.clone(), self.tree.clone())
            .await
            .context("error while recording subscriber delivery response")?;

        // Finally, if this was the last outstanding stage of the pipeline instance, then remove
        // it from the active instances set.
        if inst.outputs.len() == self.pipeline.stages.len() {
            self.active_pipelines.remove(&offset);
        }

        Ok(())
    }

    /// Record the ack/nack response from a subscriber delivery.
    #[tracing::instrument(level = "trace", skip(res, offset, stage_name, pipeline_tree))]
    async fn try_record_delivery_response(
        res: std::result::Result<Vec<u8>, String>, offset: u64, stage_name: Arc<String>, pipeline_tree: Tree,
    ) -> ShutdownResult<()> {
        Database::spawn_blocking(move || -> ShutdownResult<()> {
            let output = match res {
                Ok(output) => output,
                Err(_err) => {
                    // FUTURE: record this error info for observability system.
                    return Ok(());
                }
            };
            let key = format!(
                "{}{}/{}",
                unsafe { std::str::from_utf8_unchecked(PREFIX_PIPELINE_STAGE_OUTPUTS) },
                offset,
                &*stage_name,
            );
            pipeline_tree
                .insert(key.as_bytes(), output.as_slice())
                .context("error recording pipeline stage output on disk")
                .map_err(ShutdownError::from)?;
            pipeline_tree.flush().context(ERR_DB_FLUSH).map_err(ShutdownError::from)?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    /// Fetch data from the input stream, filtering out any records which do not match the
    /// pipeline triggers, and transactionally record them as pipeline instances.
    #[tracing::instrument(level = "trace", skip(self))]
    fn fetch_stream_data(&mut self) {
        self.is_fetching_stream_data = true;
        tokio::spawn(Self::try_fetch_stream_data(
            self.tree.clone(),
            self.tree_stream.clone(),
            self.tree_metadata.clone(),
            self.last_offset_processed,
            self.pipeline.max_parallel,
            self.pipeline.id,
            self.buf.clone(),
            self.events_tx.clone(),
        ));
    }

    #[tracing::instrument(level = "trace", skip(tree_pipeline, tree_stream, tree_metadata, last_offset_processed))]
    async fn try_fetch_stream_data(
        tree_pipeline: Tree, tree_stream: Tree, tree_metadata: Tree, last_offset_processed: u64, slots_available: u32, pipeline_id: u64,
        mut buffer: BytesMut, tx: mpsc::Sender<PipelineSubCtlMsg>,
    ) {
        tracing::debug!("fetching stream data for pipeline");
        let data_res = Database::spawn_blocking(move || -> Result<FetchStreamRecords> {
            // Iterate over the records of the stream up to the maximum parallel allowed.
            let (mut batch, mut new_instances) = (sled::Batch::default(), Vec::with_capacity(slots_available as usize));
            let (start, mut count) = (&utils::encode_u64(last_offset_processed + 1), 0);
            let mut stream = tree_stream.range::<_, std::ops::RangeFrom<&[u8]>>(start..);
            for record_res in stream {
                // Decode the records offset.
                let (key, record) = record_res.context(ERR_ITER_FAILURE)?;
                let offset = utils::decode_u64(key.as_ref()).context("error decoding stream record offset")?;
                // TODO: critical path: need to start encoding headers so that we can check for type match.

                // Construct pipeline instance & add to batch.
                let mut buf = buffer.split();
                let pipeline_inst = PipelineInstance { id: offset, pipeline_id };
                pipeline_inst.encode(&mut buf).context("error encoding pipeline instance record")?;
                batch.insert(&key, buf.freeze().as_ref());
                let inst = ActivePipelineInstance {
                    instance: pipeline_inst,
                    root_event: Bytes::from(record.to_vec()),
                    outputs: Default::default(),
                    active_deliveries: Default::default(),
                };
                new_instances.push(inst);
                count += 1;

                // If we've colleced the slots available, then break.
                if count == slots_available {
                    break;
                }
            }

            // Transactionally apply the batch.
            let last_processed = last_offset_processed + count as u64;
            let res = (&tree_pipeline, &tree_metadata).transaction(move |(pipeline, metadata)| -> TxResult {
                pipeline.apply_batch(&batch)?;
                metadata.insert(KEY_LAST_OFFSET_PROCESSED, &utils::encode_u64(last_processed))?;
                Ok(())
            });

            Ok(FetchStreamRecords {
                last_offset_processed: last_processed,
                new_pipeline_instances: new_instances,
            })
        })
        .await
        .map_err(ShutdownError::from)
        .and_then(|res| res.map_err(ShutdownError::from));

        let _ = tx.send(PipelineSubCtlMsg::FetchStreamRecords(data_res)).await;
    }
}

/// Recover this pipeline's last recorded state.
///
/// The pipeline tree records pipeline instances/executions based on the input stream's
/// offset, which provides easy "exactly once" consumption of the input stream. In the pipeline
/// tree:
/// - The key for a pipeline instance will be roughly `/i/{instance}/`, where `{instance}` is the
/// input stream record's offset. The value stored here is top-level metadata on the instance.
/// - The output of each stage of a pipeline instance is recorded under `/s/{instance}/{stage}` where
/// `{instance}` is the input stream record's offset and `{stage}` is the name of the pipeline stage.
async fn recover_pipeline_state(
    pipeline_tree: &Tree, metadata_tree: &Tree, stream_tree: &Tree, pipeline: Arc<Pipeline>,
) -> Result<(u64, BTreeMap<u64, ActivePipelineInstance>)> {
    let (pipeline_tree, metadata_tree, stream_tree) = (pipeline_tree.clone(), metadata_tree.clone(), stream_tree.clone());
    let val = Database::spawn_blocking(move || -> Result<(u64, BTreeMap<u64, ActivePipelineInstance>)> {
        // Fetch last source stream offset to have been processed by this pipeline.
        let last_offset = metadata_tree
            .get(KEY_LAST_OFFSET_PROCESSED)
            .context("error fetching pipeline last offset processed key")?
            .map(|val| utils::decode_u64(&val))
            .transpose()
            .context("error decoding pipeline last offset processed key")?
            .unwrap_or(0); // If no data recorded, then this is pristine, so start at 0.

        // Fetch active instances.
        let active_instances = metadata_tree.scan_prefix(PREFIX_META_ACTIVE_INSTANCES).values().try_fold(
            BTreeMap::new(),
            |mut acc, val| -> Result<BTreeMap<u64, ActivePipelineInstance>> {
                let offset_ivec = val.context(ERR_ITER_FAILURE)?;
                let offset = utils::decode_u64(&offset_ivec).context("error decoding active pipeline offset")?;
                #[allow(clippy::unnecessary_lazy_evaluations)]
                let instance = pipeline_tree
                    .get(&utils::encode_3_byte_prefix(PREFIX_PIPELINE_INSTANCES, offset))
                    .context("error fetching pipeline instance")?
                    .map(|val| PipelineInstance::decode(val.as_ref()).context("error decoding pipeline instance"))
                    .transpose()?
                    .unwrap_or_else(|| PipelineInstance {
                        id: offset,
                        pipeline_id: pipeline.id,
                    });

                // Fetch the stream record which triggered this pipeline instance.
                let root_event = stream_tree
                    .get(&offset_ivec)
                    .context("error fetching pipeline instance root event")?
                    .map(|data| Bytes::from(data.to_vec()))
                    .unwrap_or_else(Bytes::new);

                // Iterate over all outputs currently recorded for this pipeline instance.
                let mut outputs = HashMap::new();
                for iter_res in pipeline_tree.scan_prefix(PREFIX_PIPELINE_STAGE_OUTPUTS) {
                    let (_key, val) = iter_res.context(ERR_ITER_FAILURE)?;
                    let output = PipelineStageOutput::decode(val.as_ref()).context("error decoding pipeline stage output")?;
                    outputs.insert(output.stage.clone(), output);
                }

                let inst = ActivePipelineInstance {
                    instance,
                    root_event,
                    outputs,
                    active_deliveries: Default::default(),
                };
                acc.insert(offset, inst);
                Ok(acc)
            },
        )?;

        Ok((last_offset, active_instances))
    })
    .await??;
    Ok(val)
}

//////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////

/// A message bound for a pipeline subscription controller.
pub enum PipelineSubCtlMsg {
    /// A client request being routed to the controller.
    Request(H2Channel),
    /// A response from a subscriber following a delivery of data for processing.
    DeliveryResponse(DeliveryResponse),
    /// A result from fetching records from the stream for pipeline instantiation.
    FetchStreamRecords(ShutdownResult<FetchStreamRecords>),
}

/// A result from fetching stream records for pipeline creation.
#[derive(Debug)]
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
    output: Option<Result<(H2DataChannel, Bytes)>>,
}

/// Data on a subscription group for a specific pipeline stage.
struct SubscriptionGroup {
    /// An Arc'd copy of the stage's name for easy sharing across threads
    /// without the need for additional allocations.
    stage_name: Arc<String>,
    /// A mapping of all active subscribers of this group.
    active_channels: HashMap<Uuid, SubChannel>,
}

impl SubscriptionGroup {
    /// Create a new instance.
    pub fn new(group_name: &str) -> Self {
        Self {
            stage_name: Arc::new(group_name.into()),
            active_channels: Default::default(),
        }
    }
}

/// A type representing the various states/locations of a pipeline stage subscription channel.
enum SubChannel {
    /// The channel is currently out as it is being used to deliver data.
    OutForDelivery,
    /// The channel is currently held in a stream monitoring its liveness.
    MonitoringLiveness,
}
