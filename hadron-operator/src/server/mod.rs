mod webhook;

use std::sync::Arc;

use anyhow::{Context, Result};
use futures::future::FusedFuture;
use futures::prelude::*;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tonic::transport::Server as TonicServer;
use tonic::Request;

use crate::config::Config;
use crate::grpc;
use crate::server::webhook::WebhookServer;
use hadron_core::auth;
use hadron_core::error::AppError;

/// Application server.
pub struct AppServer {
    /// The application's runtime config.
    config: Arc<Config>,

    /// A channel used for triggering graceful shutdown.
    shutdown: broadcast::Sender<()>,
}

impl AppServer {
    /// Create a new instance.
    pub fn new(config: Arc<Config>, shutdown: broadcast::Sender<()>) -> Self {
        Self { config, shutdown }
    }

    /// Spawn this controller.
    ///
    /// This call does not block, but does asynchronously spawn all other network servers used
    /// by the system.
    pub async fn spawn(self) -> Result<JoinHandle<()>> {
        // Spawn the HTTP server for webhooks & healthcheck.
        let http_server = WebhookServer::new(self.config.clone(), self.shutdown.clone())
            .await
            .context("error building webhook server")?
            .spawn();

        // Spawn the gRPC server.
        let shutdown = self.shutdown.clone();
        let grpc_addr = ([0, 0, 0, 0], self.config.client_port);
        let mut grpc_shutdown_rx = self.shutdown.subscribe();
        let service = grpc::OperatorServer::new(self);
        let grpc_server = TonicServer::builder()
            .add_service(service)
            .serve_with_shutdown(grpc_addr.into(), async move {
                let _res = grpc_shutdown_rx.recv().await;
            });

        // Spawn a task which awaits the shutdown of both spawned servers.
        Ok(tokio::spawn(async move {
            let grpc_server_fused = grpc_server.fuse();
            let http_server_fused = http_server.fuse();
            tokio::pin!(grpc_server_fused, http_server_fused);
            loop {
                tokio::select! {
                    Err(err) = &mut grpc_server_fused, if !grpc_server_fused.is_terminated() => {
                        tracing::error!(error = ?err, "error from gRPC server, shutting down");
                        let _res = shutdown.send(());
                    },
                    Err(err) = &mut http_server_fused, if !http_server_fused.is_terminated() => {
                        tracing::error!(error = ?err, "error from http server, shutting down");
                        let _res = shutdown.send(());
                    },
                    else => break,
                }
            }
        }))
    }

    /// Extract the given request's auth token, else fail.
    #[allow(dead_code)]
    fn must_get_token<T>(&self, req: &Request<T>) -> Result<auth::TokenCredentials> {
        // Extract the authorization header.
        let header_val = req
            .metadata()
            .get("authorization")
            .cloned()
            .ok_or(AppError::Unauthorized)?;
        auth::TokenCredentials::from_auth_header(header_val, &self.config.jwt_decoding_key.0)
    }

    /// Extract the given request's basic auth, else fail.
    #[allow(dead_code)]
    fn must_get_user<T>(&self, req: &Request<T>) -> Result<auth::UserCredentials> {
        // Extract the authorization header.
        let header_val = req
            .metadata()
            .get("authorization")
            .cloned()
            .ok_or(AppError::Unauthorized)?;
        auth::UserCredentials::from_auth_header(header_val)
    }
}

#[tonic::async_trait]
impl grpc::Operator for AppServer {}

// use axum::http::StatusCode;
// use axum::{body::Body, http::Response};

// /// A result type used to work seamlessly with axum.
// type ServerResult<T> = std::result::Result<T, ServerError>;

// /// A newtype to make anyhow errors work with axum.
// struct ServerError(pub anyhow::Error);

// impl From<anyhow::Error> for ServerError {
//     fn from(src: anyhow::Error) -> Self {
//         ServerError(src)
//     }
// }

// impl axum::response::IntoResponse for ServerError {
//     type Body = axum::body::Body;
//     type BodyError = <Self::Body as axum::body::HttpBody>::Error;

//     fn into_response(self) -> Response<Self::Body> {
//         tracing::error!(error = ?self.0, "error handling request");
//         let mut res = Response::new(Body::empty());
//         *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
//         res
//     }
// }
