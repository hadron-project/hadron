//! Network server.

mod cache;
mod events;
mod pipeline;
mod pipeline_replica;
mod stream;
mod stream_replica;

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use futures::stream::StreamExt;
use http::Method;
use prost::Message;
use proto::v1::ENDPOINT_METADATA_SUBSCRIBE;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::auth;
use crate::config::Config;
use crate::crd::{Pipeline, Stream};
use crate::database::Database;
use crate::error::AppError;
use crate::k8s::events::CrdStateChange;
pub use crate::server::cache::MetadataCache;
use crate::server::pipeline::PipelineCtlMsg;
use crate::server::pipeline_replica::PipelineReplicaCtlMsg;
use crate::server::stream::StreamCtlMsg;
use crate::server::stream_replica::StreamReplicaCtlMsg;
use crate::utils;

/// An H2 channel (stream) created from an active HTTP2 connection.
pub type H2Channel = (http::Request<h2::RecvStream>, h2::server::SendResponse<Bytes>);
/// An H2 channel (stream) where both ends are streaming bidirectional data.
pub type H2DataChannel = (h2::RecvStream, h2::SendStream<Bytes>);

/// Network server used to handle client requests.
pub struct Server {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,
    /// The system metadata cache.
    cache: Arc<MetadataCache>,
    /// A map active stream controllers to their communcation channel and offset signal.
    ///
    /// Note that across all partitions of a stream, a pod may only be used once as a partition leader.
    streams: HashMap<String, BTreeMap<u8, StreamHandle>>,
    /// A map of active stream replica controllers to their communication channel.
    ///
    /// Note that per stream partition, a pod may only be assigned once for replication.
    stream_replicas: HashMap<String, BTreeMap<u8, StreamReplicaHandle>>,
    /// A map active pipeline controllers to their communcation channels.
    ///
    /// Note that across all partitions of a stream, a pod may only be used once as a partition leader,
    /// and pipelines follow their source stream's schedule, so this constraint applies for pipelines as well.
    pipelines: HashMap<String, BTreeMap<u8, PipelineHandle>>,
    /// A map of active pipeline replica controllers to their communication channel.
    ///
    /// Note that per stream partition, a pod may only be assigned once for replication,
    /// and pipelines follow their source stream's schedule, so this constraint applies
    /// for pipelines as well.
    pipeline_replicas: HashMap<String, BTreeMap<u8, PipelineReplicaHandle>>,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,

    /// A channel used for system events.
    events_rx: mpsc::Receiver<CrdStateChange>,
    /// A channel used for sending metadata requests to the metadata controller.
    metadata_requests_tx: mpsc::Sender<H2Channel>,

    /// The router used to match inbound requests.
    router: matchit::Node<RouterBackend>,
    /// The TCP listener used by this server.
    listener: TcpListener,
    /// The channel used to handle H2 channels as they arrive.
    channels_tx: mpsc::Sender<H2Channel>,
    /// The channel used to handle H2 channels as they arrive.
    channels_rx: ReceiverStream<H2Channel>,

    /// A general purpose reusable bytes buffer, safe for concurrent use.
    buf: BytesMut,
}

impl Server {
    /// Create a new instance.
    pub async fn new(
        config: Arc<Config>, db: Database, shutdown_tx: broadcast::Sender<()>, events_rx: mpsc::Receiver<CrdStateChange>,
        metadata_requests_tx: mpsc::Sender<H2Channel>,
    ) -> Result<(Self, Arc<MetadataCache>)> {
        // Build TCP listener.
        let addr = format!("0.0.0.0:{}", config.client_port);
        let listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("error creating network listener on addr {}", addr))?;
        tracing::info!("server is listening on {}", addr);

