//! Stream controller.
//!
//! ## Replication
//! Stream replication is purely deterministic and is scoped only to the pods of the Stream's
//! StatefulSet. Each partition is authoritative on which pods are in-sync, and client requested
//! durability is always bound to the leader's view of in-sync replicas.
//!
//! Each pod within a Stream's StatefulSet is aware of its partition offset and will
//! deterministically select peer pods for replication based on its own offset. The algorithm can
//! can be expressed visually in the following table for a replication factor of `2`.
//!
//! ```sql
//! id | p0      | p1      | p2      | p3      | p4      | p5
//! ---+---------+---------+---------+---------+---------+--------
//! r  | p1,p2   | p0,p2   | p0,p1   | p0,p1   | p2,p3   | p3,p4
//! ---+---------+---------+---------+---------+---------+--------
//! c  | 3       | 3       | 3       | 2       | 1       | 0
//! ---+---------+---------+---------+---------+---------+--------
//! a  | o+1,o+2 | o-1,o+1 | o-2,o-1 | o-3,o-2 | o-2,o-1 | o-2,o-1
//! ```
//!
//! Replication is limited to 2. When there is only 1 partition, there is no replication. When
//! there are only 2 partitions, only a single replica will be possible.

#[cfg(test)]
mod mod_test;
mod publisher;
#[cfg(test)]
mod publisher_test;
mod subscriber;
#[cfg(test)]
mod subscriber_test;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::StreamExt;
use prost::Message;
use sled::Tree;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tonic::Streaming;

use crate::config::Config;
use crate::database::Database;
use crate::error::{RpcResult, ShutdownError, ERR_DB_FLUSH, ERR_ITER_FAILURE};
use crate::grpc::{StreamEventLocation, StreamPublishRequest, StreamPublishResponse, StreamSubscribeRequest, StreamSubscribeResponse, StreamSubscribeSetup};
use crate::models::stream::Subscription;
use crate::stream::subscriber::StreamSubCtlMsg;
use crate::utils;
use hadron_core::crd::{StreamRetentionPolicy, StreamRetentionSpec};

/// The key prefix used for storing stream events.
///
/// NOTE: in order to preserve lexicographical ordering of keys, it is important to always use
/// the `utils::encode_byte_prefix*` methods.
pub const PREFIX_STREAM_EVENT: &[u8; 1] = b"e";
/// The key prefix used for storing stream event timestamps, always stored as i64 seconds timestamp.
///
/// NOTE: in order to preserve lexicographical ordering of keys, it is important to always use
/// the `utils::encode_byte_prefix*` methods.
pub const PREFIX_STREAM_TS: &[u8; 1] = b"t";
/// The database key prefix used for storing stream subscriber data.
///
/// NOTE: in order to preserve lexicographical ordering of keys, it is important to always use
/// the `utils::encode_byte_prefix*` methods.
pub const PREFIX_STREAM_SUBS: &[u8; 1] = b"s";
/// The database key prefix used for storing stream subscriber offsets.
///
/// NOTE: in order to preserve lexicographical ordering of keys, it is important to always use
/// the `utils::encode_byte_prefix*` methods.
pub const PREFIX_STREAM_SUB_OFFSETS: &[u8; 1] = b"o";
/// The key used to store the last written offset for the stream.
pub const KEY_STREAM_LAST_WRITTEN_OFFSET: &[u8; 1] = b"l";
/// The key used to store the seconds timestamp of the last compaction event.
pub const KEY_STREAM_LAST_COMPACTION: &[u8; 1] = b"c";

const COMPACTION_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60 * 30);
const ERR_DECODING_STREAM_META_GROUP_NAME: &str = "error decoding stream meta group name from storage";

pub(self) const METRIC_CURRENT_OFFSET: &str = "hadron_stream_current_offset";
pub(self) const METRIC_SUB_NUM_GROUPS: &str = "hadron_stream_subscriber_num_groups";
pub(self) const METRIC_SUB_GROUP_MEMBERS: &str = "hadron_stream_subscriber_group_members";
pub(self) const METRIC_SUB_LAST_OFFSET: &str = "hadron_stream_subscriber_last_offset_processed";
pub(self) const METRIC_LAST_COMPACTED_OFFSET: &str = "hadron_stream_last_compacted_offset";

/// A controller encapsulating all logic for interacting with a stream.
pub struct StreamCtl {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    _db: Database,
    /// This stream's database tree.
    tree: Tree,
    /// The stream partition of this controller.
    partition: u32,

