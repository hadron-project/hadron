use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::BytesMut;
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
use crate::error::{AppError, AppErrorExt, RpcResult, ShutdownError, ShutdownResult, ERR_DB_FLUSH, ERR_ITER_FAILURE};
use crate::futures::LivenessStream;
use crate::grpc::{
    Event, StreamSubscribeRequest, StreamSubscribeRequestAction, StreamSubscribeResponse, StreamSubscribeSetup, StreamSubscribeSetupStartingPoint,
};
use crate::models::stream::Subscription;
use crate::stream::{PREFIX_STREAM_SUBS, PREFIX_STREAM_SUB_OFFSETS};
use crate::utils;

/// The default max batch size for subscription groups.
const DEFAULT_MAX_BATCH_SIZE: u32 = 50;

/// The liveness stream type used by this controller.
type SubLivenessStream = LivenessStream<RpcResult<StreamSubscribeResponse>, StreamSubscribeRequest>;
/// The sender & receiver side of a client channel.
type ClientChannel = (mpsc::Sender<RpcResult<StreamSubscribeResponse>>, Streaming<StreamSubscribeRequest>);

/// A stream controller's child controller for managing subscriptions.
pub struct StreamSubCtl {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    _db: Database,
    /// This stream's database tree for storing stream records.
    tree: Tree,
    /// This stream's database tree for metadata storage.
    tree_metadata: Tree,
    /// The stream partition of this controller.
    partition: u32,

    /// Data on all subscriptions and active subscribers.
    subs: SubscriberInfo,
    /// The parent stream's next offset.
    next_offset: u64,

    /// A channel of events to be processed by this controller.
    events_tx: mpsc::Sender<StreamSubCtlMsg>,
    /// A channel of events to be processed by this controller.
    events_rx: ReceiverStream<StreamSubCtlMsg>,
    /// A signal from the parent stream on the stream's `next_offset` value.
    stream_offset: WatchStream<u64>,
    /// A stream of liveness checks on the active subscriber channels.
    liveness_checks: StreamMap<Uuid, SubLivenessStream>,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
    /// A bool indicating that this controller has been descheduled and needs to shutdown.
    descheduled: bool,

    /// A general purpose reusable bytes buffer, safe for concurrent use.
    buf: BytesMut,
}

