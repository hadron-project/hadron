//! The Client gRPC service implementation module.

use anyhow::Result;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, oneshot};

use crate::network::{map_result_to_status, status_from_rcv_error, TonicResult};
use crate::proto::client::client_server::Client;
pub use crate::proto::client::client_server::ClientServer;
use crate::proto::client::{EphemeralPubRequest, EphemeralPubResponse, EphemeralSubClient, EphemeralSubServer};
use crate::proto::client::{PipelineStageSubClient, PipelineStageSubServer, StreamUnsubRequest, StreamUnsubResponse};
use crate::proto::client::{RpcPubRequest, RpcPubResponse, RpcSubClient, RpcSubServer};
use crate::proto::client::{StreamPubRequest, StreamPubResponse, StreamSubClient, StreamSubServer};
use crate::proto::client::{UpdateSchemaRequest, UpdateSchemaResponse};
use crate::NodeId;

use tonic::{async_trait, Request, Response};

pub(super) struct ClientService {
    /// This node's ID.
    id: NodeId,
    /// The channel used for propagating requests into the network actor, which flows into the rest of the system.
    network: mpsc::UnboundedSender<ClientRequest>, // TODO: In the future, we will want to document this & make capacity configurable.
}

impl ClientService {
    pub fn new(id: NodeId, network: mpsc::UnboundedSender<ClientRequest>) -> Self {
        Self { id, network }
    }
}

#[async_trait]
impl Client for ClientService {
    async fn ephemeral_pub(&self, request: Request<EphemeralPubRequest>) -> TonicResult<Response<EphemeralPubResponse>> {
        let (tx, rx) = oneshot::channel();
        let msg = ClientRequest::EphemeralPub(request, tx);
        let _ = self.network.send(msg);
        rx.await.map_err(status_from_rcv_error).and_then(map_result_to_status).map(Response::new)
    }

    type EphemeralSubStream = mpsc::UnboundedReceiver<TonicResult<EphemeralSubServer>>;
    async fn ephemeral_sub(&self, request: Request<EphemeralSubClient>) -> TonicResult<Response<Self::EphemeralSubStream>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let msg = ClientRequest::EphemeralSub(request, tx);
        let _ = self.network.send(msg);
        Ok(Response::new(rx))
    }

    async fn rpc_pub(&self, request: Request<RpcPubRequest>) -> TonicResult<Response<RpcPubResponse>> {
        let (tx, rx) = oneshot::channel();
        let msg = ClientRequest::RpcPub(request, tx);
        let _ = self.network.send(msg);
        rx.await.map_err(status_from_rcv_error).and_then(map_result_to_status).map(Response::new)
    }

    type RpcSubStream = mpsc::UnboundedReceiver<TonicResult<RpcSubServer>>;
    async fn rpc_sub(&self, request: Request<tonic::Streaming<RpcSubClient>>) -> TonicResult<Response<Self::RpcSubStream>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let network = self.network.clone();
        tokio::spawn(async move {
            // Send the initial stream confirmation.
            let _ = tx.send(Ok(RpcSubServer {}));
            let mut req_stream = request.into_inner();
            while let Some(Ok(req)) = req_stream.next().await {
                let _ = network.send(ClientRequest::RpcSub(req, tx.clone()));
            }
        });
        Ok(Response::new(rx))
    }

    async fn stream_pub(&self, request: Request<StreamPubRequest>) -> TonicResult<Response<StreamPubResponse>> {
        let (tx, rx) = oneshot::channel();
        let msg = ClientRequest::StreamPub(request, tx);
        let _ = self.network.send(msg);
        rx.await.map_err(status_from_rcv_error).and_then(map_result_to_status).map(Response::new)
    }

    type StreamSubStream = mpsc::UnboundedReceiver<TonicResult<StreamSubServer>>;
    async fn stream_sub(&self, request: Request<tonic::Streaming<StreamSubClient>>) -> TonicResult<Response<Self::StreamSubStream>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let network = self.network.clone();
        tokio::spawn(async move {
            // Send the initial stream confirmation.
            let _ = tx.send(Ok(StreamSubServer {}));
            let mut req_stream = request.into_inner();
            while let Some(Ok(req)) = req_stream.next().await {
                let _ = network.send(ClientRequest::StreamSub(req, tx.clone()));
            }
        });
        Ok(Response::new(rx))
    }

    async fn stream_unsub(&self, request: Request<StreamUnsubRequest>) -> TonicResult<Response<StreamUnsubResponse>> {
        let (tx, rx) = oneshot::channel();
        let msg = ClientRequest::StreamUnsub(request, tx);
        let _ = self.network.send(msg);
        rx.await.map_err(status_from_rcv_error).and_then(map_result_to_status).map(Response::new)
    }

    type PipelineStageSubStream = mpsc::UnboundedReceiver<TonicResult<PipelineStageSubServer>>;
    async fn pipeline_stage_sub(
        &self, request: Request<tonic::Streaming<PipelineStageSubClient>>,
    ) -> TonicResult<Response<Self::PipelineStageSubStream>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let network = self.network.clone();
        tokio::spawn(async move {
            // Send the initial stream confirmation.
            let _ = tx.send(Ok(PipelineStageSubServer {}));
            let mut req_stream = request.into_inner();
            while let Some(Ok(req)) = req_stream.next().await {
                let _ = network.send(ClientRequest::PipelineStageSub(req, tx.clone()));
            }
        });
        Ok(Response::new(rx))
    }

    async fn update_schema(&self, request: Request<UpdateSchemaRequest>) -> TonicResult<Response<UpdateSchemaResponse>> {
        let (tx, rx) = oneshot::channel();
        let msg = ClientRequest::UpdateSchema(request, tx);
        let _ = self.network.send(msg);
        rx.await.map_err(status_from_rcv_error).and_then(map_result_to_status).map(Response::new)
    }
}

/// A request flowing into this node from a client.
pub(super) enum ClientRequest {
    EphemeralPub(Request<EphemeralPubRequest>, oneshot::Sender<Result<EphemeralPubResponse>>),
    EphemeralSub(Request<EphemeralSubClient>, mpsc::UnboundedSender<TonicResult<EphemeralSubServer>>),
    RpcPub(Request<RpcPubRequest>, oneshot::Sender<Result<RpcPubResponse>>),
    RpcSub(RpcSubClient, mpsc::UnboundedSender<TonicResult<RpcSubServer>>),
    StreamPub(Request<StreamPubRequest>, oneshot::Sender<Result<StreamPubResponse>>),
    StreamSub(StreamSubClient, mpsc::UnboundedSender<TonicResult<StreamSubServer>>),
    StreamUnsub(Request<StreamUnsubRequest>, oneshot::Sender<Result<StreamUnsubResponse>>),
    PipelineStageSub(PipelineStageSubClient, mpsc::UnboundedSender<TonicResult<PipelineStageSubServer>>),
    UpdateSchema(Request<UpdateSchemaRequest>, oneshot::Sender<Result<UpdateSchemaResponse>>),
}
