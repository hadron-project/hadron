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
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::config::Config;
use crate::database::Database;
use crate::error::{AppError, ERR_ITER_FAILURE};
use crate::models::{schema::Stream, stream::Subscription};
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
    db: Database,
    /// This stream's database tree.
    tree: Tree,
    /// This stream's database tree for metadata storage.
    tree_metadata: Tree,
    /// The system metadata cache.
    cache: Arc<MetadataCache>,
    /// The data model of the stream with which this controller is associated.
    stream: Arc<Stream>,

    /// A channel of inbound client requests.
    requests: ReceiverStream<H2Channel>,
    /// A stream of all publisher channels sending in data to be published.
    publishers: futures::stream::FuturesUnordered<publisher::PublisherFut>,
    /// A channel of requests for stream subscription.
    subs_tx: mpsc::Sender<StreamSubCtlMsg>,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
    /// A handle to this controller's spawned subscription controller.
    sub_ctl: JoinHandle<Result<()>>,

    /// A general purpose reusable bytes buffer, safe for concurrent use.
    buf: BytesMut,

    /// The next offset of the stream.
    next_offset: u64,
}

impl StreamCtl {
    /// Create a new instance.
    pub async fn new(
        config: Arc<Config>, db: Database, cache: Arc<MetadataCache>, stream: Arc<Stream>, shutdown_tx: broadcast::Sender<()>,
        requests: mpsc::Receiver<H2Channel>,
    ) -> Result<Self> {
        // Recover stream state.
        let tree = db.get_stream_tree(&stream.metadata.namespace, &stream.metadata.name).await?;
        let tree_metadata = db.get_stream_tree_metadata(&stream.metadata.namespace, &stream.metadata.name).await?;
        let (next_offset, subs) = recover_stream_state(&tree, &tree_metadata).await?;

        // Spawn the subscriber controller.
        let (subs_tx, subs_rx) = mpsc::channel(100);
        let sub_ctl = subscriber::StreamSubCtl::new(
            config.clone(),
            db.clone(),
            tree.clone(),
            tree_metadata.clone(),
            cache.clone(),
            stream.clone(),
            shutdown_tx.clone(),
            subs_tx.clone(),
            subs_rx,
            subs,
            next_offset,
        )
        .spawn();

        Ok(Self {
            config,
            db,
            tree,
            tree_metadata,
            cache,
            stream,
            requests: ReceiverStream::new(requests),
            publishers: FuturesUnordered::new(),
            subs_tx,
            shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
            shutdown_tx,
            sub_ctl,
            buf: BytesMut::with_capacity(5000),
            next_offset,
        })
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!(
            "stream controller {}/{} has started",
            self.stream.metadata.namespace,
            self.stream.metadata.name
        );

        loop {
            tokio::select! {
                Some(request) = self.requests.next() => self.handle_request(request).await,
                Some(Some(pubres)) = self.publishers.next() => self.handle_publisher_request(pubres).await,
                Some(_) = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        if let Err(err) = self.sub_ctl.await {
            tracing::error!(error = ?err, "error shutting down subscription controller for {}/{}",
                self.stream.metadata.namespace,
                self.stream.metadata.name
            );
        }
        tracing::debug!(
            "stream controller {}/{} has shutdown",
            self.stream.metadata.namespace,
            self.stream.metadata.name
        );
        Ok(())
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
                self.publishers.push(publisher::PublisherFut::new((req.0.into_body(), res_chan)));
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
        let kv_opt = stream_tree.last().context("error fetching next offset key during recovery")?;
        let next_offset = kv_opt
            .map(|(key, _val)| utils::decode_u64(&key).context("error decoding next offset value from storage"))
            .transpose()?
            .map(|val| val + 1) // We need to know the next offset, not the last written.
            .unwrap_or(0);

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
            if let Some((sub, offset_val)) = subs.get_mut(group_name) {
                *offset_val = offset;
            }
        }

        let subs: Vec<_> = subs.into_iter().map(|(_, val)| val).collect();
        Ok((next_offset, subs))
    })
    .await??;
    Ok(val)
}
