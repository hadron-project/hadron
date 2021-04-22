//! Network server.

mod metadata;
mod stream;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::BytesMut;
use futures::stream::StreamExt;
use http::Method;
use prost::Message;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, UnboundedReceiverStream};

use crate::auth;
use crate::config::Config;
use crate::database::Database;
use crate::error::AppError;
use crate::models::events::{Event, InitialState, PipelineCreated, StreamCreated};
use crate::models::prelude::*;
use crate::models::schema::{Pipeline, Stream};
pub use crate::server::metadata::MetadataCache;
use crate::server::stream::StreamCtl;
use crate::utils;

/// An H2 channel (stream) created from an active HTTP2 connection.
type H2Channel = (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>);

/// Network server used to handle client requests.
pub struct Server {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,
    /// The system metadata cache.
    cache: Arc<MetadataCache>,
    /// A map of communication channels to active stream controllers.
    streams: HashMap<u64, mpsc::Sender<H2Channel>>,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,

    /// A MPMC channel used for system events.
    events_tx: broadcast::Sender<Arc<Event>>,
    /// A MPMC channel used for system events.
    events_rx: broadcast::Receiver<Arc<Event>>,

    /// The TCP listener used by this server.
    listener: TcpListener,
    /// The channel used to handle H2 channels as they arrive.
    channels_tx: mpsc::UnboundedSender<H2Channel>,
    /// The channel used to handle H2 channels as they arrive.
    channels_rx: UnboundedReceiverStream<H2Channel>,

    // A handle to the MetadataCtl.
    metadata_tx: mpsc::Sender<H2Channel>,
    metadata_ctl: JoinHandle<Result<()>>,

    /// A general purpose reusable bytes buffer, safe for concurrent use.
    buf: BytesMut,
}

impl Server {
    /// Create a new instance.
    pub async fn new(
        config: Arc<Config>, db: Database, shutdown_tx: broadcast::Sender<()>, events_tx: broadcast::Sender<Arc<Event>>,
        events_rx: broadcast::Receiver<Arc<Event>>,
    ) -> Result<(Self, Arc<MetadataCache>)> {
        // Build TCP listener.
        let addr = format!("0.0.0.0:{}", config.client_port);
        let listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("error creating network listener on addr {}", addr))?;

        // Spawn the metadata controller.
        let (metadata_tx, metadata_rx) = mpsc::channel(100);
        let (metadata_ctl, cache) =
            metadata::MetadataCtl::new(config.clone(), db.clone(), shutdown_tx.clone(), metadata_rx, events_tx.clone()).await?;
        let metadata_ctl = metadata_ctl.spawn();

        let (channels_tx, channels_rx) = mpsc::unbounded_channel();
        Ok((
            Self {
                config,
                db,
                cache: cache.clone(),
                streams: Default::default(),
                shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
                shutdown_tx,
                events_tx,
                events_rx,
                listener,
                channels_tx,
                channels_rx: UnboundedReceiverStream::new(channels_rx),
                metadata_tx,
                metadata_ctl,
                buf: BytesMut::with_capacity(5000),
            },
            cache,
        ))
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!("network server started");

