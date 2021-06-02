mod publisher;
mod subscriber;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::BytesMut;
use futures::stream::{FuturesUnordered, StreamExt};
use prost::Message;
use proto::v1::{StreamPubSetupResponse, StreamPubSetupResponseResult, URL_STREAM_PUBLISH, URL_STREAM_SUBSCRIBE};
use sled::Tree;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::config::Config;
use crate::crd::{RequiredMetadata, Stream};
use crate::database::Database;
use crate::error::{AppError, ERR_ITER_FAILURE};
use crate::models::stream::Subscription;
use crate::server::stream::subscriber::StreamSubCtlMsg;
use crate::server::{send_error, H2Channel, MetadataCache};
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
    /// The system metadata cache.
    cache: Arc<MetadataCache>,
    /// The data model of the stream with which this controller is associated.
    stream: Arc<Stream>,
    /// The stream partition of this controller.
    partition: u8,

    /// A channel of inbound client requests.
    requests: ReceiverStream<StreamCtlMsg>,
    /// A stream of all publisher channels sending in data to be published.
    publishers: futures::stream::FuturesUnordered<publisher::PublisherFut>,
    /// A channel of requests for stream subscription.
    subs_tx: mpsc::Sender<StreamSubCtlMsg>,

    /// A channel used for communicating the stream's `next_offset` value.
    offset_signal: watch::Sender<u64>,
    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
    /// A bool indicating that this controller has been descheduled and needs to shutdown.
    descheduled: bool,
    /// A handle to this controller's spawned subscription controller.
    sub_ctl: JoinHandle<Result<()>>,

    /// A general purpose reusable bytes buffer, safe for concurrent use.
    buf: BytesMut,

    /// The next offset of the stream.
    next_offset: u64,
    /// The CloudEvents `source` identifier used for events published to this stream partition.
    source_identifier: String,
}

impl StreamCtl {
    /// Create a new instance.
    pub async fn new(
        config: Arc<Config>, db: Database, cache: Arc<MetadataCache>, stream: Arc<Stream>, partition: u8, shutdown_tx: broadcast::Sender<()>,
        requests: mpsc::Receiver<StreamCtlMsg>,
    ) -> Result<(Self, watch::Receiver<u64>)> {
        // Recover stream state.
        let tree = db.get_stream_tree(stream.name(), partition).await?;
        let tree_metadata = db.get_stream_tree_metadata(stream.name(), partition).await?;
        let (next_offset, subs) = recover_stream_state(&tree, &tree_metadata).await?;
        let source_identifier = format!("/{}/{}/{}/", config.cluster, stream.name(), partition);

        // Spawn the subscriber controller.
        let (offset_signal, offset_signal_rx) = watch::channel(next_offset);
        let (subs_tx, subs_rx) = mpsc::channel(100);
        let sub_ctl = subscriber::StreamSubCtl::new(
            config.clone(),
            db.clone(),
            tree.clone(),
            tree_metadata.clone(),
            cache.clone(),
            stream.clone(),
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
                cache,
                stream,
                partition,
                requests: ReceiverStream::new(requests),
                publishers: FuturesUnordered::new(),
                subs_tx,
                offset_signal,
                shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
                shutdown_tx,
                descheduled: false,
                sub_ctl,
                buf: BytesMut::with_capacity(5000),
                next_offset,
                source_identifier,
            },
            offset_signal_rx,
        ))
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!("stream controller {}/{} has started", self.stream.name(), self.partition);

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
            tracing::error!(error = ?err, "error shutting down subscription controller for {}/{}",
                self.stream.name(),
                self.partition,
            );
        }
        tracing::debug!("stream controller {}/{} has shutdown", self.stream.name(), self.partition);
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
            StreamCtlMsg::Request(req) => self.handle_request(req).await,
            StreamCtlMsg::StreamUpdated(stream) => self.handle_stream_updated(stream).await,
            StreamCtlMsg::StreamDeleted(stream) => self.handle_stream_deleted(stream).await,
        }
    }

    /// Handle an update to the stream model.
    ///
    /// ## Updates
    /// - When the number of partitions is increased for a stream, that will be reflected in this
    /// model, but does not represent an action which needs to be taken by this instance.
    /// - When the number of replicas increases, the scheduler will eventually assign more
    /// replicas per partition leader, and once those assignments have been made, this controller
    /// will need to begin replicating data to the new replicas, if this node is the leader.
    /// - When a stream's TTL info is changed, the TTL system will need to take into account the
    /// new TTL value.
    #[tracing::instrument(level = "trace", skip(self, new))]
    async fn handle_stream_updated(&mut self, new: Arc<Stream>) {
        self.stream = new;
        // TODO[replication]: implement replication & ttl system.
        // TODO[scheduling]: react to leadership state. Shutdown if needed.
    }

    /// Handle non-system shutdown of this controller.
    #[tracing::instrument(level = "trace", skip(self, new))]
    async fn handle_stream_deleted(&mut self, new: Arc<Stream>) {
        self.stream = new;
        self.descheduled = true;
        let _ = self.subs_tx.send(StreamSubCtlMsg::Shutdown).await;
    }

    /// Handle a request which has been sent to this controller.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_request(&mut self, mut req: H2Channel) {
        let path = req.0.uri().path().split('/').last().unwrap_or("");
        match path {
            URL_STREAM_PUBLISH => {
                let res_chan = match self.validate_publisher_channel(&mut req).await {
                    Ok(res_chan) => res_chan,
                    Err(err) => {
                        send_error(&mut req, self.buf.split(), err, |e| StreamPubSetupResponse {
                            result: Some(StreamPubSetupResponseResult::Err(e)),
                        });
                        return;
                    }
                };
                self.publishers
                    .push(publisher::PublisherFut::new((req.0.into_body(), res_chan)));
            }
            URL_STREAM_SUBSCRIBE => {
                // Send request over to the subscriber controller.
                let _ = self.subs_tx.send(StreamSubCtlMsg::Request(req)).await;
            }
            _ => send_error(&mut req, self.buf.split(), AppError::ResourceNotFound.into(), std::convert::identity),
        };
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
    /// A client request being routed to the controller.
    Request(H2Channel),
    /// An update to the controller's stream object.
    StreamUpdated(Arc<Stream>),
    /// An update indicating that this stream has been deleted.
    StreamDeleted(Arc<Stream>),
}
