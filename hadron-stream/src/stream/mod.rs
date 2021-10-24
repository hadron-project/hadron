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
use crate::error::{RpcResult, ERR_ITER_FAILURE};
use crate::grpc::{StreamPublishRequest, StreamPublishResponse, StreamSubscribeRequest, StreamSubscribeResponse, StreamSubscribeSetup};
use crate::models::stream::Subscription;
use crate::stream::subscriber::StreamSubCtlMsg;
use crate::utils;

/// The key prefix used for storing stream events.
///
/// NOTE: in order to preserve lexicographical ordering of keys, it is important to always use
/// the `utils::encode_byte_prefix*` methods.
pub const PREFIX_STREAM_EVENT: &[u8; 1] = b"e";
/// The key prefix used for storing stream event timestamps.
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

const ERR_DECODING_STREAM_META_GROUP_NAME: &str = "error decoding stream meta group name from storage";

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
    requests: ReceiverStream<StreamCtlMsg>,
    /// A channel of requests for stream subscription.
    subs_tx: mpsc::Sender<StreamSubCtlMsg>,

    /// A channel used for communicating the stream's last written offset value.
    offset_signal: watch::Sender<u64>,
    /// A channel used for triggering graceful shutdown.
    _shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
    /// A bool indicating that this controller has been descheduled and needs to shutdown.
    descheduled: bool,
    /// A handle to this controller's spawned subscription controller.
    sub_ctl: JoinHandle<Result<()>>,

    /// The last written offset of the stream.
    current_offset: u64,
}

impl StreamCtl {
    /// Create a new instance.
    pub async fn new(
        config: Arc<Config>, db: Database, shutdown_tx: broadcast::Sender<()>, requests: mpsc::Receiver<StreamCtlMsg>,
    ) -> Result<(Self, watch::Receiver<u64>)> {
        // Recover stream state.
        let partition = config.partition;
        let tree = db.get_stream_tree().await?;
        let (current_offset, subs) = recover_stream_state(tree.clone()).await?;

        // Spawn the subscriber controller.
        let (offset_signal, offset_signal_rx) = watch::channel(current_offset);
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
            subs,
            current_offset,
        )
        .spawn();

        Ok((
            Self {
                config,
                _db: db,
                tree,
                partition,
                requests: ReceiverStream::new(requests),
                subs_tx,
                offset_signal,
                shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
                _shutdown_tx: shutdown_tx,
                descheduled: false,
                sub_ctl,
                current_offset,
            },
            offset_signal_rx,
        ))
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!("stream controller {}/{} has started", self.config.stream, self.partition);

        loop {
            if self.descheduled {
                break;
            }
            tokio::select! {
                msg_opt = self.requests.next() => self.handle_ctl_msg(msg_opt).await,
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
                let _ = self.subs_tx.send(StreamSubCtlMsg::Shutdown).await;
                self.descheduled = true;
                return;
            }
        };
        match msg {
            StreamCtlMsg::RequestPublish { tx, request } => self.handle_publisher_request(tx, request).await,
            StreamCtlMsg::RequestSubscribe { tx, rx, setup } => self.handle_request_subscribe(tx, rx, setup).await,
        }
    }

    /// Handle a request to setup a subscriber channel.
    async fn handle_request_subscribe(
        &mut self, tx: mpsc::Sender<RpcResult<StreamSubscribeResponse>>, rx: Streaming<StreamSubscribeRequest>, setup: StreamSubscribeSetup,
    ) {
        let _ = self.subs_tx.send(StreamSubCtlMsg::Request { tx, rx, setup }).await;
    }
}

/// Recover this stream's last recorded state.
async fn recover_stream_state(tree: Tree) -> Result<(u64, Vec<(Subscription, u64)>)> {
    let val = Database::spawn_blocking(move || -> Result<(u64, Vec<(Subscription, u64)>)> {
        // Fetch next offset info.
        let offset_opt = tree
            .get(KEY_STREAM_LAST_WRITTEN_OFFSET)
            .context("error fetching next offset key during recovery")?;
        let last_written_offset = offset_opt
            .map(|val| utils::decode_u64(&val).context("error decoding offset value from storage"))
            .transpose()?
            .unwrap_or(0);

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

        let subs: Vec<_> = subs.into_iter().map(|(_, val)| val).collect();
        Ok((last_written_offset, subs))
    })
    .await??;
    Ok(val)
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
}
