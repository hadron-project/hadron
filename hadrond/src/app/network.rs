use std::sync::Arc;

use anyhow::Result;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use crate::app::TokensMap;
use crate::auth;
use crate::config::Config;
use crate::crd::Token;
use crate::error::{AppError, RpcResult};
use crate::grpc;

/// Application network implementation.
///
/// TODO: add handles for stream pub controller, stream sub controller & pipelines atomic map with channels.
/// TODO: metadata system can be driven entirely from a watch channel updated by a k8s watcher.
pub struct AppNetwork {
    /// The application's runtime config.
    config: Arc<Config>,
    /// A map of all known Token CRs in the namespace.
    tokens: TokensMap,
}

#[tonic::async_trait]
impl grpc::StreamController for AppNetwork {
    /// Server streaming response type for the Metadata method.
    type MetadataStream = ReceiverStream<RpcResult<grpc::MetadataResponse>>;
    /// Server streaming response type for the StreamPublish method.
    type StreamPublishStream = ReceiverStream<RpcResult<grpc::StreamPublishResponse>>;
    /// Server streaming response type for the StreamSubscribe method.
    type StreamSubscribeStream = ReceiverStream<RpcResult<grpc::StreamSubscribeResponse>>;
    /// Server streaming response type for the PipelineSubscribe method.
    type PipelineSubscribeStream = ReceiverStream<RpcResult<grpc::PipelineSubscribeResponse>>;

    /// Open a metadata stream.
    async fn metadata(&self, request: Request<grpc::MetadataRequest>) -> RpcResult<Response<Self::MetadataStream>> {
        let creds = self.must_get_token(&request).map_err(AppError::grpc)?;
        let claims = self.must_get_token_claims(&creds.claims.id).map_err(AppError::grpc)?;

        todo!()
    }

    /// Open a stream publisher channel.
    async fn stream_publish(&self, request: Request<Streaming<grpc::StreamPublishRequest>>) -> RpcResult<Response<Self::StreamPublishStream>> {
        let creds = self.must_get_token(&request).map_err(AppError::grpc)?;
        let claims = self.must_get_token_claims(&creds.claims.id).map_err(AppError::grpc)?;
        claims.check_stream_pub_auth(&self.config.stream).map_err(AppError::grpc)?;

        todo!()
    }

    /// Open a stream subscriber channel.
    async fn stream_subscribe(&self, request: Request<Streaming<grpc::StreamSubscribeRequest>>) -> RpcResult<Response<Self::StreamSubscribeStream>> {
        let creds = self.must_get_token(&request).map_err(AppError::grpc)?;
        let claims = self.must_get_token_claims(&creds.claims.id).map_err(AppError::grpc)?;
        claims.check_stream_sub_auth(&self.config.stream).map_err(AppError::grpc)?;

        todo!()
    }

    /// Open a pipeline subscriber channel.
    async fn pipeline_subscribe(
        &self, request: Request<Streaming<grpc::PipelineSubscribeRequest>>,
    ) -> RpcResult<Response<Self::PipelineSubscribeStream>> {
        let creds = self.must_get_token(&request).map_err(AppError::grpc)?;
        let claims = self.must_get_token_claims(&creds.claims.id).map_err(AppError::grpc)?;
        claims.check_stream_sub_auth(&self.config.stream).map_err(AppError::grpc)?;

        todo!()
    }
}

impl AppNetwork {
    /// Create a new instance.
    pub fn new(config: Arc<Config>, tokens: TokensMap) -> Self {
        Self { config, tokens }
    }

    /// Extract the given request's auth token, else fail.
    fn must_get_token<T>(&self, req: &Request<T>) -> Result<auth::TokenCredentials> {
        // Extract the authorization header.
        let header_val = req
            .metadata()
            .get("authorization")
            .cloned()
            .ok_or(AppError::Unauthorized)?;
        auth::TokenCredentials::from_auth_header(header_val, &self.config)
    }

    /// Get the given token's claims, else return an auth error.
    pub fn must_get_token_claims(&self, token_id: &String) -> Result<Arc<Token>> {
        match self.tokens.as_ref().load().get(token_id).cloned() {
            Some(claims) => Ok(claims),
            None => Err(AppError::UnknownToken.into()),
        }
    }

    /// Extract the given request's basic auth, else fail.
    fn must_get_user<T>(&self, req: &Request<T>) -> Result<auth::UserCredentials> {
        // Extract the authorization header.
        let header_val = req
            .metadata()
            .get("authorization")
            .cloned()
            .ok_or(AppError::Unauthorized)?;
        auth::UserCredentials::from_auth_header(header_val, &self.config)
    }
}
