//! The Client gRPC service implementation module.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{mpsc, oneshot};
use tonic::metadata::{MetadataMap, MetadataValue};
use tonic::{async_trait, transport::Channel, Request, Response, Status, Streaming};

use crate::auth::TokenCredentials;
use crate::config::Config;
pub use crate::proto::client::client_client::ClientClient;
use crate::proto::client::client_server::Client;
pub use crate::proto::client::client_server::ClientServer;
use crate::proto::client::{EphemeralPubRequest, EphemeralPubResponse, EphemeralSubClient, EphemeralSubServer};
use crate::proto::client::{PipelineStageSubClient, PipelineStageSubServer, StreamUnsubRequest, StreamUnsubResponse};
use crate::proto::client::{RpcPubRequest, RpcPubResponse, RpcSubClient, RpcSubServer};
use crate::proto::client::{StreamPubRequest, StreamPubResponse, StreamSubClient, StreamSubServer};
use crate::proto::client::{TransactionClient, TransactionServer, UpdateSchemaRequest, UpdateSchemaResponse};
use crate::utils::{self, map_result_to_status, status_from_rcv_error, TonicResult};
use crate::NodeId;

pub(super) struct ClientService {
    /// This node's ID.
    _id: NodeId,
    config: Arc<Config>,
    /// The channel used for propagating requests into the network actor, which flows into the rest of the system.
    network: mpsc::UnboundedSender<ClientRequest>, // TODO: In the future, we will want to document this & make capacity configurable.
}

impl ClientService {
    pub fn new(_id: NodeId, config: Arc<Config>, network: mpsc::UnboundedSender<ClientRequest>) -> Self {
        Self { _id, config, network }
    }

    /// Deserialize token credentials provided in the given metadata map, or reject as unauthorized.
    fn must_get_token(&self, meta: &MetadataMap) -> TonicResult<TokenCredentials> {
        let token = meta
            .get(utils::HEADER_X_HADRON_AUTH)
            .ok_or_else(|| Status::unauthenticated("no credentials provided"))?
            .to_owned();
        let creds = TokenCredentials::from_auth_header(token, &*self.config).or_else(|err| map_result_to_status(Err(err)))?;
        Ok(creds)
    }
}

#[async_trait]
impl Client for ClientService {
    type TransactionStream = mpsc::UnboundedReceiver<TonicResult<TransactionServer>>;
    async fn transaction(&self, req: Request<Streaming<TransactionClient>>) -> TonicResult<Response<Self::TransactionStream>> {
        let creds = self.must_get_token(req.metadata())?;
        let (tx, rx) = mpsc::unbounded_channel();
        let req = req.into_inner();
        let _ = self.network.send(ClientRequest::Transaction(Transaction { req, tx, creds }));
        Ok(Response::new(rx))
    }

    async fn ephemeral_pub(&self, req: Request<EphemeralPubRequest>) -> TonicResult<Response<EphemeralPubResponse>> {
        let creds = self.must_get_token(req.metadata())?;
        let (tx, rx) = oneshot::channel();
        let req = req.into_inner();
        let _ = self.network.send(ClientRequest::EphemeralPub(EphemeralPub { req, tx, creds }));
        rx.await.map_err(status_from_rcv_error).and_then(|res| res).map(Response::new)
    }

    type EphemeralSubStream = mpsc::UnboundedReceiver<TonicResult<EphemeralSubServer>>;
    async fn ephemeral_sub(&self, req: Request<EphemeralSubClient>) -> TonicResult<Response<Self::EphemeralSubStream>> {
        let creds = self.must_get_token(req.metadata())?;
        let (tx, rx) = mpsc::unbounded_channel();
        let req = req.into_inner();
        let _ = self.network.send(ClientRequest::EphemeralSub(EphemeralSub { req, tx, creds }));
        Ok(Response::new(rx))
    }

    async fn rpc_pub(&self, req: Request<RpcPubRequest>) -> TonicResult<Response<RpcPubResponse>> {
        let creds = self.must_get_token(req.metadata())?;
        let (tx, rx) = oneshot::channel();
        let req = req.into_inner();
        let _ = self.network.send(ClientRequest::RpcPub(RpcPub { req, tx, creds }));
        rx.await.map_err(status_from_rcv_error).and_then(|res| res).map(Response::new)
    }

