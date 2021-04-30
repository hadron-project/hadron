use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

use anyhow::{bail, Context, Result};
use bytes::{Bytes, BytesMut};
use futures::stream::StreamExt;
use http::Method;
use prost::Message;
use proto::v1::{
    Record, StreamSubDelivery, StreamSubDeliveryResponse, StreamSubDeliveryResponseResult, StreamSubSetupRequest, StreamSubSetupRequestStartingPoint,
    StreamSubSetupResponse, StreamSubSetupResponseResult,
};
use rand::seq::IteratorRandom;
use sled::{IVec, Tree};
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
use crate::models::{schema::Stream, stream::Subscription};
use crate::server::stream::{PREFIX_STREAM_SUBS, PREFIX_STREAM_SUB_OFFSETS};
use crate::server::{must_get_token, require_method, send_error, H2Channel, H2DataChannel, MetadataCache};
use crate::utils;

/// A stream controller's child controller for managing subscriptions.
pub struct StreamSubCtl {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,
    /// This stream's database tree for storing stream records.
    tree: Tree,
    /// This stream's database tree for metadata storage.
    tree_metadata: Tree,
    /// The system metadata cache.
    metadata: Arc<MetadataCache>,
    /// The data model of the stream with which this controller is associated.
    stream: Arc<Stream>,

    /// Data on all subscriptions and active subscribers.
    subs: SubscriberInfo,
    /// The parent stream's next offset.
    next_offset: u64,

    /// A channel of events to be processed by this controller.
    events_tx: mpsc::Sender<StreamSubCtlMsg>,
    /// A channel of events to be processed by this controller.
    events_rx: ReceiverStream<StreamSubCtlMsg>,
    /// A signal from the parent stream on the stream's current `next_offset` value.
    stream_offset: WatchStream<u64>,
    /// A stream of liveness checks on the active subscriber channels.
    liveness_checks: StreamMap<Uuid, LivenessStream>,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,

    /// A general purpose reusable bytes buffer, safe for concurrent use.
    buf: BytesMut,
}