        loop {
            tokio::select! {
                Ok((socket, addr)) = self.listener.accept() => self.spawn_connection(socket, addr),
                Some(h2chan) = self.channels_rx.next() => self.route_request(h2chan).await,
                event_res = self.events_rx.recv() => self.handle_event_res(event_res).await,
                Some(_) = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        tracing::debug!("network server shutdown");
        Ok(())
    }

    /// Spawn the given TCP connection, completing its H2 handshake & streaming in new H2 channels.
    #[tracing::instrument(level = "trace", skip(self, socket, _addr))]
    fn spawn_connection(&mut self, socket: TcpStream, _addr: SocketAddr) {
        let (tx, mut shutdown) = (self.channels_tx.clone(), BroadcastStream::new(self.shutdown_tx.subscribe()));
        tokio::spawn(async move {
            let handshake_res = h2::server::handshake(socket).await.context("error performing H2 handshake");
            let mut conn = match handshake_res {
                Ok(conn) => conn,
                Err(err) => {
                    tracing::error!(error = ?err, "error performing H2 handshake on client connection");
                    return;
                }
            };
            loop {
                tokio::select! {
                    chan_opt = conn.accept() => match chan_opt {
                        Some(Ok(h2chan)) => {
                            let _ = tx.send(h2chan);
                        },
                        Some(Err(err)) => tracing::error!(error = ?err, "error handling new H2 stream"),
                        None => break,
                    },
                    Some(_) = shutdown.next() => break,
                }
            }
        });
    }

    /// Handle a new H2 channel created from an H2 connection.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn route_request(&mut self, mut req: H2Channel) {
        // Perform top-level routing based on URI path.
        let mut path = req.0.uri().path().split('/');
        let _ = path.next(); // First segment will always be "".
        let res: Result<()> = match path.next() {
            // Handle V1 requests.
            Some(proto::v1::URL_V1) => match path.next() {
                // Metadata requests.
                Some(proto::v1::URL_METADATA) => {
                    let _ = self.metadata_tx.send(req).await;
                    return;
                }
                // Requests bound for a stream controller.
                Some(proto::v1::URL_STREAM) => match (path.next(), path.next()) {
                    (Some(ns), Some(name)) => {
                        tracing::debug!(ns, name, "handling new stream channel");
                        let stream_hash_id = utils::ns_name_hash_id(ns, name);
                        match self.streams.get(&stream_hash_id) {
                            Some(stream_tx) => {
                                tracing::debug!(ns, name, "sending channel to controller");
                                let _ = stream_tx.send(req).await;
                                return;
                            }
                            None => {
                                tracing::debug!(ns, name, "stream controller not found");
                                Err(AppError::ResourceNotFound.into())
                            }
                        }
                    }
                    _ => Err(AppError::ResourceNotFound.into()),
                },
                _ => Err(AppError::ResourceNotFound.into()),
            },
            _ => Err(AppError::ResourceNotFound.into()),
        };
        if let Err(err) = res {
            send_error(&mut req, self.buf.split(), err);
        }
    }

    /// Handle a system metadata event.
    #[tracing::instrument(level = "trace", skip(self, event_res))]
    async fn handle_event_res(&mut self, event_res: std::result::Result<Arc<Event>, broadcast::error::RecvError>) {
        let mut event_res = Some(event_res);
        loop {
            let event = match event_res.take() {
                Some(Ok(event)) => event,
                Some(Err(err)) => {
                    // Shutdown if we've run into an error on the events channel.
                    tracing::error!(error = ?err, "error consuming system metadata events, shutting down");
                    let _ = self.shutdown_tx.send(());
                    return;
                }
                None => return,
            };
            match event.as_ref() {
                Event::Initial(e) => self.handle_event_initial(e).await,
                Event::PipelineCreated(e) => self.handle_event_pipeline_created(e).await,
                Event::StreamCreated(e) => self.handle_event_stream_created(e).await,
            }
            event_res = match self.events_rx.try_recv() {
                Ok(ok) => Some(Ok(ok)),
                Err(err) => match err {
                    broadcast::error::TryRecvError::Empty => return,
                    broadcast::error::TryRecvError::Closed => Some(Err(broadcast::error::RecvError::Closed)),
                    broadcast::error::TryRecvError::Lagged(val) => Some(Err(broadcast::error::RecvError::Lagged(val))),
                },
            }
        }
    }

    /// Handle an initial metadata payload following a system recovery.
    #[tracing::instrument(level = "trace", skip(self, event))]
    async fn handle_event_initial(&mut self, event: &InitialState) {
        for stream in event.streams.iter() {
            if stream.partitions.contains(&self.config.repl_set_name) {
                self.spawn_stream_controller(stream).await;
            }
        }
        for pipeline in event.pipelines.iter() {
            if pipeline.replica_set == self.config.repl_set_name {
                self.spawn_pipeline_controller(pipeline).await;
            }
        }
    }

    /// Handle an event indicating that a new pipeline was created.
    #[tracing::instrument(level = "trace", skip(self, event))]
    async fn handle_event_pipeline_created(&mut self, event: &PipelineCreated) {
        if event.pipeline.replica_set == self.config.repl_set_name {
            self.spawn_pipeline_controller(&event.pipeline).await;
        }
    }