    type RpcSubStream = mpsc::UnboundedReceiver<TonicResult<RpcSubServer>>;
    async fn rpc_sub(&self, req: Request<Streaming<RpcSubClient>>) -> TonicResult<Response<Self::RpcSubStream>> {
        let creds = self.must_get_token(req.metadata())?;
        let (tx, rx) = mpsc::unbounded_channel();
        let req = req.into_inner();
        let _ = self.network.send(ClientRequest::RpcSub(RpcSub { req, tx, creds }));
        Ok(Response::new(rx))
    }

    async fn stream_pub(&self, req: Request<StreamPubRequest>) -> TonicResult<Response<StreamPubResponse>> {
        let creds = self.must_get_token(req.metadata())?;
        let (tx, rx) = oneshot::channel();
        let req = req.into_inner();
        let _ = self.network.send(ClientRequest::StreamPub(StreamPub { req, tx, creds }));
        rx.await.map_err(status_from_rcv_error).and_then(|res| res).map(Response::new)
    }

    type StreamSubStream = mpsc::UnboundedReceiver<TonicResult<StreamSubServer>>;
    async fn stream_sub(&self, req: Request<Streaming<StreamSubClient>>) -> TonicResult<Response<Self::StreamSubStream>> {
        let creds = self.must_get_token(req.metadata())?;
        let (tx, rx) = mpsc::unbounded_channel();
        let req = req.into_inner();
        let _ = self.network.send(ClientRequest::StreamSub(StreamSub { req, tx, creds }));
        Ok(Response::new(rx))
    }

    async fn stream_unsub(&self, req: Request<StreamUnsubRequest>) -> TonicResult<Response<StreamUnsubResponse>> {
        let creds = self.must_get_token(req.metadata())?;
        let (tx, rx) = oneshot::channel();
        let req = req.into_inner();
        let _ = self.network.send(ClientRequest::StreamUnsub(StreamUnsub { req, tx, creds }));
        rx.await.map_err(status_from_rcv_error).and_then(|res| res).map(Response::new)
    }

    type PipelineStageSubStream = mpsc::UnboundedReceiver<TonicResult<PipelineStageSubServer>>;
    async fn pipeline_stage_sub(&self, req: Request<Streaming<PipelineStageSubClient>>) -> TonicResult<Response<Self::PipelineStageSubStream>> {
        let creds = self.must_get_token(req.metadata())?;
        let (tx, rx) = mpsc::unbounded_channel();
        let req = req.into_inner();
        let _ = self.network.send(ClientRequest::PipelineStageSub(PipelineStageSub { req, tx, creds }));
        Ok(Response::new(rx))
    }

    async fn update_schema(&self, req: Request<UpdateSchemaRequest>) -> TonicResult<Response<UpdateSchemaResponse>> {
        let creds = self.must_get_token(req.metadata())?;
        let (tx, rx) = oneshot::channel();
        let req = req.into_inner();
        let _ = self.network.send(ClientRequest::UpdateSchema(UpdateSchema { req, tx, creds }));
        rx.await.map_err(status_from_rcv_error).and_then(|res| res).map(Response::new)
    }
}

/// A request flowing into this node from a client.
pub enum ClientRequest {
    Transaction(Transaction),
    EphemeralPub(EphemeralPub),
    EphemeralSub(EphemeralSub),
    RpcPub(RpcPub),
    RpcSub(RpcSub),
    StreamPub(StreamPub),
    StreamSub(StreamSub),
    StreamUnsub(StreamUnsub),
    PipelineStageSub(PipelineStageSub),
    UpdateSchema(UpdateSchema),
}

pub struct Transaction {
    pub req: Streaming<TransactionClient>,
    pub tx: mpsc::UnboundedSender<TonicResult<TransactionServer>>,
    pub creds: TokenCredentials,
}

pub struct EphemeralPub {
    pub req: EphemeralPubRequest,
    pub tx: oneshot::Sender<TonicResult<EphemeralPubResponse>>,
    pub creds: TokenCredentials,
}

pub struct EphemeralSub {
    pub req: EphemeralSubClient,
    pub tx: mpsc::UnboundedSender<TonicResult<EphemeralSubServer>>,
    pub creds: TokenCredentials,
}

pub struct RpcPub {
    pub req: RpcPubRequest,
    pub tx: oneshot::Sender<TonicResult<RpcPubResponse>>,
    pub creds: TokenCredentials,
}

pub struct RpcSub {
    pub req: Streaming<RpcSubClient>,
    pub tx: mpsc::UnboundedSender<TonicResult<RpcSubServer>>,
    pub creds: TokenCredentials,
}

pub struct StreamPub {
    pub req: StreamPubRequest,
    pub tx: oneshot::Sender<TonicResult<StreamPubResponse>>,
    pub creds: TokenCredentials,
}

