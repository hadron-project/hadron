mod publisher;

use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use futures::stream::{FuturesUnordered, StreamExt};
use http::Method;
use proto::v1::{URL_STREAM_PUBLISH, URL_STREAM_SUBSCRIBE};
use sled::Tree;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::config::Config;
use crate::database::Database;
use crate::error::AppError;
use crate::models::{prelude::*, schema::Stream};
use crate::server::{must_get_token, require_method, send_error, H2Channel, MetadataCache};
use crate::utils;

/// An H2 channel where both ends are streaming bidirectional data.
type H2DataChannel = (h2::RecvStream, h2::SendStream<Bytes>);

/// The database key used for storing the next available offset.
const KEY_NEXT_OFFSET: &str = "/meta/next_offset";

pub struct StreamCtl {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,
    /// This stream's database tree.
    tree: Tree,
    /// The system metadata cache.
    cache: Arc<MetadataCache>,
    /// The data model of the stream with which this controller is associated.
    stream: Arc<Stream>,

    /// A channel of inbound client requests.
    requests: ReceiverStream<H2Channel>,
    /// A stream of all publisher channels sending in data to be published.
    publishers: futures::stream::FuturesUnordered<publisher::PublisherFut>,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,

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
        let next_offset = recover_stream_state(&tree).await?;

        Ok(Self {
            config,
            db,
            tree,
            cache,
            stream,
            requests: ReceiverStream::new(requests),
            publishers: FuturesUnordered::new(),
            shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
            shutdown_tx,
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
        tracing::debug!(
            path = req.0.uri().path(),
            final_segment = path,
            "handling request in stream controller {}/{}",
            self.stream.namespace(),
            self.stream.name()
        );
        match path {
            URL_STREAM_PUBLISH => {
                tracing::debug!("setting up new publisher channel");
                let res_chan = match self.validate_publisher_channel(&mut req).await {
                    Ok(res_chan) => res_chan,
                    Err(err) => {
                        send_error(&mut req, self.buf.split(), err);
                        return;
                    }
                };
                self.publishers.push(publisher::PublisherFut::new((req.0.into_body(), res_chan)));
            }
            URL_STREAM_SUBSCRIBE => {
                if let Err(err) = self.setup_subscriber_channel(&mut req).await {
                    send_error(&mut req, self.buf.split(), err);
                }
            }
            _ => send_error(&mut req, self.buf.split(), AppError::ResourceNotFound.into()),
        };
    }

    /// Setup a stream subscriber channel.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn setup_subscriber_channel(&mut self, req: &mut H2Channel) -> Result<()> {
        let (ref mut req_chan, _res_chan) = req;
        require_method(&req_chan, Method::POST)?;
        let _creds = must_get_token(&req_chan, self.config.as_ref())?;

        /* TODO:
        ## Conn
        - after the subscriber is setup, this controller will be pushing data out to subscribers.
        - need to ensure that we can push data to subscribers without blocking this task, and that
          as the subscriber acks their work, we can update their offsets here.

        ## Data
        - need to persist info on the new consumer, so that we can track its offsets in a durable fashion.
        */
        anyhow::bail!("hacking")
    }
}

/// Recover this stream's last recorded state.
///
/// This routine is responsible for recovering the following data:
/// - The next offset available for use.
/// - (TODO:) The offsets of all consumers of the stream.
async fn recover_stream_state(db: &Tree) -> Result<u64> {
    let db = db.clone();
    let val = Database::spawn_blocking(move || -> Result<u64> {
        let key_opt = db.get(KEY_NEXT_OFFSET).context("error fetching next offset key during recovery")?;
        Ok(key_opt
            .map(|key| utils::decode_u64(&key).context("error decoding next offset value from storage"))
            .transpose()?
            .unwrap_or(0))
    })
    .await??;
    Ok(val)
}
