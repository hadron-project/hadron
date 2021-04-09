use std::sync::Arc;

use anyhow::Result;
use async_raft::{AppData, AppDataResponse};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::models::placement::{Assignment, StreamReplica};
use crate::models::schema;
use crate::network::{ClientRequest, UpdateSchema};
use crate::network::{RaftAppendEntries, RaftInstallSnapshot, RaftVote};
use crate::proto::client::UpdateSchemaResponse;
use crate::utils;

/// All request variants which the CRC can handle.
pub enum CRCRequest {
    Client(CRCClientRequest),
    Placement(AssignStreamReplicaToNode),
    RaftAppendEntries(RaftAppendEntries),
    RaftVote(RaftVote),
    RaftInstallSnapshot(RaftInstallSnapshot),
}

impl CRCRequest {
    pub(super) fn respond_with_error(self, err: anyhow::Error) {
        match self {
            CRCRequest::Client(CRCClientRequest::UpdateSchema(val)) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            CRCRequest::Placement(val) => {
                let _ = val.tx.send(Err(err));
            }
            CRCRequest::RaftAppendEntries(val) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            CRCRequest::RaftVote(val) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            CRCRequest::RaftInstallSnapshot(val) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
        }
    }
}

impl From<CRCClientRequest> for CRCRequest {
    fn from(src: CRCClientRequest) -> Self {
        CRCRequest::Client(src)
    }
}

/// All requests variants which the Hadron core data layer can handle, specifically those which
/// can come from clients.
pub enum CRCClientRequest {
    UpdateSchema(UpdateSchema),
}

/// A CPC request to assign a stream replica to a specific node.
pub struct AssignStreamReplicaToNode {
    pub change: Assignment,
    pub replica: Arc<StreamReplica>,
    pub tx: oneshot::Sender<Result<RaftAssignStreamReplicaToNodeResponse>>,
}

/// The Raft request type used by the CRC.
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum RaftClientRequest {
    UpdateSchema(RaftUpdateSchemaRequest),
    AssignStreamReplicaToNode(RaftAssignStreamReplicaToNode),
}

/// A request to perform a schema update.
#[derive(Serialize, Deserialize, Clone)]
pub struct RaftUpdateSchemaRequest {
    /// The validated form of the request.
    pub validated: schema::SchemaUpdate,
    /// The ID of the token provided on the request.
    pub token_id: u64,
}

/// A request from the CPC to assign a stream replica to a specific node.
#[derive(Serialize, Deserialize, Clone)]
pub struct RaftAssignStreamReplicaToNode {
    /// The ID of the node to which the replica will be assigned.
    pub change: Assignment,
    /// The ID of the stream replica.
    pub replica: u64,
}

impl AppData for RaftClientRequest {}

/// The Raft response type used by the CRC.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum RaftClientResponse {
    UpdateSchema(UpdateSchemaResponse),
    AssignStreamReplicaToNode(RaftAssignStreamReplicaToNodeResponse),
}

/// A response object for stream replica node assignment.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RaftAssignStreamReplicaToNodeResponse {}

impl AppDataResponse for RaftClientResponse {}

impl From<CRCClientRequest> for ClientRequest {
    fn from(src: CRCClientRequest) -> Self {
        match src {
            CRCClientRequest::UpdateSchema(req) => ClientRequest::UpdateSchema(req),
        }
    }
}