pub struct StreamSub {
    pub req: Streaming<StreamSubClient>,
    pub tx: mpsc::UnboundedSender<TonicResult<StreamSubServer>>,
    pub creds: TokenCredentials,
}

pub struct StreamUnsub {
    pub req: StreamUnsubRequest,
    pub tx: oneshot::Sender<TonicResult<StreamUnsubResponse>>,
    pub creds: TokenCredentials,
}

pub struct PipelineStageSub {
    pub req: Streaming<PipelineStageSubClient>,
    pub tx: mpsc::UnboundedSender<TonicResult<PipelineStageSubServer>>,
    pub creds: TokenCredentials,
}

pub struct UpdateSchema {
    pub req: UpdateSchemaRequest,
    pub tx: oneshot::Sender<TonicResult<UpdateSchemaResponse>>,
    pub creds: TokenCredentials,
}

/// Forward the given client request to the target node.
#[tracing::instrument(level = "trace", skip(req, chan))]
pub async fn forward_client_request(req: ClientRequest, chan: Channel) {
    let client = ClientClient::new(chan);
    match req {
        ClientRequest::Transaction(req) => forward_transaction(req, client).await,
        ClientRequest::EphemeralPub(req) => forward_ephemeral_pub(req, client).await,
        ClientRequest::EphemeralSub(req) => forward_ephemeral_sub(req, client).await,
        ClientRequest::RpcPub(req) => forward_rpc_pub(req, client).await,
        ClientRequest::RpcSub(req) => forward_rpc_sub(req, client).await,
        ClientRequest::StreamPub(req) => forward_stream_pub(req, client).await,
        ClientRequest::StreamSub(req) => forward_stream_sub(req, client).await,
        ClientRequest::StreamUnsub(req) => forward_stream_unsub(req, client).await,
        ClientRequest::PipelineStageSub(req) => forward_pipeline_stage_sub(req, client).await,
        ClientRequest::UpdateSchema(req) => forward_update_schema(req, client).await,
    }
}

#[tracing::instrument(level = "trace", skip(req, client))]
async fn forward_transaction(req: Transaction, client: ClientClient<Channel>) {
    todo!("")
}

#[tracing::instrument(level = "trace", skip(req, client))]
async fn forward_ephemeral_pub(req: EphemeralPub, client: ClientClient<Channel>) {
    todo!("")
}

#[tracing::instrument(level = "trace", skip(req, client))]
async fn forward_ephemeral_sub(req: EphemeralSub, client: ClientClient<Channel>) {
    todo!("")
}

#[tracing::instrument(level = "trace", skip(req, client))]
async fn forward_rpc_pub(req: RpcPub, client: ClientClient<Channel>) {
    todo!("")
}

#[tracing::instrument(level = "trace", skip(req, client))]
async fn forward_rpc_sub(req: RpcSub, client: ClientClient<Channel>) {
    todo!("")
}

#[tracing::instrument(level = "trace", skip(req, client))]
async fn forward_stream_pub(req: StreamPub, mut client: ClientClient<Channel>) {
    tracing::info!("forwarding stream pub request");
    let tx = req.tx;
    let res = client
        .stream_pub(build_request_with_creds(req.req, req.creds))
        .await
        .map(|res| res.into_inner());
    let _ = tx.send(res);
}

#[tracing::instrument(level = "trace", skip(req, client))]
async fn forward_stream_sub(req: StreamSub, client: ClientClient<Channel>) {
    todo!("")
}

#[tracing::instrument(level = "trace", skip(req, client))]
async fn forward_stream_unsub(req: StreamUnsub, client: ClientClient<Channel>) {
    todo!("")
}

#[tracing::instrument(level = "trace", skip(req, client))]
async fn forward_pipeline_stage_sub(req: PipelineStageSub, client: ClientClient<Channel>) {
    todo!("")
}

#[tracing::instrument(level = "trace", skip(req, client))]
async fn forward_update_schema(req: UpdateSchema, mut client: ClientClient<Channel>) {
    tracing::info!("forwarding update schema request");
    let tx = req.tx;
    let res = client
        .update_schema(build_request_with_creds(req.req, req.creds))
        .await
        .map(|res| res.into_inner());
    let _ = tx.send(res);
}

fn build_request_with_creds<T>(req: T, creds: TokenCredentials) -> Request<T> {
    let mut request = Request::new(req);
    request.metadata_mut().insert(utils::HEADER_X_HADRON_AUTH, creds.auth_header);
    request
}
