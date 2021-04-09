//! The Client gRPC service implementation module.

use std::hash::Hasher;
use std::sync::Arc;

use tokio::stream::StreamExt;
use tokio::sync::{mpsc, oneshot};
use tonic::metadata::MetadataMap;
use tonic::{async_trait, transport::Channel, Request, Response, Status, Streaming};

use crate::auth::TokenCredentials;
use crate::config::Config;
use crate::ctl_raft::CRCIndex;
use crate::models::{placement::ControlGroup, schema};
pub use crate::proto::client::client_client::ClientClient;
use crate::proto::client::client_server::Client;
pub use crate::proto::client::client_server::ClientServer;
use crate::proto::client::{
    stream_pub_client::Request as StreamPubClientRequest, stream_pub_server::Response as StreamPubServerResponse, StreamPubClient,
    StreamPubConnectResponse, StreamPubServer, StreamSubClient, StreamSubServer,
};
use crate::proto::client::{EphemeralPubRequest, EphemeralPubResponse, EphemeralSubClient, EphemeralSubServer};
use crate::proto::client::{PipelineStageSubClient, PipelineStageSubServer, StreamUnsubRequest, StreamUnsubResponse};
use crate::proto::client::{RpcPubRequest, RpcPubResponse, RpcSubClient, RpcSubServer};
use crate::proto::client::{TransactionClient, TransactionServer, UpdateSchemaRequest, UpdateSchemaResponse};
use crate::utils::{self, map_result_to_status, status_from_rcv_error, TonicResult};
use crate::NodeId;

pub(super) struct ClientService {
    /// This node's ID.
    id: NodeId,
    config: Arc<Config>,
    /// The CRC data index.
    index: Arc<CRCIndex>,
    /// The channel used for propagating requests into the network actor, which flows into the rest of the system.
    network: mpsc::UnboundedSender<ClientRequest>, // TODO: In the future, we will want to document this & make capacity configurable.
}