    /// Handle an event indicating that a new stream was created.
    #[tracing::instrument(level = "trace", skip(self, event))]
    async fn handle_event_stream_created(&mut self, event: &StreamCreated) {
        if event.stream.partitions.contains(&self.config.repl_set_name) {
            self.spawn_stream_controller(&event.stream).await;
        }
    }

    /// Spawn a stream controller.
    #[tracing::instrument(level = "trace", skip(self, stream))]
    async fn spawn_stream_controller(&mut self, stream: &Arc<Stream>) {
        let (tx, rx) = mpsc::channel(10_000);
        let stream_res = StreamCtl::new(
            self.config.clone(),
            self.db.clone(),
            self.cache.clone(),
            stream.clone(),
            self.shutdown_tx.clone(),
            rx,
        )
        .await
        .context("error spawning stream controller");
        let ctl = match stream_res {
            Ok(ctl) => ctl,
            Err(err) => {
                tracing::error!(
                    error = ?err,
                    "error spawning stream controller for {}/{}",
                    stream.metadata.namespace,
                    stream.metadata.name
                );
                let _ = self.shutdown_tx.send(());
                return;
            }
        };
        let _ctl_handle = ctl.spawn();
        let _ = self.streams.insert(stream.hash_id(), tx);
    }

    /// Spawn a pipeline controller.
    #[tracing::instrument(level = "trace", skip(self, pipeline))]
    async fn spawn_pipeline_controller(&mut self, pipeline: &Arc<Pipeline>) {
        tracing::info!(?pipeline, "spawning pipeline controller");
    }
}

/// Handle the given error, generating an appropriate response based on its type.
#[tracing::instrument(level = "trace", skip(err, req))]
fn send_error(req: &mut H2Channel, buf: BytesMut, err: anyhow::Error) {
    // Build response object.
    let (status, message) = err
        .downcast_ref::<AppError>()
        .map(|err| err.status_and_message())
        .unwrap_or_else(|| (http::StatusCode::INTERNAL_SERVER_ERROR, "internal server error".into()));
    let resp_res = http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, utils::HEADER_OCTET_STREAM)
        .body(());
    let resp = match resp_res {
        Ok(resp) => resp,
        Err(err) => {
            tracing::error!(error = ?err, "error building response body");
            return;
        }
    };

    // Send response.
    let error = proto::v1::Error { message };
    send_response(req, buf, resp, Some(&error));
}

/// Send a final response over the given H2 channel.
///
/// If a body is given, it will be serialized into the given bytes buf. If not body is given, then
/// a header-only response will be returned with no body.
#[tracing::instrument(level = "trace", skip(req, buf, headers_res, body))]
fn send_response<M: Message>(req: &mut H2Channel, mut buf: BytesMut, headers_res: http::Response<()>, body: Option<&M>) {
    // If we have a body to serialize, then do so.
    if let Some(body) = body {
        if let Err(err) = body.encode(&mut buf) {
            tracing::error!(error = ?err, "error serializing response body");
            return;
        }
    }
    // Send header response.
    let mut body_chan = match req.1.send_response(headers_res, body.is_none()) {
        Ok(body_chan) => body_chan,
        Err(err) => {
            tracing::error!(error = ?err, "error sending header response");
            return;
        }
    };
    // Send body response, if a body was given.
    if body.is_some() {
        if let Err(err) = body_chan.send_data(buf.freeze(), true) {
            tracing::error!(error = ?err, "error sending body response");
        }
    }
}

/// Extract the given request's auth token, else fail.
#[tracing::instrument(level = "trace", skip(req))]
fn must_get_token<T>(req: &http::Request<T>, config: &Config) -> Result<auth::TokenCredentials> {
    // Extract the authorization header.
    let header_val = req
        .headers()
        .iter()
        .find(|(name, _)| name.as_str() == "authorization")
        .ok_or(AppError::Unauthorized)
        .map(|(_, val)| val.clone())?;
    auth::TokenCredentials::from_auth_header(header_val, config)
}

/// Require that the given request has the given method, else error
#[tracing::instrument(level = "trace", skip(req))]
fn require_method<T>(req: &http::Request<T>, method: Method) -> Result<()> {
    if matches!(req.method(), method) {
        Ok(())
    } else {
        Err(AppError::MethodNotAllowed.into())
    }
}