        let cache = Arc::new(MetadataCache::default());
        let (channels_tx, channels_rx) = mpsc::channel(10_000);
        Ok((
            Self {
                config,
                db,
                cache: cache.clone(),
                streams: Default::default(),
                stream_replicas: Default::default(),
                pipelines: Default::default(),
                pipeline_replicas: Default::default(),
                shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
                shutdown_tx,
                events_rx,
                metadata_requests_tx,
                router: build_routes()?,
                listener,
                channels_tx,
                channels_rx: ReceiverStream::new(channels_rx),
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
                Some(event) = self.events_rx.recv() => self.handle_event(event).await,
                _ = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        for (_, stream) in self.streams.drain() {
            for (_, handle) in stream {
                if let Err(err) = handle.handle.await {
                    tracing::error!(error = ?err, "error awaiting shutdown of spawned stream controller");
                }
            }
        }
        for (_, pipeline) in self.pipelines.drain() {
            for (_, handle) in pipeline {
                if let Err(err) = handle.handle.await {
                    tracing::error!(error = ?err, "error awaiting shutdown of spawned pipeline controller");
                }
            }
        }
        tracing::debug!("network server shutdown");
        Ok(())
    }

    /// Spawn the given TCP connection, completing its H2 handshake & streaming in new H2 channels.
    #[tracing::instrument(level = "trace", skip(self, socket, _addr))]
    fn spawn_connection(&mut self, socket: TcpStream, _addr: SocketAddr) {
        let (tx, mut shutdown) = (self.channels_tx.clone(), BroadcastStream::new(self.shutdown_tx.subscribe()));
        tokio::spawn(async move {
            let handshake_res = h2::server::handshake(socket)
                .await
                .context("error performing H2 handshake");
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
                            let _ = tx.send(h2chan).await;
                        },
                        Some(Err(err)) => {
                            tracing::error!(error = ?err, "error handling new H2 stream");
                            break;
                        }
                        None => break,
                    },
                    _ = shutdown.next() => break,
                }
            }
        });
    }

    /// Handle a new H2 channel created from an H2 connection.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn route_request(&mut self, mut req: H2Channel) {
        // Check for a router match on the given request.
        let matches = match self.router.at(req.0.uri().path()) {
            Ok(matches) => matches,
            Err(_) => {
                send_error(&mut req, self.buf.split(), AppError::ResourceNotFound.into(), std::convert::identity);
                return;
            }
        };

        // Handle backend target & unpack params.
        let params = matches.params;
        let err: anyhow::Error = match matches.value {
            RouterBackend::StreamPublish => {
                let name = params.get("name").unwrap_or_default();
                let partition = params.get("partition").unwrap_or_default();
                match self.get_v1_stream_handle(name, partition) {
                    Ok(handle) => {
                        let _res = handle.tx.send(StreamCtlMsg::RequestPublish(req)).await;
                        return;
                    }
                    Err(err) => err,
                }
            }
            RouterBackend::StreamSubscribe => {
                let name = params.get("name").unwrap_or_default();
                let partition = params.get("partition").unwrap_or_default();
                match self.get_v1_stream_handle(name, partition) {
                    Ok(handle) => {
                        let _res = handle.tx.send(StreamCtlMsg::RequestSubscribe(req)).await;
                        return;
                    }
                    Err(err) => err,
                }
            }
            RouterBackend::PipelineSubscribe => {
                let name = params.get("name").unwrap_or_default();
                let partition = params.get("partition").unwrap_or_default();
                match self.get_v1_pipeline_handle(name, partition) {
                    Ok(handle) => {
                        let _res = handle.tx.send(PipelineCtlMsg::Request(req)).await;
                        return;
                    }
                    Err(err) => err,
                }
            }
            RouterBackend::MetadataStream => {
                let _res = self.metadata_requests_tx.send(req).await;
                return;
            }
        };
        send_error(&mut req, self.buf.split(), err, std::convert::identity);
    }

    /// Get a reference to the target stream handle for the given request info.
    #[tracing::instrument(level = "trace", skip(self))]
    fn get_v1_stream_handle(&self, name: &str, partition: &str) -> Result<&StreamHandle> {
        let partition: u8 = partition
            .parse()
            .map_err(|_| AppError::InvalidInput("invalid stream partition provided".into()))?;
        self.streams
            .get(name)
            .and_then(|partitions| partitions.get(&partition))
            .ok_or_else(|| AppError::ResourceNotFound.into())
    }

    /// Get a reference to the target pipeline handle for the given request info.
    #[tracing::instrument(level = "trace", skip(self))]
    fn get_v1_pipeline_handle(&self, name: &str, partition: &str) -> Result<&PipelineHandle> {
        let partition: u8 = partition
            .parse()
            .map_err(|_| AppError::InvalidInput("invalid pipeline partition provided".into()))?;
        self.pipelines
            .get(name)
            .and_then(|partitions| partitions.get(&partition))
            .ok_or_else(|| AppError::ResourceNotFound.into())
    }
}

/// All router backends used by the server.
#[derive(Clone, Debug)]
enum RouterBackend {
    StreamPublish,
    StreamSubscribe,
    PipelineSubscribe,
    MetadataStream,
}

/// Build a router for matching inbound requests.
fn build_routes() -> Result<matchit::Node<RouterBackend>> {
    let mut router = matchit::Node::new();
    router
        .insert("/v1/stream/:name/:partition/publish", RouterBackend::StreamPublish)
        .and_then(|_| router.insert("/v1/stream/:name/:partition/subscribe", RouterBackend::StreamSubscribe))
        .and_then(|_| router.insert("/v1/pipeline/:name/:partition/subscribe", RouterBackend::PipelineSubscribe))
        .and_then(|_| router.insert(ENDPOINT_METADATA_SUBSCRIBE, RouterBackend::MetadataStream))
        .context("error building router")?;
    Ok(router)
}