impl ClientService {
    pub fn new(id: NodeId, config: Arc<Config>, index: Arc<CRCIndex>, network: mpsc::UnboundedSender<ClientRequest>) -> Self {
        Self { id, config, index, network }
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

    type StreamPubStream = mpsc::UnboundedReceiver<TonicResult<StreamPubServer>>;
    async fn stream_pub(&self, req: Request<Streaming<StreamPubClient>>) -> TonicResult<Response<Self::StreamPubStream>> {
        // Authenticate request & ensure caller is authorized to publish to the target stream.
        let creds = self.must_get_token(req.metadata())?;
        let _claims = self.index.must_get_token_claims(&creds.id).map_err(utils::status_from_err)?;
        // TODO: authz

        // Unpack the initial connection frame for determining exactly where
        // this streaming connection needs to be sent.
        let mut req = req.into_inner();
        let conn = req
            .next()
            .await
            .unwrap_or_else(|| Err(Status::cancelled("channel closed by client")))
            .and_then(|frame| match frame.request {
                Some(StreamPubClientRequest::Connect(conn)) => Ok(conn),
                _ => Err(Status::invalid_argument("expected an initial connect frame")),
            })?;

        // Fetch the stream's ID.
        let (tx, rx) = mpsc::unbounded_channel();
        let mut hasher = seahash::SeaHasher::new();
        hasher.write(conn.namespace.as_bytes());
        hasher.write_u8(b'/');
        hasher.write(conn.stream.as_bytes());
        let hash = hasher.finish();
        let stream_id = match self.index.stream_names.get(&hash).map(|res| *res.value()) {
            Some(stream_id) => stream_id,
            None => {
                let _ = tx.send(Err(Status::invalid_argument("metadata on the target stream does not exist on this node")));
                return Ok(Response::new(rx));
            }
        };

        // Fetch the target control group for the partition.
        let cg_opt = self
            .index
            .stream_control_groups
            .get(&(stream_id, conn.partition))
            .map(|res| *res.value())
            .and_then(|idx| self.index.control_groups.get(&idx).map(|res| res.value().clone()));
        let cg = match cg_opt {
            Some(cg) => cg,
            None => {
                tracing::error!("inconsistent data index, this is a bug"); // TOOD: make these consistent.
                let _ = tx.send(Err(tonic::Status::failed_precondition("inconsistent data index, this is a bug")));
                return Ok(Response::new(rx));
            }
        };

        // If this node is not the partition leader, then respond with info on the leader node.
        if let Some((leader, _)) = &cg.leader {
            if &self.id != leader {
                // Fetch DNS info on leader host.
                // TODO: CRITICAL PATH:
                // - keep DNS associations with node IDs as they go through discovery and handshake.
                // - dashmap the data, and use it here.
                let response = Some(StreamPubServerResponse::Connect(StreamPubConnectResponse {
                    is_ready: false,
                    leader: leader.to_string(),
                }));
                let _ = tx.send(Ok(StreamPubServer { response }));
                return Ok(Response::new(rx));
            }
        }

        let _ = self.network.send(ClientRequest::StreamPub(StreamPub { req, tx, creds, cg }));
        Ok(Response::new(rx))
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
        let _claims = self.index.must_get_token_claims(&creds.id).map_err(utils::status_from_err)?;
        let req = req.into_inner();
        let validated = schema::SchemaUpdate::decode_and_validate(&req).map_err(utils::status_from_err)?;
        let (tx, rx) = oneshot::channel();
        let _ = self.network.send(ClientRequest::UpdateSchema(UpdateSchema { req, validated, tx, creds }));
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
    pub req: Streaming<StreamPubClient>,
    pub tx: mpsc::UnboundedSender<TonicResult<StreamPubServer>>,
    pub creds: TokenCredentials,
    pub cg: Arc<ControlGroup>,
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
    pub validated: schema::SchemaUpdate,
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

#[tracing::instrument(level = "trace", skip(_req, _client))]
async fn forward_transaction(_req: Transaction, _client: ClientClient<Channel>) {
    todo!("") // TODO: remove as forwarding will not be used here.
}

#[tracing::instrument(level = "trace", skip(_req, _client))]
async fn forward_ephemeral_pub(_req: EphemeralPub, _client: ClientClient<Channel>) {
    todo!("") // TODO: remove as forwarding will not be used here.
}

#[tracing::instrument(level = "trace", skip(_req, _client))]
async fn forward_ephemeral_sub(_req: EphemeralSub, _client: ClientClient<Channel>) {
    todo!("") // TODO: remove as forwarding will not be used here.
}

#[tracing::instrument(level = "trace", skip(_req, _client))]
async fn forward_rpc_pub(_req: RpcPub, _client: ClientClient<Channel>) {
    todo!("") // TODO: remove as forwarding will not be used here.
}

#[tracing::instrument(level = "trace", skip(_req, _client))]
async fn forward_rpc_sub(_req: RpcSub, _client: ClientClient<Channel>) {
    todo!("") // TODO: remove as forwarding will not be used here.
}

#[tracing::instrument(level = "trace", skip(_req, _client))]
async fn forward_stream_pub(_req: StreamPub, _client: ClientClient<Channel>) {
    todo!("") // TODO: remove as forwarding will not be used here.
}

#[tracing::instrument(level = "trace", skip(_req, _client))]
async fn forward_stream_sub(_req: StreamSub, _client: ClientClient<Channel>) {
    todo!("") // TODO: remove as forwarding will not be used here.
}

#[tracing::instrument(level = "trace", skip(_req, _client))]
async fn forward_stream_unsub(_req: StreamUnsub, _client: ClientClient<Channel>) {
    todo!("") // TODO: remove as forwarding will not be used here.
}

#[tracing::instrument(level = "trace", skip(_req, _client))]
async fn forward_pipeline_stage_sub(_req: PipelineStageSub, _client: ClientClient<Channel>) {
    todo!("") // TODO: remove as forwarding will not be used here.
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
    request.metadata_mut().insert(utils::HEADER_X_HADRON_AUTH, creds.header);
    request
}