impl StreamSubCtl {
    /// Create a new instance.
    pub fn new(
        config: Arc<Config>, db: Database, tree: Tree, tree_metadata: Tree, metadata: Arc<MetadataCache>, stream: Arc<Stream>,
        shutdown_tx: broadcast::Sender<()>, events_tx: mpsc::Sender<StreamSubCtlMsg>, events_rx: mpsc::Receiver<StreamSubCtlMsg>,
        stream_offset: watch::Receiver<u64>, subs: Vec<(Subscription, u64)>, next_offset: u64,
    ) -> Self {
        let subs = SubscriberInfo::new(subs);
        Self {
            config,
            db,
            tree,
            tree_metadata,
            metadata,
            stream,
            subs,
            next_offset,
            events_tx,
            events_rx: ReceiverStream::new(events_rx),
            stream_offset: WatchStream::new(stream_offset),
            liveness_checks: StreamMap::new(),
            shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
            shutdown_tx,
            buf: BytesMut::with_capacity(5000),
        }
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!(
            "stream subscriber controller {}/{} has started",
            self.stream.metadata.namespace,
            self.stream.metadata.name
        );

        loop {
            tokio::select! {
                Some(msg) = self.events_rx.next() => self.handle_msg(msg).await,
                Some(offset) = self.stream_offset.next() => self.handle_offset_update(offset).await,
                Some(dead_chan) = self.liveness_checks.next() => self.handle_dead_subscriber(dead_chan.1).await,
                Some(_) = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        tracing::debug!(
            "stream subscriber controller {}/{} has shutdown",
            self.stream.metadata.namespace,
            self.stream.metadata.name
        );
        Ok(())
    }

    /// Handle a message sent to this controller from its parent stream controller.
    #[tracing::instrument(level = "trace", skip(self, msg))]
    async fn handle_msg(&mut self, msg: StreamSubCtlMsg) {
        match msg {
            StreamSubCtlMsg::Request(req) => self.handle_request(req).await,
            StreamSubCtlMsg::FetchStreamRecords(res) => self.handle_fetch_stream_records_result(res).await,
            StreamSubCtlMsg::DeliveryResponse(res) => self.handle_delivery_response(res).await,
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
    async fn handle_dead_subscriber(&mut self, dead_chan: (Arc<String>, Uuid, H2DataChannel)) {
        let (group_name, id, chan) = (dead_chan.0, dead_chan.1, dead_chan.2);
        tracing::debug!(?id, group_name = ?&*group_name, "dropping stream subscriber channel");
        self.liveness_checks.remove(&id);
        let group = match self.subs.groups.get_mut(&*group_name) {
            Some(group) => group,
            None => return,
        };
        group.active_channels.remove(&id);
        self.subs.groups.retain(|_, group| group.durable || !group.active_channels.is_empty());
    }

    /// Handle a request which has been sent to this controller.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_request(&mut self, mut req: H2Channel) {
        // Validate the inbound subscriber request.
        let (chan, sub) = match self.validate_subscriber_channel(&mut req).await {
            Ok(chan_and_sub) => chan_and_sub,
            Err(err) => {
                send_error(&mut req, self.buf.split(), err, |e| StreamSubSetupResponse {
                    result: Some(StreamSubSetupResponseResult::Err(e)),
                });
                return;
            }
        };

        // Ensure the subscription is properly recorded.
        let group = match self.ensure_subscriber_record(&sub).await {
            Ok(group) => group,
            Err(err) => {
                if err.downcast_ref::<ShutdownError>().is_some() {
                    let _ = self.shutdown_tx.send(());
                }
                send_error(&mut req, self.buf.split(), err, |e| StreamSubSetupResponse {
                    result: Some(StreamSubSetupResponseResult::Err(e)),
                });
                return;
            }
        };
        let group_name = group.group_name.clone();

        // Roll a new ID for the channel & add it to the group's active channels.
        let h2_data_chan = (req.0.into_body(), chan);
        let id = Uuid::new_v4();
        group.active_channels.insert(id, SubChannel::MonitoringLiveness);
        self.liveness_checks.insert(
            id,
            LivenessStream {
                chan: Some(h2_data_chan),
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
        let last_included_offset = match fetched_data.data.last().map(|(offset, _)| *offset) {
            Some(offset) => offset,
            None => {
                tracing::debug!("empty fetch payload returned from stream fetch");
                return;
            }
        };
        let mut buf = self.buf.split();
        let msg = StreamSubDelivery {
            last_included_offset,
            batch: fetched_data
                .data
                .iter()
                .map(|(offset, data)| Record {
                    offset: *offset,
                    data: data.to_vec(), // TODO: too much copying ... do something else.
                })
                .collect(),
        };
        if let Err(err) = msg.encode(&mut buf) {
            tracing::error!(error = ?err, "error encoding subscription delivery payload");
            return;
        }
        let msg_bytes = buf.freeze();
        group.delivery_cache = SubGroupDataCache::NeedsDelivery(Arc::new((msg_bytes, last_included_offset)));

        // Attempt to deliver the data.
        Self::try_deliver_data_to_sub(group, &mut self.liveness_checks, self.events_tx.clone()).await;
    }

    /// Handle the response from a subscriber following a data payload delivery.
    #[tracing::instrument(level = "trace", skip(self, res), fields(group = ?res.group_name, last_offset = res.orig_data.1))]
    async fn handle_delivery_response(&mut self, mut res: DeliveryResponse) {
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

    /// Validate a stream subscriber channel before full setup.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn validate_subscriber_channel(&mut self, req: &mut H2Channel) -> Result<(h2::SendStream<Bytes>, StreamSubSetupRequest)> {
        require_method(&req.0, Method::POST)?;
        let creds = must_get_token(&req.0, self.config.as_ref())?;
        let claims = self.metadata.must_get_token_claims(&creds.claims.id.as_u128())?;
        claims.check_stream_sub_auth(&self.stream.metadata.namespace, &self.stream.metadata.name)?;

        // Read initial body so that we know the name of the subscriber.
        let mut body_req = req
            .0
            .body_mut()
            .data()
            .await
            .context("no body received while setting up subscriber channel")?
            .context("error awaiting request body while setting up subscriber channel")?;
        let body = StreamSubSetupRequest::decode(&mut body_req).context("error decoding stream sub setup request")?;
        tracing::debug!("finished validating subscriber setup for subscriber: {:?}", body);

        // Validate contents of setup request.
        if body.group_name.is_empty() {
            bail!(AppError::InvalidInput("subscriber group name may not be an empty string".into()));
        }
        if body.max_batch_size == 0 {
            bail!(AppError::InvalidInput("subscriber batch size must be greater than 0".into()));
        }

        // Respond to subscriber to let them know that the channel is ready for use.
        let mut res = http::Response::new(());
        *res.status_mut() = http::StatusCode::OK;
        let mut res_chan = req
            .1
            .send_response(res, false)
            .context("error returning response to subscriber for channel setup")?;
        let setup_res = StreamSubSetupResponse {
            result: Some(StreamSubSetupResponseResult::Ok(Default::default())),
        };
        let mut setup_res_buf = self.buf.split();
        setup_res.encode(&mut setup_res_buf).context("error encoding stream sub setup response")?;
        res_chan
            .send_data(setup_res_buf.freeze(), false)
            .context("error sending stream sub setup response")?;
        Ok((res_chan, body))
    }

    /// Ensure a subscription record exists if it does not already exist in the index.
    #[tracing::instrument(level = "trace", skip(self, sub))]
    async fn ensure_subscriber_record(&mut self, sub: &StreamSubSetupRequest) -> Result<&mut SubscriptionGroup> {
        // Get a handle to the group subscriber data, creating one if not present.
        let already_exists = self.subs.groups.contains_key(&sub.group_name);
        let stream_current_offset = self.next_offset - 1; // Underflow not possible, see `recover_stream_state`.
        let offset = match sub.starting_point {
            Some(StreamSubSetupRequestStartingPoint::Beginning(_)) => 0,
            Some(StreamSubSetupRequestStartingPoint::Latest(_)) => stream_current_offset,
            Some(StreamSubSetupRequestStartingPoint::Offset(offset)) => {
                let offset = if offset == 0 { 0 } else { offset - 1 };
                if offset > stream_current_offset {
                    stream_current_offset
                } else {
                    offset
                }
            }
            None => stream_current_offset,
        };
        let mut entry = self.subs.groups.entry(sub.group_name.clone())
            // Ensure the subscription model exists.
            .or_insert_with(|| {
                let durable = sub.durable;
                let sub = Subscription {
                    group_name: sub.group_name.clone(),
                    max_batch_size: sub.max_batch_size,
                };
                SubscriptionGroup::new(sub, offset, durable)
            });

        // If the subscription is durable & did not already exist, then write the subscription model to disk.
        if sub.durable && !already_exists {
            let tree_metadata = self.tree_metadata.clone();
            let mut buf = self.buf.split();
            entry.subscription.encode(&mut buf).context("error encoding subscription record")?;
            let stream_model_key = format!("{}{}", PREFIX_STREAM_SUBS, sub.group_name);
            let stream_offset_key = format!("{}{}", PREFIX_STREAM_SUB_OFFSETS, sub.group_name);
            Database::spawn_blocking(move || -> ShutdownResult<()> {
                tree_metadata
                    .insert(stream_model_key.as_bytes(), buf.freeze().as_ref())
                    .context("error writing subscription record to disk")
                    .map_err(ShutdownError::from)?;
                tree_metadata
                    .insert(stream_offset_key.as_bytes(), &utils::encode_u64(offset))
                    .context("error writing subscription offset to disk")
                    .map_err(ShutdownError::from)?;
                tree_metadata.flush().context(ERR_DB_FLUSH).map_err(ShutdownError::from)?;
                Ok(())
            })
            .await??;
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
        for (name, group) in self.subs.groups.iter_mut() {
            // If the gruop has no active channels, then skip it.
            if group.active_channels.is_empty() {
                continue;
            }

            // If the group has an idel delivery cache, attempt to deliver it.
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
            if group.offset >= self.next_offset {
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
    async fn try_handle_delivery_response(&mut self, mut delivery_res: DeliveryResponse) -> Result<()> {
        // Get a mutable handle to subscription group to which this response applies.
        let (chan_id, group_name) = (&delivery_res.id, &*delivery_res.group_name);
        let group = self
            .subs
            .groups
            .get_mut(group_name)
            .context("response from subscription delivery dropped as group no longer exists")?;

        // Unpack response body.
        let last_offset = delivery_res.orig_data.1;
        group.delivery_cache = SubGroupDataCache::NeedsDelivery(delivery_res.orig_data);
        let res = delivery_res
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
                    group: group.group_name.clone(),
                    chan_id: *chan_id,
                },
            );
        }

        // Decode the response body and record the result.
        let response_msg = StreamSubDeliveryResponse::decode(&mut body).context("malformed response returned from subscriber")?;
        let record_res = match response_msg.result {
            // If ack'ed, then we don't redeliver the data.
            Some(StreamSubDeliveryResponseResult::Ack(_)) => {
                group.delivery_cache = SubGroupDataCache::None;
                group.offset = last_offset;
                Ok(last_offset)
            }
            Some(StreamSubDeliveryResponseResult::Nack(err)) => Err(err.message),
            None => Err("malformed response returned from subscriber, unknown result variant".into()),
        };
        if group.durable {
            Self::try_record_delivery_response(record_res, group.group_name.clone(), self.tree_metadata.clone())
                .await
                .context("error while recording subscriber delivery response")?;
        }
        Ok(())
    }

    /// Attempt to deliver a payload of data to the target group.
    #[tracing::instrument(level = "trace", skip(group, liveness_stream, tx))]
    async fn try_deliver_data_to_sub(
        group: &mut SubscriptionGroup, liveness_stream: &mut StreamMap<Uuid, LivenessStream>, tx: mpsc::Sender<StreamSubCtlMsg>,
    ) {
        // Get a handle to the group's cached data, else there is nothing to do here.
        let data = match &group.delivery_cache {
            SubGroupDataCache::NeedsDelivery(data) => data.clone(),
            _ => return,
        };

        // Attempt to deliver the data to one of the active channels of this group randomly.
        loop {
            // Randomly select one of the active channels for delivery.
            let mut rng = rand::thread_rng();
            let chan_id = match group.active_channels.keys().choose(&mut rng) {
                // Unpack the selected channel.
                Some(chan_id) => *chan_id,
                // Nothing to do if there are no longer any active channels.
                None => {
                    group.delivery_cache = SubGroupDataCache::None;
                    return;
                }
            };

            // Extract the channel and prep for use.
            let mut chan = match group.active_channels.remove(&chan_id) {
                Some(data) => data,
                None => continue, // This will never be hit.
            };
            let mut chan = match chan {
                // The selected channel is already out for delivery, and this should never happen
                // as the group data cache would also be marked as "out for delivery" and this method
                // would not be invoked until the delivery was resolved.
                SubChannel::OutForDelivery => {
                    tracing::error!("invariant violation: subscription channel was not properly set back into idle state");
                    continue;
                }
                // The selected channel is currently being monitored for liveness.
                SubChannel::MonitoringLiveness => match liveness_stream.remove(&chan_id).and_then(|data| data.chan) {
                    Some(chan) => chan,
                    None => {
                        tracing::error!("invariant violation: subscription channel was not properly held in liveness monitoring stream");
                        continue;
                    }
                },
            };
            group.delivery_cache = SubGroupDataCache::OutForDelivery((chan_id, data.clone()));

            // Send the data payload to the channel.
            if let Err(err) = chan.1.send_data(data.0.clone(), false) {
                tracing::error!(error = ?err, "error delivering payload to subscription channel");
                continue;
            }

            // Spawn a task to wait for the subscriber's response & handle timeouts & errors.
            group.active_channels.insert(chan_id, SubChannel::OutForDelivery);
            let (tx, group_name, id, orig_data) = (tx.clone(), group.group_name.clone(), chan_id, data);
            tokio::spawn(async move {
                // TODO: add optional timeouts here based on subscription group config.
                let output = chan.0.data().await.map(|res| res.map_err(anyhow::Error::from).map(|data| (chan, data)));
                let _ = tx
                    .send(StreamSubCtlMsg::DeliveryResponse(DeliveryResponse {
                        id,
                        group_name,
                        output,
                        orig_data,
                    }))
                    .await;
            });
            return;
        }
    }

    /// Record the ack/nack response from a subscriber delivery.
    #[tracing::instrument(level = "trace", skip(res, group_name, tree_metadata))]
    async fn try_record_delivery_response(res: std::result::Result<u64, String>, group_name: Arc<String>, tree_metadata: Tree) -> ShutdownResult<()> {
        Database::spawn_blocking(move || -> ShutdownResult<()> {
            let offset = match res {
                Ok(offset) => offset,
                Err(_err) => {
                    // TODO: in the future, we will record this for observability system.
                    return Ok(());
                }
            };
            let key = format!("{}{}", PREFIX_STREAM_SUB_OFFSETS, &*group_name);
            tree_metadata
                .insert(key.as_bytes(), &utils::encode_u64(offset))
                .context("error updating subscription offsets on disk")
                .map_err(ShutdownError::from)?;
            tree_metadata.flush().context(ERR_DB_FLUSH).map_err(ShutdownError::from)?;
            Ok(())
        })
        .await??;
        Ok(())
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
                    let (key, val) = iter_res.context(ERR_ITER_FAILURE).map_err(ShutdownError::from)?;
                    let offset = utils::decode_u64(key.as_ref()).map_err(ShutdownError::from)?;
                    data.push((offset, val));
                }
                Ok(FetchStreamRecords {
                    group_name,
                    data: Arc::new(data),
                })
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
    Request(H2Channel),
    /// A result from fetching records from the stream for subscription delivery.
    FetchStreamRecords(ShutdownResult<FetchStreamRecords>),
    /// A response from a subscriber following a delivery of data for processing.
    DeliveryResponse(DeliveryResponse),
}

pub struct FetchStreamRecords {
    group_name: Arc<String>,
    data: Arc<Vec<(u64, IVec)>>,
}

pub struct DeliveryResponse {
    /// The ID of the subscription channel.
    id: Uuid,
    /// The name of the subscription group.
    group_name: Arc<String>,
    /// The output of awaiting for the subscriber's response.
    output: Option<Result<(H2DataChannel, Bytes)>>,
    /// The original data delivered.
    orig_data: Arc<(Bytes, u64)>,
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
    active_channels: HashMap<Uuid, SubChannel>,
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
enum SubChannel {
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
    NeedsDelivery(Arc<(Bytes, u64)>),
    /// Data is currently being delivered to the identified channel.
    OutForDelivery((Uuid, Arc<(Bytes, u64)>)),
}