/// Handle the given error, generating an appropriate response based on its type.
#[tracing::instrument(level = "trace", skip(req, buf, err, wrapper))]
pub(crate) fn send_error<F, M>(req: &mut H2Channel, buf: BytesMut, err: anyhow::Error, wrapper: F)
where
    F: Fn(proto::v1::Error) -> M,
    M: Message,
{
    // Build response object.
    let (status, message) = err
        .downcast_ref::<AppError>()
        .map(|err| err.status_and_message())
        .unwrap_or_else(|| (http::StatusCode::INTERNAL_SERVER_ERROR, "internal server error".into()));
    let resp_res = http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, utils::HEADER_APP_PROTO)
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
    send_response(req, buf, resp, Some(wrapper(error)));
}

/// Send a final response over the given H2 channel.
///
/// If a body is given, it will be serialized into the given bytes buf. If no body is given, then
/// a header-only response will be returned with no body.
#[tracing::instrument(level = "trace", skip(req, buf, headers_res, body))]
pub(crate) fn send_response<M: Message>(req: &mut H2Channel, mut buf: BytesMut, headers_res: http::Response<()>, body: Option<M>) {
    // If we have a body to serialize, then do so.
    if let Some(body) = &body {
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
pub(crate) fn must_get_token<T>(req: &http::Request<T>, config: &Config) -> Result<auth::TokenCredentials> {
    // Extract the authorization header.
    let header_val = req
        .headers()
        .iter()
        .find(|(name, _)| name.as_str() == "authorization")
        .ok_or(AppError::Unauthorized)
        .map(|(_, val)| val.clone())?;
    auth::TokenCredentials::from_auth_header(header_val, config)
}

/// Extract the given request's basic auth, else fail.
#[tracing::instrument(level = "trace", skip(req))]
pub(crate) fn must_get_user<'a, T>(req: &'a http::Request<T>) -> Result<auth::UserCredentials> {
    // Extract the authorization header.
    let header_val = req
        .headers()
        .iter()
        .find(|(name, _)| name.as_str() == "authorization")
        .ok_or(AppError::Unauthorized)
        .map(|(_, val)| val)?;
    auth::UserCredentials::from_auth_header(header_val)
}

/// Require that the given request has the given method, else error
#[tracing::instrument(level = "trace", skip(req))]
pub(crate) fn require_method<T>(req: &http::Request<T>, method: Method) -> Result<()> {
    #[allow(unused_variables)] // A linting bug in `matches!` it would seem.
    if matches!(req.method(), method) {
        Ok(())
    } else {
        Err(AppError::MethodNotAllowed.into())
    }
}

/// A handle to a live stream controller.
struct StreamHandle {
    /// A pointer to the stream object.
    ///
    /// This is always kept up-to-date as data flows in from K8s.
    pub stream: Arc<Stream>,
    /// The stream partition of this controller.
    pub partition: u8,
    /// The controller's communication channel.
    pub tx: mpsc::Sender<StreamCtlMsg>,
    /// A signal of the stream's last offset.
    pub last_offset: watch::Receiver<u64>,
    /// The spawned controller's join handle.
    pub handle: JoinHandle<Result<()>>,
}

/// A handle to a live stream replica controller.
struct StreamReplicaHandle {
    /// A pointer to the stream object.
    ///
    /// This is always kept up-to-date as data flows in from K8s.
    pub stream: Arc<Stream>,
    /// The stream partition of this controller.
    pub partition: u8,
    /// The controller's communication channel.
    pub tx: mpsc::Sender<StreamReplicaCtlMsg>,
    /// The spawned controller's join handle.
    pub handle: JoinHandle<Result<()>>,
}

/// A handle to a live pipeline controller.
struct PipelineHandle {
    /// A pointer to the pipeline object.
    ///
    /// This is always kept up-to-date as data flows in from K8s.
    pub pipeline: Arc<Pipeline>,
    /// The pipeline partition of this controller.
    pub partition: u8,
    /// The controller's communication channel.
    pub tx: mpsc::Sender<PipelineCtlMsg>,
    /// The spawned controller's join handle.
    pub handle: JoinHandle<Result<()>>,
}

/// A handle to a live pipeline replica controller.
struct PipelineReplicaHandle {
    /// A pointer to the pipeline object.
    ///
    /// This is always kept up-to-date as data flows in from K8s.
    pub pipeline: Arc<Pipeline>,
    /// The pipeline partition of this controller.
    pub partition: u8,
    /// The controller's communication channel.
    pub tx: mpsc::Sender<PipelineReplicaCtlMsg>,
    /// The spawned controller's join handle.
    pub handle: JoinHandle<Result<()>>,
}
