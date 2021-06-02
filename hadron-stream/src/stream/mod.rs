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

mod publisher;
mod subscriber;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::{FuturesUnordered, StreamExt};
use prost::Message;
use sled::Tree;
use tokio::sync::{broadcast, mpsc, watch};
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

/// The database key prefix used for storing stream subscriber data.
const PREFIX_STREAM_SUBS: &str = "/subscribers/";
/// The database key prefix used for storing stream subscriber data.
const PREFIX_STREAM_SUB_OFFSETS: &str = "/subscriber_offsets/";

const ERR_DECODING_STREAM_META_GROUP_NAME: &str = "error decoding stream meta group name from storage";

/// A controller encapsulating all logic for interacting with a stream.
pub struct StreamCtl {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    _db: Database,
    /// This stream's database tree.
    tree: Tree,
    /// This stream's database tree for metadata storage.
    _tree_metadata: Tree,
    /// The stream partition of this controller.
    partition: u32,

    /// A channel of inbound client requests.
    requests: ReceiverStream<StreamCtlMsg>,
    /// A stream of all publisher channels sending in data to be published.
    publishers: futures::stream::FuturesUnordered<publisher::PublisherFut>,
    /// A channel of requests for stream subscription.
    subs_tx: mpsc::Sender<StreamSubCtlMsg>,

    /// A channel used for communicating the stream's `next_offset` value.
    offset_signal: watch::Sender<u64>,
    /// A channel used for triggering graceful shutdown.
    _shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
    /// A bool indicating that this controller has been descheduled and needs to shutdown.
    descheduled: bool,
    /// A handle to this controller's spawned subscription controller.
    sub_ctl: JoinHandle<Result<()>>,

    /// The next offset of the stream.
    next_offset: u64,
    /// The CloudEvents `source` field assigned to each stream event on this partition.
    source: String,
}

impl StreamCtl {
    /// Create a new instance.
    pub async fn new(
        config: Arc<Config>, db: Database, shutdown_tx: broadcast::Sender<()>, requests: mpsc::Receiver<StreamCtlMsg>,
    ) -> Result<(Self, watch::Receiver<u64>)> {
        // Recover stream state.
        let partition = config.partition;
        let tree = db.get_stream_tree().await?;
        let tree_metadata = db.get_stream_tree_metadata().await?;
        let (next_offset, subs) = recover_stream_state(&tree, &tree_metadata).await?;
        let source = format!("/{}/{}/{}", config.cluster_name, config.stream, partition);

        // Spawn the subscriber controller.
        let (offset_signal, offset_signal_rx) = watch::channel(next_offset);
        let (subs_tx, subs_rx) = mpsc::channel(100);
        let sub_ctl = subscriber::StreamSubCtl::new(
            config.clone(),
            db.clone(),
            tree.clone(),
            tree_metadata.clone(),
            partition,
            shutdown_tx.clone(),
            subs_tx.clone(),
            subs_rx,
            offset_signal_rx.clone(),
            subs,
            next_offset,
        )
        .spawn();

        Ok((
            Self {
                config,
                _db: db,
                tree,
                _tree_metadata: tree_metadata,
                partition,
                requests: ReceiverStream::new(requests),
                publishers: FuturesUnordered::new(),
                subs_tx,
                offset_signal,
                shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
                _shutdown_tx: shutdown_tx,
                descheduled: false,
                sub_ctl,
                next_offset,
                source,
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
                Some(Some(pubres)) = self.publishers.next() => self.handle_publisher_request(pubres).await,
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
            StreamCtlMsg::RequestPublish { tx, rx } => self.handle_request_publish(tx, rx).await,
            StreamCtlMsg::RequestSubscribe { tx, rx, setup } => self.handle_request_subscribe(tx, rx, setup).await,
        }
    }

    /// Handle a request to setup a publisher channel.
    async fn handle_request_publish(&mut self, tx: mpsc::Sender<RpcResult<StreamPublishResponse>>, rx: Streaming<StreamPublishRequest>) {
        self.publishers.push(publisher::PublisherFut::new((tx, rx)));
    }

    /// Handle a request to setup a subscriber channel.
    async fn handle_request_subscribe(
        &mut self, tx: mpsc::Sender<RpcResult<StreamSubscribeResponse>>, rx: Streaming<StreamSubscribeRequest>, setup: StreamSubscribeSetup,
    ) {
        let _ = self.subs_tx.send(StreamSubCtlMsg::Request { tx, rx, setup }).await;
    }
}

/// Recover this stream's last recorded state.
async fn recover_stream_state(stream_tree: &Tree, metadata_tree: &Tree) -> Result<(u64, Vec<(Subscription, u64)>)> {
    let (stream_tree, metadata_tree) = (stream_tree.clone(), metadata_tree.clone());
    let val = Database::spawn_blocking(move || -> Result<(u64, Vec<(Subscription, u64)>)> {
        // Fetch next offset info.
        let kv_opt = stream_tree
            .last()
            .context("error fetching next offset key during recovery")?;
        let next_offset = kv_opt
            .map(|(key, _val)| utils::decode_u64(&key).context("error decoding next offset value from storage"))
            .transpose()?
            .map(|val| val + 1) // We need to know the next offset, not the last written.
            // Always start at `1`, not `0`, as this makes subscriber initialization more simple.
            .unwrap_or(1);

        // Fetch all stream subscriber info.
        let mut subs = HashMap::new();
        for entry_res in metadata_tree.scan_prefix(PREFIX_STREAM_SUBS) {
            let (key, val) = entry_res.context(ERR_ITER_FAILURE)?;
            let group_name = std::str::from_utf8(key.as_ref())
                .context(ERR_DECODING_STREAM_META_GROUP_NAME)?
                .strip_prefix(PREFIX_STREAM_SUBS)
                .unwrap_or("")
                .to_string();
            let sub = Subscription::decode(val.as_ref()).context("error decoding subscriber record from storage")?;
            subs.insert(group_name, (sub, 0));
        }

        // Fetch all stream subscriber offsets.
        for entry_res in metadata_tree.scan_prefix(PREFIX_STREAM_SUB_OFFSETS) {
            let (key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
            let group_name = std::str::from_utf8(key.as_ref())
                .context(ERR_DECODING_STREAM_META_GROUP_NAME)?
                .strip_prefix(PREFIX_STREAM_SUB_OFFSETS)
                .unwrap_or("");
            let offset = utils::decode_u64(entry.as_ref()).context("error decoding stream offset from storage")?;
            if let Some((_sub, offset_val)) = subs.get_mut(group_name) {
                *offset_val = offset;
            }
        }

        let subs: Vec<_> = subs.into_iter().map(|(_, val)| val).collect();
        Ok((next_offset, subs))
    })
    .await??;
    Ok(val)
}

/// A message bound for a stream controller.
pub enum StreamCtlMsg {
    /// A client request to setup a publisher channel.
    RequestPublish {
        tx: mpsc::Sender<RpcResult<StreamPublishResponse>>,
        rx: Streaming<StreamPublishRequest>,
    },
    /// A client request to setup a subscriber channel.
    RequestSubscribe {
        tx: mpsc::Sender<RpcResult<StreamSubscribeResponse>>,
        rx: Streaming<StreamSubscribeRequest>,
        setup: StreamSubscribeSetup,
    },
}
