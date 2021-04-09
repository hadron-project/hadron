//! Network server.

use std::sync::Arc;
use std::{io::Read, net::SocketAddr};

use anyhow::{Context, Result};
use bytes::{BufMut, BytesMut};
use futures::prelude::*;
use futures::stream::StreamExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{SignalStream, UnboundedReceiverStream, WatchStream};
use tokio_stream::StreamMap;

use crate::auth;
use crate::config::Config;
use crate::database::Database;
use crate::error::AppError;
use crate::NodeId;

/// An H2 channel (stream) created from an active HTTP2 connection.
type H2Channel = (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>);

/// Network server used to handle client requests.
pub struct Server {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: watch::Receiver<bool>,
    /// A channel used for triggering graceful shutdown.
    shutdown: WatchStream<bool>,

    /// The TCP listener used by this server.
    listener: TcpListener,
    /// The channel used to handle H2 channels as they arrive.
    channels_tx: mpsc::UnboundedSender<H2Channel>,
    /// The channel used to handle H2 channels as they arrive.
    channels_rx: UnboundedReceiverStream<H2Channel>,

    /// A general purpose reusable bytes buffer, safe for concurrent use.
    buf: BytesMut,
}

impl Server {
    /// Create a new instance.
    pub async fn new(config: Arc<Config>, db: Database, shutdown_rx: watch::Receiver<bool>) -> Result<Self> {
        // Build TCP listener.
        let addr = format!("0.0.0.0:{}", config.client_port);
        let mut listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("error creating network listener on addr {}", addr))?;

        let (channels_tx, channels_rx) = mpsc::unbounded_channel();
        let shutdown = WatchStream::new(shutdown_rx.clone());
        Ok(Self {
            config,
            db,
            shutdown_rx,
            shutdown,
            listener,
            channels_tx,
            channels_rx: UnboundedReceiverStream::new(channels_rx),
            buf: BytesMut::with_capacity(5000),
        })
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        loop {
            tokio::select! {
                Ok((socket, addr)) = self.listener.accept() => self.spawn_connection(socket, addr),
                Some(h2chan) = self.channels_rx.next() => self.handle_request(h2chan).await,
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
    async fn handle_request(&mut self, mut req: H2Channel) {
        // Perform top-level routing based on HTTP method.
        let res = match req.0.method() {
            &http::Method::GET => match req.0.uri().path() {
                proto::v1::URL_METADATA => self.handle_metadata_request(&mut req).await,
                _ => Err(AppError::ResourceNotFound.into()),
            },
            &http::Method::POST => match req.0.uri().path() {
                proto::v1::URL_SCHEMA => self.handle_schema_update(&mut req).await,
                // TODO: handle these. Route over to needed controller.
                _ => Err(AppError::ResourceNotFound.into()),
            },
            // Reject request for use of unsupported method.
            method => Err(AppError::MethodNotSupported.into()),
        };
        let err = match res {
            Ok(_) => return,
            Err(err) => err,
        };
        handle_error(err, &mut req, self.buf.split());
    }

    /// Handle a request for cluster metadata.
    #[tracing::instrument(level = "trace", skip(self, h2chan))]
    async fn handle_metadata_request(&mut self, h2chan: &mut H2Channel) -> Result<()> {
        let (ref mut req, ref mut res_chan) = h2chan;

        // Stub: respond with some random yaml.
        let body = bytes::Bytes::from(r#"{"nothing":"so far"}"#);
        let mut response = http::Response::new(());
        response
            .headers_mut()
            .append("content-type", "application/json".parse().context("could not parse header for response")?);
        let mut body_res = res_chan.send_response(response, false).context("error sending header response")?;
        body_res.send_data(body, true).context("error sending body response")?;
        Ok(())
    }

    /// Handle a request to update the cluster's schema.
    #[tracing::instrument(level = "trace", skip(self, h2chan))]
    async fn handle_schema_update(&mut self, h2chan: &mut H2Channel) -> Result<()> {
        let (ref mut req, ref mut res_chan) = h2chan;

        let creds = must_get_token(&req, &*self.config)?;
        anyhow::bail!(AppError::InvalidInput("bad job!!!".into()));
        // let _claims = self.index.must_get_token_claims(&creds.id).map_err(utils::status_from_err)?;
        // let req = req.into_inner();
        // let validated = schema::SchemaUpdate::decode_and_validate(&req).map_err(utils::status_from_err)?;
        // let (tx, rx) = oneshot::channel();
        // let _ = self.network.send(ClientRequest::UpdateSchema(UpdateSchema { req, validated, tx, creds }));
        // rx.await.map_err(status_from_rcv_error).and_then(|res| res).map(Response::new)

        // Stub: respond with some random yaml.
        let body = bytes::Bytes::from(r#"{"schema":"update"}"#);
        let response = http::Response::builder()
            .status(http::StatusCode::OK)
            .header("content-type", "application/json")
            .body(())
            .unwrap();
        let mut body_res = res_chan.send_response(response, false).unwrap();
        body_res.send_data(body, true).unwrap();
        Ok(())
    }
}

/// Handle the given error, generating an appropriate response based on its type.
#[tracing::instrument(level = "trace", skip(err, req))]
fn handle_error(err: anyhow::Error, req: &mut H2Channel, mut buf: BytesMut) {
    // Build response object.
    let mut resp = http::Response::new(());
    let (status, message) = err
        .downcast_ref::<AppError>()
        .map(|err| err.status_and_message())
        .unwrap_or_else(|| (http::StatusCode::INTERNAL_SERVER_ERROR, "internal server error".into()));
    *resp.status_mut() = status;

    // Serialize the error message body.
    let error = proto::v1::Error { message };
    if let Err(err) = proto::v1::write_to_bytes(&error, &mut buf) {
        tracing::error!(error = ?err, "error writing message to response body");
    }

    // Send error response.
    let mut body_chan = match req.1.send_response(resp, false) {
        Ok(body_chan) => body_chan,
        Err(err) => {
            tracing::error!(error = ?err, "error responding to request from error handler");
            return;
        }
    };
    if let Err(err) = body_chan.send_data(buf.freeze(), true) {
        tracing::error!(error = ?err, "error sending body response from error handler");
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
