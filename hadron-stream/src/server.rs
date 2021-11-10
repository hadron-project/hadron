use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use futures::prelude::*;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{ReceiverStream, WatchStream};
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};

use crate::config::Config;
use crate::error::{AppError, AppErrorExt, RpcResult};
use crate::grpc;
use crate::pipeline::PipelineCtlMsg;
use crate::stream::StreamCtlMsg;
use crate::watchers::{PipelinesMap, SecretsMap, StreamMetadataRx, TokensMap};
use hadron_core::auth;
use hadron_core::crd::Token;

const CLIENT_ADDR: &str = "0.0.0.0:7000";

/// Application server.
pub struct AppServer {
    /// The application's runtime config.
    config: Arc<Config>,
    /// A map of active pipeline controllers to their communcation channels.
    pipelines: PipelinesMap,
    /// A map of all known Token CRs in the namespace.
    tokens: TokensMap,
    /// A map of all known Secrets in the namespace belonging to Hadron.
    secrets: SecretsMap,
    /// A channel of stream metadata for connection info.
    metadata_rx: StreamMetadataRx,

    /// A channel used for triggering graceful shutdown.
    shutdown: broadcast::Sender<()>,

    /// A channel for communicating with the stream controller.
    stream_tx: mpsc::Sender<StreamCtlMsg>,
}

impl AppServer {
    /// Create a new instance.
    pub fn new(
        config: Arc<Config>, pipelines: PipelinesMap, tokens: TokensMap, secrets: SecretsMap, metadata_rx: StreamMetadataRx, shutdown: broadcast::Sender<()>, stream_tx: mpsc::Sender<StreamCtlMsg>,
    ) -> Self {
        Self {
            config,
            pipelines,
            tokens,
            secrets,
            metadata_rx,
            shutdown,
            stream_tx,
        }
    }

    /// Spawn this controller which also creates the client gRPC server.
    pub fn spawn(self) -> Result<JoinHandle<()>> {
        let addr = CLIENT_ADDR.parse().context("failed to parse listener address")?;
        let (shutdown, mut shutdown_rx) = (self.shutdown.clone(), self.shutdown.subscribe());
        let service = grpc::StreamControllerServer::new(self);
        let fut = Server::builder().add_service(service).serve_with_shutdown(addr, async move {
            let _res = shutdown_rx.recv().await;
        });
        Ok(tokio::spawn(async move {
            if let Err(err) = fut.await {
                tracing::error!(error = ?err, "error from client gRPC server");
            }
            let _res = shutdown.send(());
        }))
    }

    /// Extract the given request's auth token, else fail.
    fn must_get_token<T>(&self, req: &Request<T>) -> Result<auth::UnverifiedTokenCredentials> {
        // Extract the authorization header.
        let header_val = req.metadata().get("authorization").cloned().ok_or(AppError::Unauthorized)?;
        auth::UnverifiedTokenCredentials::from_auth_header(header_val)
    }

    /// Get the given token's claims, cryptographically verifying the claims, else return an auth error.
    pub fn must_get_token_claims(&self, token: auth::UnverifiedTokenCredentials) -> Result<(Arc<Token>, auth::TokenCredentials)> {
        let secret = match self.secrets.as_ref().load().get(&token.claims.sub).cloned() {
            Some(secret) => secret,
            None => return Err(AppError::UnknownToken.into()),
        };
        let token = token.verify(secret.key())?;
        match self.tokens.as_ref().load().get(&token.claims.sub).cloned() {
            Some(claims) => Ok((claims, token)),
            None => Err(AppError::UnknownToken.into()),
        }
    }

    /// Extract the given request's basic auth, else fail.
    #[allow(dead_code)]
    fn must_get_user<T>(&self, req: &Request<T>) -> Result<auth::UserCredentials> {
        // Extract the authorization header.
        let header_val = req.metadata().get("authorization").cloned().ok_or(AppError::Unauthorized)?;
        auth::UserCredentials::from_auth_header(header_val)
    }
}

#[tonic::async_trait]
impl grpc::StreamController for AppServer {
    /// Server streaming response type for the Metadata method.
    type MetadataStream = ReceiverStream<RpcResult<grpc::MetadataResponse>>;
    /// Server streaming response type for the StreamSubscribe method.
    type StreamSubscribeStream = ReceiverStream<RpcResult<grpc::StreamSubscribeResponse>>;
    /// Server streaming response type for the PipelineSubscribe method.
    type PipelineSubscribeStream = ReceiverStream<RpcResult<grpc::PipelineSubscribeResponse>>;