impl StreamSubCtl {
    /// Create a new instance.
    pub fn new(
        config: Arc<Config>, db: Database, tree: Tree, tree_metadata: Tree, partition: u32, shutdown_tx: broadcast::Sender<()>,
        events_tx: mpsc::Sender<StreamSubCtlMsg>, events_rx: mpsc::Receiver<StreamSubCtlMsg>, stream_offset: watch::Receiver<u64>,
        subs: Vec<(Subscription, u64)>, next_offset: u64,
    ) -> Self {
        let subs = SubscriberInfo::new(subs);
        Self {
            config,
            _db: db,
            tree,
            tree_metadata,
            partition,
            subs,
            next_offset,
            events_tx,
            events_rx: ReceiverStream::new(events_rx),
            stream_offset: WatchStream::new(stream_offset),
            liveness_checks: StreamMap::new(),
            shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
            shutdown_tx,
            descheduled: false,
            buf: BytesMut::with_capacity(5000),
        }
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!("stream subscriber controller {}/{} has started", self.config.stream, self.partition,);

        loop {
            if self.descheduled {
                break;
            }
            tokio::select! {
                Some(msg) = self.events_rx.next() => self.handle_msg(msg).await,
                Some(offset) = self.stream_offset.next() => self.handle_offset_update(offset).await,
                Some(dead_chan) = self.liveness_checks.next() => self.handle_dead_subscriber(dead_chan.1).await,
                _ = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        tracing::debug!("stream subscriber controller {}/{} has shutdown", self.config.stream, self.partition);
        Ok(())
    }

    /// Handle a message sent to this controller from its parent stream controller.
    #[tracing::instrument(level = "trace", skip(self, msg))]
    async fn handle_msg(&mut self, msg: StreamSubCtlMsg) {
        match msg {
            StreamSubCtlMsg::Request { tx, rx, setup } => self.handle_request(tx, rx, setup).await,
            StreamSubCtlMsg::FetchStreamRecords(res) => self.handle_fetch_stream_records_result(res).await,
            StreamSubCtlMsg::DeliveryResponse(res) => self.handle_delivery_response(res).await,
            StreamSubCtlMsg::Shutdown => {
                self.descheduled = true;
            }
        }
    }

    /// Handle an update of the stream's next offset, indicating that new data has been written.
    #[tracing::instrument(level = "trace", skip(self, offset))]
    async fn handle_offset_update(&mut self, offset: u64) {
        self.next_offset = offset;
        self.execute_delivery_pass().await;
    }

    /// Handle any dead channels.
    #[tracing::instrument(level = "trace", skip(self, dead_chan))]
    async fn handle_dead_subscriber(&mut self, dead_chan: (Arc<String>, Uuid, ClientChannel)) {
        let (group_name, id, _chan) = (dead_chan.0, dead_chan.1, dead_chan.2);
        tracing::debug!(?id, group_name = ?&*group_name, "dropping stream subscriber channel");
        self.liveness_checks.remove(&id);
        let group = match self.subs.groups.get_mut(&*group_name) {
            Some(group) => group,
            None => return,
        };
        group.active_channels.remove(&id);
        self.subs
            .groups
            .retain(|_, group| group.durable || !group.active_channels.is_empty());
    }

    /// Handle a request which has been sent to this controller.
    #[tracing::instrument(level = "trace", skip(self, tx, rx, setup))]
    async fn handle_request(
        &mut self, tx: mpsc::Sender<RpcResult<StreamSubscribeResponse>>, rx: Streaming<StreamSubscribeRequest>, setup: StreamSubscribeSetup,
    ) {
        // Validate contents of setup request.
        if setup.group_name.is_empty() {
            let _res = tx
                .send(Err(Status::invalid_argument("subscriber group name may not be an empty string")))
                .await;
            return;
        }
        if setup.max_batch_size == 0 {
            let _res = tx
                .send(Err(Status::invalid_argument("subscriber batch size must be greater than 0")))
                .await;
            return;
        }

        // Ensure the subscription is properly recorded.
        let group = match self.ensure_subscriber_record(&setup).await {
            Ok(group) => group,
            Err(err) => {
                if err.downcast_ref::<ShutdownError>().is_some() {
                    tracing::error!(error = ?err);
                    let _ = self.shutdown_tx.send(());
                }
                let _res = tx.send(Err(AppError::grpc(err))).await;
                return;
            }
        };
        let group_name = group.group_name.clone();

        // Roll a new ID for the channel & add it to the group's active channels.
        let id = Uuid::new_v4();
        group.active_channels.insert(id, SubChannelState::MonitoringLiveness);
        self.liveness_checks.insert(
            id,
            LivenessStream {
                chan: Some((tx, rx)),
                chan_id: id,
                group: group_name,
            },
        );
        self.execute_delivery_pass().await;
    }

    /// Handle the result of fetching records from the stream to be cached for subscription delivery.
    #[tracing::instrument(level = "trace", skip(self, res))]
    async fn handle_fetch_stream_records_result(&mut self, res: ShutdownResult<FetchStreamRecords>) {
        // Unpack data fetch result.
        let fetched_data = match res {
            Ok(fetched_data) => fetched_data,
            Err(err) => {
                tracing::error!(error = ?err, "error while fetching data for subscription delivery");
                let _ = self.shutdown_tx.send(());
                return;
            }
        };

        // Get a mutable handle to subscription group for which data has been fetched.
        let group = match self.subs.groups.get_mut(&*fetched_data.group_name) {
            Some(group) => group,
            None => {
                tracing::warn!(group = ?&*fetched_data.group_name, "data fetched for group delivery dropped as group no longer exists");
                return;
            }
        };
        group.is_fetching_data = false;

        // Encode the fetched data for delivery.
        let last_included_offset = match fetched_data.data.last().map(|event| event.id) {
            Some(offset) => offset,
            None => {
                tracing::debug!("empty fetch payload returned from stream fetch");
                return;
            }
        };
        let msg = StreamSubscribeResponse { last_included_offset, batch: fetched_data.data };
        group.delivery_cache = SubGroupDataCache::NeedsDelivery(Arc::new(msg));

        // Attempt to deliver the data.
        Self::try_deliver_data_to_sub(group, &mut self.liveness_checks, self.events_tx.clone()).await;
    }

    /// Handle the response from a subscriber following a data payload delivery.
    #[tracing::instrument(level = "trace", skip(self, res), fields(group = ?res.group_name, last_offset = res.orig_data.last_included_offset))]
    async fn handle_delivery_response(&mut self, res: DeliveryResponse) {
        if let Err(err) = self.try_handle_delivery_response(res).await {
            tracing::error!(error = ?err, "error handling subscriber ack/nack response");
            if err.downcast_ref::<ShutdownError>().is_some() {
                let _ = self.shutdown_tx.send(());
                return;
            }
        }
        // Drive another delivery pass.
        self.execute_delivery_pass().await;
    }

    /// Ensure a subscription record exists if it does not already exist in the index.
    #[tracing::instrument(level = "trace", skip(self, sub))]
    async fn ensure_subscriber_record(&mut self, sub: &StreamSubscribeSetup) -> Result<&mut SubscriptionGroup> {
        // Get a handle to the group subscriber data, creating one if not present.
        let already_exists = self.subs.groups.contains_key(&sub.group_name);
        let stream_current_offset = self.next_offset.saturating_sub(1);
        let offset = match &sub.starting_point {
            Some(StreamSubscribeSetupStartingPoint::Beginning(_empty)) => 0,
            Some(StreamSubscribeSetupStartingPoint::Latest(_empty)) => stream_current_offset,
            Some(StreamSubscribeSetupStartingPoint::Offset(offset)) => {
                let offset = if *offset == 0 { 0 } else { offset - 1 };
                if offset > stream_current_offset {
                    stream_current_offset
                } else {
                    offset
                }
            }
            None => stream_current_offset,
        };
        let entry = self.subs.groups.entry(sub.group_name.clone())
            // Ensure the subscription model exists.
            .or_insert_with(|| {
                let durable = sub.durable;
                let max_batch_size = if sub.max_batch_size == 0 { DEFAULT_MAX_BATCH_SIZE } else { sub.max_batch_size };
                let sub = Subscription {
                    group_name: sub.group_name.clone(),
                    max_batch_size,
                };
                SubscriptionGroup::new(sub, offset, durable)
            });

        // If the subscription is durable & did not already exist, then write the subscription model to disk.
        if sub.durable && !already_exists {
            let mut buf = self.buf.split();
            entry
                .subscription
                .encode(&mut buf)
                .context("error encoding subscription record")?;
            let stream_model_key = format!("{}{}", PREFIX_STREAM_SUBS, sub.group_name);
            let stream_offset_key = format!("{}{}", PREFIX_STREAM_SUB_OFFSETS, sub.group_name);

            let mut batch = sled::Batch::default();
            batch.insert(stream_model_key.as_bytes(), buf.freeze().as_ref());
            batch.insert(stream_offset_key.as_bytes(), &utils::encode_u64(offset));
            self.tree_metadata
                .apply_batch(batch)
                .context("error writing subscription record and offset to disk")
                .map_err(ShutdownError::from)?;
            self.tree_metadata
                .flush_async()
                .await
                .context(ERR_DB_FLUSH)
                .map_err(ShutdownError::from)?;
        }

        Ok(entry)
    }

    /// Execute a loop over all active subscription groups, delivering data if possible.
    ///
    /// ### Notes
    /// - The database page cache will stay hot when subscribers are running line-rate. As such,
    ///   cache updates will stay in memory for up-to-date subscriptions.
    #[tracing::instrument(level = "trace", skip(self))]
    async fn execute_delivery_pass(&mut self) {
        // Execute a delivery pass.
        let stream_current_offset = self.next_offset.saturating_sub(1);
        for (_name, group) in self.subs.groups.iter_mut() {
            // If the gruop has no active channels, then skip it.
            if group.active_channels.is_empty() {
                continue;
            }

            // If the group has an idle delivery cache, attempt to deliver it.
            match group.delivery_cache {
                // Attempt to drive a delivery.
                SubGroupDataCache::NeedsDelivery(_) => {
                    Self::try_deliver_data_to_sub(group, &mut self.liveness_checks, self.events_tx.clone()).await;
                    continue;
                }
                // Skip this group as it already has an active delivery.
                SubGroupDataCache::OutForDelivery(_) => continue,
                // This group has no cached data, so proceed.
                SubGroupDataCache::None => (),
            }

            // If the group is at the head of the stream, then skip it.
            if group.offset >= stream_current_offset {
                continue;
            }

            // If data is currently being fetched for this group, then skip it.
            if group.is_fetching_data {
                continue;
            }

            // This group is ready to have some data fetched, so spawn a task to do so.
            group.is_fetching_data = true;
            Self::spawn_group_fetch(
                group.group_name.clone(),
                group.offset + 1,
                group.subscription.max_batch_size,
                self.tree.clone(),
                self.events_tx.clone(),
            );
        }
    }

    #[tracing::instrument(level = "trace", skip(self, delivery_res))]
    async fn try_handle_delivery_response(&mut self, delivery_res: DeliveryResponse) -> Result<()> {
        // Get a mutable handle to subscription group to which this response applies.
        let (chan_id, group_name) = (&delivery_res.id, &*delivery_res.group_name);
        let group = self
            .subs
            .groups
            .get_mut(group_name)
            .context("response from subscription delivery dropped as group no longer exists")?;

        // Unpack response body.
        let last_offset = delivery_res.orig_data.last_included_offset;
        group.delivery_cache = SubGroupDataCache::NeedsDelivery(delivery_res.orig_data);
        let res = delivery_res
            .output
            .context("subscriber channel closed while awaiting delivery response")
            .map_err(|err| {
                let _ = group.active_channels.remove(chan_id);
                err
            })?;
        let (client_chan, body) = res
            .context("error returned while awaiting subscriber delivery response")
            .map_err(|err| {
                let _ = group.active_channels.remove(chan_id);
                err
            })?;
        if let Some(chan_wrapper) = group.active_channels.get_mut(chan_id) {
            *chan_wrapper = SubChannelState::MonitoringLiveness;
            self.liveness_checks.insert(
                *chan_id,
                LivenessStream {
                    chan: Some(client_chan),
                    group: group.group_name.clone(),
                    chan_id: *chan_id,
                },
            );
        }

        // Check the response variant.
        let record_res = match body.action {
            // If ack'ed, then we don't redeliver the data.
            Some(StreamSubscribeRequestAction::Ack(_empty)) => {
                group.delivery_cache = SubGroupDataCache::None;
                group.offset = last_offset;
                Ok(last_offset)
            }
            Some(StreamSubscribeRequestAction::Nack(err)) => Err(err),
            _ => Err("unexpected or malformed response returned from subscriber, expected ack or nack".into()),
        };
        if group.durable {
            Self::try_record_delivery_response(record_res, group.group_name.clone(), self.tree_metadata.clone())
                .await
                .context("error while recording subscriber delivery response")?;
        }
        Ok(())
    }

    /// Record the ack/nack response from a subscriber delivery.
    #[tracing::instrument(level = "trace", skip(res, group_name, tree_metadata))]
    async fn try_record_delivery_response(res: std::result::Result<u64, String>, group_name: Arc<String>, tree_metadata: Tree) -> ShutdownResult<()> {
        let offset = match res {
            Ok(offset) => offset,
            Err(_err) => {
                // TODO[telemetry]: in the future, we will record this for observability system.
                return Ok(());
            }
        };
        let key = format!("{}{}", PREFIX_STREAM_SUB_OFFSETS, &*group_name);
        tree_metadata
            .insert(key.as_bytes(), &utils::encode_u64(offset))
            .context("error updating subscription offsets on disk")
            .map_err(ShutdownError::from)?;
        tree_metadata
            .flush_async()
            .await
            .context(ERR_DB_FLUSH)
            .map_err(ShutdownError::from)?;
        Ok(())
    }

    /// Attempt to deliver a payload of data to the target group.
    #[tracing::instrument(level = "trace", skip(group, liveness_stream, tx))]
    async fn try_deliver_data_to_sub(
        group: &mut SubscriptionGroup, liveness_stream: &mut StreamMap<Uuid, SubLivenessStream>, tx: mpsc::Sender<StreamSubCtlMsg>,
    ) {
        // Get a handle to the group's cached data, else there is nothing to do here.
        let data = match &group.delivery_cache {
            SubGroupDataCache::NeedsDelivery(data) => data.clone(),
            _ => return,
        };

        // Attempt to deliver the data to one of the active channels of this group randomly.
        loop {
            // Randomly select one of the active channels for delivery.
            let chan_id = {
                let mut rng = rand::thread_rng();
                match group.active_channels.keys().choose(&mut rng) {
                    // Unpack the selected channel.
                    Some(chan_id) => *chan_id,
                    // Nothing to do if there are no longer any active channels.
                    None => {
                        group.delivery_cache = SubGroupDataCache::None;
                        return;
                    }
                }
            };

            // Extract the channel and prep for use.
            let chan_state = match group.active_channels.remove(&chan_id) {
                Some(chan_state) => chan_state,
                None => continue, // This will never be hit.
            };
            let mut chan = match chan_state {
                // The selected channel is already out for delivery, and this should never happen
                // as the group data cache would also be marked as "out for delivery" and this method
                // would not be invoked until the delivery was resolved.
                SubChannelState::OutForDelivery => {
                    tracing::error!("invariant violation: subscription channel was not properly set back into idle state");
                    continue;
                }
                // The selected channel is currently being monitored for liveness.
                SubChannelState::MonitoringLiveness => match liveness_stream.remove(&chan_id).and_then(|data| data.chan) {
                    Some(chan) => chan,
                    None => {
                        tracing::error!("invariant violation: subscription channel was not properly held in liveness monitoring stream");
                        continue;
                    }
                },
            };
            group.delivery_cache = SubGroupDataCache::OutForDelivery((chan_id, data.clone()));

            // Send the data payload to the channel.
            if let Err(err) = chan.0.send(Ok(data.as_ref().clone())).await {
                tracing::error!(error = ?err, "error delivering payload to subscription channel");
                continue;
            }

            // Spawn a task to wait for the subscriber's response & handle timeouts & errors.
            group.active_channels.insert(chan_id, SubChannelState::OutForDelivery);
            let (tx, group_name, id, orig_data) = (tx.clone(), group.group_name.clone(), chan_id, data);
            tokio::spawn(async move {
                // TODO: add optional timeouts here based on subscription group config.
                let output = chan
                    .1
                    .next()
                    .await
                    .map(|res| res.map_err(anyhow::Error::from).map(|data| (chan, data)));
                let _ = tx
                    .send(StreamSubCtlMsg::DeliveryResponse(DeliveryResponse { id, group_name, output, orig_data }))
                    .await;
            });
            return;
        }
    }

    /// Spawn a data fetch operation to pull data from the stream for a subscription group.
    #[tracing::instrument(level = "trace", skip(group_name, max_batch_size, tree, tx))]
    fn spawn_group_fetch(group_name: Arc<String>, next_offset: u64, max_batch_size: u32, tree: Tree, tx: mpsc::Sender<StreamSubCtlMsg>) {
        tokio::spawn(async move {
            // Spawn a blocking read of the stream.
            let res = Database::spawn_blocking(move || -> Result<FetchStreamRecords> {
                let start = utils::encode_u64(next_offset);
                let stop = utils::encode_u64(next_offset + max_batch_size as u64);
                let mut data = Vec::with_capacity(max_batch_size as usize);
                for iter_res in tree.range(start..stop) {
                    let (_key, val) = iter_res.context(ERR_ITER_FAILURE).map_err(ShutdownError::from)?;
                    let event: Event = utils::decode_model(val.as_ref())
                        .context("error decoding event from storage")
                        .map_err(ShutdownError::from)?;
                    data.push(event);
                }
                Ok(FetchStreamRecords { group_name, data })
            })
            .await
            .and_then(|res| res.map_err(ShutdownError::from));

            match res {
                Ok(fetched_data) => {
                    let _ = tx.send(StreamSubCtlMsg::FetchStreamRecords(Ok(fetched_data))).await;
                }
                Err(err) => {
                    let _ = tx.send(StreamSubCtlMsg::FetchStreamRecords(Err(err))).await;
                }
            }
        });
    }
}

/// A message bound for a stream subscription controller.
pub enum StreamSubCtlMsg {
    /// A client request being routed to the controller.
    Request {
        tx: mpsc::Sender<RpcResult<StreamSubscribeResponse>>,
        rx: Streaming<StreamSubscribeRequest>,
        setup: StreamSubscribeSetup,
    },
    /// A result from fetching records from the stream for subscription delivery.
    FetchStreamRecords(ShutdownResult<FetchStreamRecords>),
    /// A response from a subscriber following a delivery of data for processing.
    DeliveryResponse(DeliveryResponse),
    /// The parent controller is shutting down, so this controller needs to do the same.
    Shutdown,
}

pub struct FetchStreamRecords {
    group_name: Arc<String>,
    data: Vec<Event>,
}

pub struct DeliveryResponse {
    /// The ID of the subscription channel.
    id: Uuid,
    /// The name of the subscription group.
    group_name: Arc<String>,
    /// The output of awaiting for the subscriber's response.
    output: Option<Result<(ClientChannel, StreamSubscribeRequest)>>,
    /// The original data delivered.
    orig_data: Arc<StreamSubscribeResponse>,
}

//////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////

/// Data on all subscriptions along with their active subscriber channels.
struct SubscriberInfo {
    /// A mapping of all subscriptions by group name.
    pub groups: HashMap<String, SubscriptionGroup>,
}

impl SubscriberInfo {
    /// Create a new instance.
    fn new(subs: Vec<(Subscription, u64)>) -> Self {
        let groups = subs
            .into_iter()
            .map(|(sub, offset)| {
                let group_data = SubscriptionGroup::new(sub, offset, true);
                (group_data.subscription.group_name.clone(), group_data)
            })
            .collect();
        Self { groups }
    }
}

/// Data on a subscription group.
struct SubscriptionGroup {
    /// An Arc'd copy of the group's name for easy sharing across threads
    /// without the need for additional allocations.
    group_name: Arc<String>,
    /// The data model of this subscription.
    subscription: Subscription,
    /// A bool indicating if this is a durable group or not.
    durable: bool,
    /// The last offset to have been processed by this subscription.
    offset: u64,
    /// A mapping of all active subscribers of this group.
    active_channels: HashMap<Uuid, SubChannelState>,
    /// The possible states of this group's data delivery cache.
    delivery_cache: SubGroupDataCache,
    /// A bool indicating if data is currently being fetched for this group.
    is_fetching_data: bool,
}

impl SubscriptionGroup {
    /// Create a new instance.
    pub fn new(subscription: Subscription, offset: u64, durable: bool) -> Self {
        Self {
            group_name: Arc::new(subscription.group_name.clone()),
            subscription,
            durable,
            offset,
            active_channels: Default::default(),
            delivery_cache: SubGroupDataCache::None,
            is_fetching_data: false,
        }
    }
}

/// A type wrapping an H2 data channel which be unavailable while out deliverying data.
enum SubChannelState {
    /// The channel is currently out as it is being used to deliver data.
    OutForDelivery,
    /// The channel is currently held in a stream monitoring its liveness.
    MonitoringLiveness,
}

/// The possible states of a subscription group's data delivery cache.
enum SubGroupDataCache {
    /// No data is currently cached.
    None,
    /// Data is cached and needs to be delivered.
    NeedsDelivery(Arc<StreamSubscribeResponse>),
    /// Data is currently being delivered to the identified channel.
    OutForDelivery((Uuid, Arc<StreamSubscribeResponse>)),
}