    /// A channel of inbound client requests.
    requests_tx: mpsc::Sender<StreamCtlMsg>,
    /// A channel of inbound client requests.
    requests_rx: ReceiverStream<StreamCtlMsg>,
    /// A channel of requests for stream subscription.
    subs_tx: mpsc::Sender<StreamSubCtlMsg>,

    /// A channel used for communicating the stream's last written offset value.
    offset_signal: watch::Sender<u64>,
    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
    /// A handle to this controller's spawned subscription controller.
    sub_ctl: JoinHandle<Result<()>>,

    /// The last written offset of the stream.
    current_offset: u64,
    /// The earliest timestamp recorded in the stream.
    ///
    /// A `None` value indicates that no timestamp record exists on disk, and it will be populated
    /// when the next batch is written to the stream.
    earliest_timestamp: Option<(i64, u64)>,
    /// A bool indicating if the stream is currently undergoing a compaction routine.
    is_compacting: bool,
    /// The timestamp of the last compaction event.
    last_compaction_event: Option<i64>,
}

impl StreamCtl {
    /// Create a new instance.
    pub async fn new(
        config: Arc<Config>, db: Database, shutdown_tx: broadcast::Sender<()>, requests_tx: mpsc::Sender<StreamCtlMsg>, requests_rx: mpsc::Receiver<StreamCtlMsg>,
    ) -> Result<(Self, watch::Receiver<u64>)> {
        // Recover stream state.
        let partition = config.partition;
        let tree = db.get_stream_tree().await?;
        let recovery_data = recover_stream_state(tree.clone()).await?;
        metrics::register_counter!(METRIC_CURRENT_OFFSET, metrics::Unit::Count, "the offset of the last entry written to the stream");
        metrics::register_counter!(METRIC_LAST_COMPACTED_OFFSET, metrics::Unit::Count, "the offset of the last compacted event");
        metrics::counter!(METRIC_CURRENT_OFFSET, recovery_data.last_written_offset);
        metrics::counter!(METRIC_LAST_COMPACTED_OFFSET, recovery_data.first_written_offset.saturating_sub(1));

        // Spawn the subscriber controller.
        let (offset_signal, offset_signal_rx) = watch::channel(recovery_data.last_written_offset);
        let (subs_tx, subs_rx) = mpsc::channel(100);
        let sub_ctl = subscriber::StreamSubCtl::new(
            config.clone(),
            db.clone(),
            tree.clone(),
            partition,
            shutdown_tx.clone(),
            subs_tx.clone(),
            subs_rx,
            offset_signal_rx.clone(),
            recovery_data.subscriptions,
            recovery_data.last_written_offset,
        )
        .spawn();

        Ok((
            Self {
                config,
                _db: db,
                tree,
                partition,
                requests_tx,
                requests_rx: ReceiverStream::new(requests_rx),
                subs_tx,
                offset_signal,
                shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
                shutdown_tx,
                sub_ctl,
                current_offset: recovery_data.last_written_offset,
                earliest_timestamp: recovery_data.first_timestamp_opt,
                is_compacting: false,
                last_compaction_event: recovery_data.last_compaction_opt,
            },
            offset_signal_rx,
        ))
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!("stream controller {}/{} has started", self.config.stream, self.partition);

        // Calculate the delay to be used for the initial compaction event.
        let delay = calculate_initial_compaction_delay(self.last_compaction_event);
        let compaction_timer = tokio::time::sleep(delay);
        tokio::pin!(compaction_timer);

        loop {
            tokio::select! {
                msg_opt = self.requests_rx.next() => self.handle_ctl_msg(msg_opt).await,
                _ = &mut compaction_timer => {
                    compaction_timer.set(tokio::time::sleep(COMPACTION_INTERVAL));
                    self.spawn_compaction_task();
                }
                _ = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        if let Err(err) = self.sub_ctl.await {
            tracing::error!(error = ?err, "error shutting down stream subscription controller");
        }
        tracing::debug!("stream controller {}/{} has shutdown", self.config.stream, self.partition);
        Ok(())
    }

    /// Handle a stream controller message.
    #[tracing::instrument(level = "trace", skip(self, msg_opt))]
    async fn handle_ctl_msg(&mut self, msg_opt: Option<StreamCtlMsg>) {
        let msg = match msg_opt {
            Some(msg) => msg,
            None => {
                let _res = self.shutdown_tx.send(());
                return;
            }
        };
        match msg {
            StreamCtlMsg::RequestPublish { tx, request } => self.handle_publisher_request(tx, request).await,
            StreamCtlMsg::RequestSubscribe { tx, rx, setup } => self.handle_request_subscribe(tx, rx, setup).await,
            StreamCtlMsg::CompactionFinished { earliest_timestamp } => self.handle_compaction_finished(earliest_timestamp).await,
            StreamCtlMsg::UpdateEventData { data, location, tx } => self.handle_update_event_data(data, location, tx).await,
        }
    }

    /// Handle a request to setup a subscriber channel.
    async fn handle_request_subscribe(&mut self, tx: mpsc::Sender<RpcResult<StreamSubscribeResponse>>, rx: Streaming<StreamSubscribeRequest>, setup: StreamSubscribeSetup) {
        let _ = self.subs_tx.send(StreamSubCtlMsg::Request { tx, rx, setup }).await;
    }

    /// Handle a request to update the data of the target event.
    #[tracing::instrument(level = "trace", skip(self, data, location, tx))]
    async fn handle_update_event_data(&mut self, data: Vec<u8>, location: StreamEventLocation, tx: oneshot::Sender<Result<()>>) {
        let res = update_event_data(data, location, self.tree.clone()).await;
        if let Err(err) = &res {
            tracing::error!(error = ?err, "error handling request to update event data");
            if err.downcast_ref::<ShutdownError>().is_some() {
                let _ = self.shutdown_tx.send(());
            }
        }
        let _res = tx.send(res);
    }

    /// Begin a compaction routine, if possible.
    #[tracing::instrument(level = "trace", skip(self))]
    fn spawn_compaction_task(&mut self) {
        // FUTURE: instrument the compaction routine so that we can generate metrics over durations.
        if self.is_compacting {
            return;
        }
        self.is_compacting = true;
        let (config, tree, ts, stream_tx, shutdown_tx) = (self.config.clone(), self.tree.clone(), self.earliest_timestamp, self.requests_tx.clone(), self.shutdown_tx.clone());
        let _handle = tokio::spawn(async move {
            match compact_stream(config, tree, ts).await {
                Ok(earliest_timestamp) => {
                    let _res = stream_tx.send(StreamCtlMsg::CompactionFinished { earliest_timestamp }).await;
                }
                Err(err) => {
                    tracing::error!(error = ?err, "error during compaction routine, shutting down");
                    let _res = shutdown_tx.send(());
                }
            }
        });
    }

    /// Handle a compaction finalization.
    #[tracing::instrument(level = "trace", skip(self))]
    async fn handle_compaction_finished(&mut self, earliest_timestamp: Option<(i64, u64)>) {
        self.is_compacting = false;
        self.earliest_timestamp = earliest_timestamp;
    }
}

/// Update the data payload of an event of this Stream, both in memory and on disk.
#[tracing::instrument(level = "trace", skip(data, location, db))]
async fn update_event_data(data: Vec<u8>, location: StreamEventLocation, db: Tree) -> Result<()> {
    /* TODO:
    - either move this to the subscriber child controller, or combine all of these components
      under the same controller.
    - update data on disk.
    - update / purge the subscription payloads for all subscriber groups which have an event
      payload which includes a copy of the updated event.
    */
    todo!()
}

/// Calculate the initial compaction delay based on the given last compaction timestamp.
fn calculate_initial_compaction_delay(last_compaction_timestamp: Option<i64>) -> std::time::Duration {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    match last_compaction_timestamp {
        Some(last_timestamp) => {
            let delay_seconds = time::Duration::seconds(now.saturating_sub(last_timestamp));
            std::time::Duration::try_from(delay_seconds).unwrap_or_else(|err| {
                tracing::error!(error = ?err, "error converting last compaction timestamp into std duration");
                COMPACTION_INTERVAL
            })
        }
        None => COMPACTION_INTERVAL,
    }
}

/// Execute a compaction routine on the given DB tree.
///
/// If compaction is not configured for the stream, then this routine will immediately finish and
/// will return the given `earliest_timestamp` as its output.
///
/// This routine currently supports time based compaction, and its algorithm is as follows:
/// - Extract the configured TTL of the policy.
/// - Calculate an invalidation threshold based on a delta of now and the configured TTL.
/// - Search the secondary time index for all records which fall behind the threshold.
/// - Delete any records which fall behind the threshold.
/// - Return the next earliest timestamp record as output.
///
/// **NOTE: any error returned from this routine will cause a shutdown to be issued.**
#[tracing::instrument(level = "trace", skip(config, tree, earliest_timestamp))]
async fn compact_stream(config: Arc<Config>, tree: Tree, earliest_timestamp: Option<(i64, u64)>) -> Result<Option<(i64, u64)>> {
    tracing::debug!("compaction routine is starting");

    // Extract the configured retention policy.
    let ttl = match &config.retention_policy.strategy {
        StreamRetentionPolicy::Retain => return Ok(earliest_timestamp),
        StreamRetentionPolicy::Time => {
            let ttl = config.retention_policy.retention_seconds.unwrap_or_else(StreamRetentionSpec::retention_seconds_default);
            time::Duration::seconds(i64::try_from(ttl).unwrap_or(i64::MAX))
        }
    };
    let now = time::OffsetDateTime::now_utc();
    let threshold = (now - ttl).unix_timestamp();

    // Search for all timestamps falling behind the calculated threshold.
    let earliest_timestamp_opt = Database::spawn_blocking(move || -> Result<Option<(i64, u64)>> {
        // Scan over timestamp index entries, adding to batch for deletion.
        let mut batch = sled::Batch::default();
        let ts_start: &[u8] = PREFIX_STREAM_TS;
        let ts_threshold: &[u8] = &utils::encode_byte_prefix_i64(PREFIX_STREAM_TS, threshold);
        let mut last_ts_offset_opt = None; // Last event u64 offset targeted based on timestamp index.
        for kv_res in tree.range(ts_start..=ts_threshold) {
            let (key, val) = kv_res.context(ERR_ITER_FAILURE)?;
            batch.remove(key);
            last_ts_offset_opt = Some(utils::decode_u64(&val).context("error decoding timestamp index offset, data corrupted")?);
        }
        let last_ts_offset = match last_ts_offset_opt {
            Some(last_ts_offset) => last_ts_offset,
            None => return Ok(None),
        };

        // Scan over stream event entries, adding to batch for deletion, stopping at the offset
        // of the last timestamp index offset of the compaction threshold.
        let event_start: &[u8] = PREFIX_STREAM_EVENT;
        let event_stop: &[u8] = &utils::encode_byte_prefix(PREFIX_STREAM_EVENT, last_ts_offset);
        for key_res in tree.range(event_start..=event_stop).keys() {
            let key = key_res.context(ERR_ITER_FAILURE)?;
            batch.remove(key);
        }

        // Find the next timestamp index which takes the place of the earliest timestamp.
        let ts_stop: &[u8] = &utils::encode_byte_prefix_i64(PREFIX_STREAM_TS, i64::MAX);
        let next_earliest_timestamp = tree
            .range(ts_threshold..=ts_stop)
            .next()
            .transpose()
            .context(ERR_ITER_FAILURE)?
            .map(|(key, val)| -> Result<(i64, u64)> {
                let ts = utils::decode_i64(&key[1..]).context("error decoding timestamp index key, data corrupted")?;
                let offset = utils::decode_u64(&val).context("error decoding timestamp index value, data corrupted")?;
                Ok((ts, offset))
            })
            .transpose()?;

        // Record the timestamp of this compaction event.
        batch.insert(KEY_STREAM_LAST_COMPACTION, &utils::encode_i64(now.unix_timestamp()));

        // Apply the batch.
        tracing::debug!("compacting timestamp and event records up through offset {}", last_ts_offset);
        tree.apply_batch(batch).context("error applying compaction batch to stream tree")?;
        tree.flush().context(ERR_DB_FLUSH)?;
        Ok(next_earliest_timestamp)
    })
    .await
    .context("error from compaction routine")
    .and_then(|res| res)?;
    Ok(earliest_timestamp_opt)
}

/// Recover this stream's last recorded state.
async fn recover_stream_state(tree: Tree) -> Result<StreamRecoveryState> {
    let val = Database::spawn_blocking(move || -> Result<StreamRecoveryState> {
        // Fetch next offset info.
        let offset_opt = tree.get(KEY_STREAM_LAST_WRITTEN_OFFSET).context("error fetching next offset key during recovery")?;
        let last_written_offset = offset_opt
            .map(|val| utils::decode_u64(&val).context("error decoding offset value from storage"))
            .transpose()?
            .unwrap_or(0);

        // Fetch the first offset info.
        let first_written_offset = tree
            .scan_prefix(PREFIX_STREAM_EVENT)
            .keys()
            .next()
            .transpose()
            .context("error fetching first event record")?
            .map(|key| utils::decode_u64(&key[1..]))
            .transpose()?
            .unwrap_or(0);

        // Fetch first timestamp record.
        let first_timestamp_opt = tree
            .scan_prefix(PREFIX_STREAM_TS)
            .next()
            .transpose()
            .context("error fetching first timestamp record")?
            .map(|(key, val)| -> Result<(i64, u64)> {
                let timestamp = utils::decode_i64(&key[1..])?;
                let offset = utils::decode_u64(&val)?;
                Ok((timestamp, offset))
            })
            .transpose()?;

        // Fetch timestamp of last compaction event.
        let last_compaction_opt = tree
            .get(KEY_STREAM_LAST_COMPACTION)
            .context("error fetching last compaction key during recovery")?
            .map(|val| utils::decode_i64(&val).context("error decoding last compaction event timestamp, data corrupted"))
            .transpose()?;

        // Fetch all stream subscriber info.
        let mut subs = HashMap::new();
        for entry_res in tree.scan_prefix(PREFIX_STREAM_SUBS) {
            let (key, val) = entry_res.context(ERR_ITER_FAILURE)?;
            let group_name = std::str::from_utf8(key.as_ref())
                .context(ERR_DECODING_STREAM_META_GROUP_NAME)?
                .strip_prefix(unsafe { std::str::from_utf8_unchecked(PREFIX_STREAM_SUBS) })
                .unwrap_or("")
                .to_string();
            let sub = Subscription::decode(val.as_ref()).context("error decoding subscriber record from storage")?;
            subs.insert(group_name, (sub, 0));
        }

        // Fetch all stream subscriber offsets.
        for entry_res in tree.scan_prefix(PREFIX_STREAM_SUB_OFFSETS) {
            let (key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
            let group_name = std::str::from_utf8(key.as_ref())
                .context(ERR_DECODING_STREAM_META_GROUP_NAME)?
                .strip_prefix(unsafe { std::str::from_utf8_unchecked(PREFIX_STREAM_SUB_OFFSETS) })
                .unwrap_or("");
            let offset = utils::decode_u64(entry.as_ref()).context("error decoding stream offset from storage")?;
            if let Some((_sub, offset_val)) = subs.get_mut(group_name) {
                *offset_val = offset;
            }
        }

        let subscriptions: Vec<_> = subs.into_iter().map(|(_, val)| val).collect();
        Ok(StreamRecoveryState {
            first_written_offset,
            last_written_offset,
            subscriptions,
            first_timestamp_opt,
            last_compaction_opt,
        })
    })
    .await??;
    Ok(val)
}

/// A representation of a Stream's state recovered from disk on startup.
struct StreamRecoveryState {
    /// The first offset still present on disk, or `0` if no entry found.
    first_written_offset: u64,
    /// The last offset to have been written to disk, or `0` if no entry found.
    last_written_offset: u64,
    /// All stream subscriber info.
    subscriptions: Vec<(Subscription, u64)>,
    /// The first timestamp record found, if any.
    first_timestamp_opt: Option<(i64, u64)>,
    /// The timestamp of the last compaction event.
    last_compaction_opt: Option<i64>,
}

/// A message bound for a stream controller.
pub enum StreamCtlMsg {
    /// A client request to setup a publisher channel.
    RequestPublish {
        tx: oneshot::Sender<RpcResult<StreamPublishResponse>>,
        request: StreamPublishRequest,
    },
    /// A client request to setup a subscriber channel.
    RequestSubscribe {
        tx: mpsc::Sender<RpcResult<StreamSubscribeResponse>>,
        rx: Streaming<StreamSubscribeRequest>,
        setup: StreamSubscribeSetup,
    },
    /// A compaction routine has finished.
    CompactionFinished {
        /// The new earliest timestamp of data on disk.
        earliest_timestamp: Option<(i64, u64)>,
    },
    /// A client request to update the data of a target event record.
    UpdateEventData {
        /// The replacement data.
        data: Vec<u8>,
        /// The location of the event to be updated.
        location: StreamEventLocation,
        /// The response channel.
        tx: oneshot::Sender<Result<()>>,
    },
}