    /// Open a metadata stream.
    async fn metadata(&self, request: Request<grpc::MetadataRequest>) -> RpcResult<Response<Self::MetadataStream>> {
        let creds = self.must_get_token(&request).map_err(AppError::grpc)?;
        let _claims = self.must_get_token_claims(creds).map_err(AppError::grpc)?;

        let (res_tx, res_rx) = mpsc::channel(1);
        let mut metadata_rx = WatchStream::new(self.metadata_rx.clone());
        tokio::spawn(async move {
            loop {
                let timeout = tokio::time::sleep(std::time::Duration::from_secs(15));
                tokio::select! {
                    data_opt = metadata_rx.next() => match data_opt {
                        Some(data) => {
                            let _res = res_tx.send(Ok(grpc::MetadataResponse { partitions: data })).await;
                        }
                        None => break,
                    },
                    _ = timeout => {
                        if res_tx.is_closed() {
                            break;
                        }
                    },
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(res_rx)))
    }

    /// Open a stream publisher channel.
    async fn stream_publish(&self, request: Request<grpc::StreamPublishRequest>) -> RpcResult<Response<grpc::StreamPublishResponse>> {
        let creds = self.must_get_token(&request).map_err(AppError::grpc)?;
        let (claims, _creds) = self.must_get_token_claims(creds).map_err(AppError::grpc)?;
        claims.check_stream_pub_auth(&self.config.stream).map_err(AppError::grpc)?;

        let (tx, rx) = oneshot::channel();
        self.stream_tx
            .send(StreamCtlMsg::RequestPublish { tx, request: request.into_inner() })
            .await
            .map_err(|_err| AppError::grpc(anyhow!("error communicating with stream controller")))?;
        let res = rx.await.map_err(|_err| AppError::grpc(anyhow!("error awaiting response from stream controller")))??;
        Ok(Response::new(res))
    }

    /// Open a stream subscriber channel.
    async fn stream_subscribe(&self, request: Request<Streaming<grpc::StreamSubscribeRequest>>) -> RpcResult<Response<Self::StreamSubscribeStream>> {
        let creds = self.must_get_token(&request).map_err(AppError::grpc)?;
        let (claims, _creds) = self.must_get_token_claims(creds).map_err(AppError::grpc)?;
        claims.check_stream_sub_auth(&self.config.stream).map_err(AppError::grpc)?;

        // Await initial setup payload and forward it along to the controller.
        let mut request_stream = request.into_inner();
        let req_action = request_stream
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("no subscription setup request received"))?
            .action
            .ok_or_else(|| Status::invalid_argument("no action variant received in request"))?;
        let setup: grpc::StreamSubscribeSetup = match req_action {
            grpc::StreamSubscribeRequestAction::Setup(setup) => setup,
            _ => return Err(Status::invalid_argument("invalid action variant received in request, expected `setup` variant")),
        };

        let (res_tx, res_rx) = mpsc::channel(10);
        self.stream_tx
            .send(StreamCtlMsg::RequestSubscribe {
                tx: res_tx,
                rx: request_stream,
                setup,
            })
            .await
            .map_err(|_err| AppError::grpc(anyhow!("error communicating with stream controller")))?;
        Ok(Response::new(ReceiverStream::new(res_rx)))
    }

    /// Open a pipeline subscriber channel.
    async fn pipeline_subscribe(&self, request: Request<Streaming<grpc::PipelineSubscribeRequest>>) -> RpcResult<Response<Self::PipelineSubscribeStream>> {
        let creds = self.must_get_token(&request).map_err(AppError::grpc)?;
        let (claims, _creds) = self.must_get_token_claims(creds).map_err(AppError::grpc)?;
        claims.check_stream_sub_auth(&self.config.stream).map_err(AppError::grpc)?;

        // Await initial setup payload and use it to find the target controller.
        let mut request_stream = request.into_inner();
        let setup_request = request_stream.message().await?.ok_or_else(|| Status::invalid_argument("no subscription setup request received"))?;
        let pipeline = &setup_request.pipeline;
        let req_action = setup_request.action.ok_or_else(|| Status::invalid_argument("no action variant received in request"))?;
        let stage_name = match req_action {
            grpc::PipelineSubscribeRequestAction::StageName(stage) => stage,
            _ => return Err(Status::invalid_argument("invalid action variant received in request, expected `StageName` variant")),
        };

        // Find the target controller & forward the request.
        let pipeline_handle = self
            .pipelines
            .load()
            .get(pipeline)
            .cloned()
            .ok_or_else(|| Status::not_found("the target pipeline controller was not found"))?;
        let (res_tx, res_rx) = mpsc::channel(10);
        pipeline_handle
            .tx
            .send(PipelineCtlMsg::Request {
                tx: res_tx,
                rx: request_stream,
                stage_name,
            })
            .await
            .map_err(|_err| AppError::grpc(anyhow!("error communicating with pipeline controller")))?;

        Ok(Response::new(ReceiverStream::new(res_rx)))
    }
}
