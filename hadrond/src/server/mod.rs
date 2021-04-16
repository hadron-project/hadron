//! Network server.

mod metadata;

use std::sync::Arc;
use std::{io::Read, net::SocketAddr};

use anyhow::{Context, Result};
use bytes::{BufMut, BytesMut};
use futures::prelude::*;
use futures::stream::StreamExt;
use http::Method;
use prost::Message;
use serde::Serialize;
use tokio::net::{TcpListener, TcpStream};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{SignalStream, UnboundedReceiverStream, WatchStream};
use tokio_stream::StreamMap;

use crate::auth;
use crate::config::Config;
use crate::database::Database;
use crate::error::AppError;
use crate::models::events::Event;
pub use crate::server::metadata::MetadataCache;
use crate::utils;
use crate::NodeId;

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
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: watch::Receiver<bool>,
    /// A channel used for triggering graceful shutdown.
    shutdown: WatchStream<bool>,
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
        config: Arc<Config>, db: Database, shutdown_rx: watch::Receiver<bool>, events_tx: broadcast::Sender<Arc<Event>>,
        events_rx: broadcast::Receiver<Arc<Event>>,
    ) -> Result<(Self, Arc<MetadataCache>)> {
        // Build TCP listener.
        let addr = format!("0.0.0.0:{}", config.client_port);
        let mut listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("error creating network listener on addr {}", addr))?;

        // Spawn the metadata controller.
        let (metadata_tx, metadata_rx) = mpsc::channel(100);
        let (metadata_ctl, cache) =
            metadata::MetadataCtl::new(config.clone(), db.clone(), shutdown_rx.clone(), metadata_rx, events_tx.clone()).await?;
        let metadata_ctl = metadata_ctl.spawn();

        let (channels_tx, channels_rx) = mpsc::unbounded_channel();
        let shutdown = WatchStream::new(shutdown_rx.clone());
        Ok((
            Self {
                config,
                db,
                cache: cache.clone(),
                shutdown_rx,
                shutdown,
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
                Some(needs_shutdown) = self.shutdown.next() => if needs_shutdown { break } else { continue },
            }
        }

        // Begin shutdown routine.
        tracing::debug!("network server shutdown");
        Ok(())
    }

    /// Spawn the given TCP connection, completing its H2 handshake & streaming in new H2 channels.
    #[tracing::instrument(level = "trace", skip(self, socket, addr))]
    fn spawn_connection(&mut self, socket: TcpStream, addr: SocketAddr) {
        let (tx, mut shutdown) = (self.channels_tx.clone(), WatchStream::new(self.shutdown_rx.clone()));
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
                    Some(needs_shutdown) = shutdown.next() => if needs_shutdown { break } else { continue },
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
            Some(proto::v1::URL_V1) => match path.next() {
                Some(proto::v1::URL_METADATA) => {
                    let _ = self.metadata_tx.send(req).await;
                    return;
                }
                _ => Err(AppError::ResourceNotFound.into()),
            },
            _ => Err(AppError::ResourceNotFound.into()),
        };
        if let Err(err) = res {
            send_error(&mut req, self.buf.split(), err);
        }
    }
}

/// Handle the given error, generating an appropriate response based on its type.
#[tracing::instrument(level = "trace", skip(err, req))]
fn send_error(req: &mut H2Channel, mut buf: BytesMut, err: anyhow::Error) {
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
