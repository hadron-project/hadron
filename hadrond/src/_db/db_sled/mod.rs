//! A module encapsulating all logic for interfacing with the data storage system.

mod consts;
mod init;
mod raft;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use actix::prelude::*;
use actix_raft::storage::HardState;
use failure;
use sled;

use crate::{
    NodeId,
    auth::User,
    db::{
        models::{Pipeline, StreamWrapper},
        stream::{StorageProducer, StorageUpdatesStream},
    },
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// SledStorage ///////////////////////////////////////////////////////////////////////////////////

#[allow(dead_code)]
pub(self) struct ObjectCollections {
    users_collection: sled::Tree,
    tokens_collection: sled::Tree,
    ns_collection: sled::Tree,
    endpoints_collection: sled::Tree,
    pipelines_collection: sled::Tree,
    streams_collection: sled::Tree,
}

/// An implementation of the Railgun storage engine using Sled.
///
/// This actor is responsible for orchestrating all interaction with the storage engine. It
/// implements the `RaftStorage` interface, and all data mutations must go through Raft.
///
/// This actor also uses a set of synchronous actors which perform the actual blocking IO on the
/// sled storage engine. However, this actor manages the memory of the sled objects, as the sync
/// actor instances are blind to the higher level abstractions of the system.
pub struct SledStorage {
    /// The ID of this node.
    id: NodeId,
    /// The addr of the sled sync actor for DB operations.
    sled: Addr<raft::SledStorageSync>,
    /// The main sled database handle.
    db: sled::Db,
    /// The keyspace used for the Raft log.
    log: sled::Tree,
    /// The latest hardstate of the node.
    ///
    /// This is only updated after successfully being written to disk.
    hs: HardState,
    /// The index of the last Raft entry to be appended to the log.
    last_log_index: u64,
    /// The term of the last Raft entry to be appended to the log.
    last_log_term: u64,
    /// The index of the last Raft entry to be applied to storage.
    last_applied_log: u64, // TODO: ensure this is updated.

    #[allow(dead_code)] object_collections: Arc<ObjectCollections>,
    #[allow(dead_code)] indexed_users: BTreeMap<String, User>,
    #[allow(dead_code)] indexed_tokens: BTreeSet<String>,
    #[allow(dead_code)] indexed_namespaces: BTreeSet<String>,
    #[allow(dead_code)] indexed_endpoints: BTreeSet<String>,
    #[allow(dead_code)] indexed_pipelines: BTreeMap<String, Pipeline>,
    indexed_streams: BTreeMap<String, StreamWrapper>,
    /// New streams which have been written to the log, but which have not yet
    /// been applied to the state machine.
    pending_streams: BTreeMap<u64, String>,
    producer: StorageProducer,
}

impl SledStorage {
    /// This node's ID.
    pub fn node_id(&self) -> NodeId {
        self.id
    }

    /// Get the keyspace for a stream's data given its namespace & name.
    fn stream_keyspace_data(ns: &str, name: &str) -> String {
        format!("{}/{}/{}/data", consts::STREAMS_DATA_PREFIX, ns, name)
    }

    /// Get the keyspace for a stream's metadata given its namespace & name.
    fn stream_keyspace_metadata(ns: &str, name: &str) -> String {
        format!("{}/{}/{}/metadata", consts::STREAMS_DATA_PREFIX, ns, name)
    }

    /// Subscribe to all storage updates.
    pub fn subscribe(&mut self) -> StorageUpdatesStream {
        self.producer.subscribe()
    }
}

impl Actor for SledStorage {
    type Context = Context<Self>;
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Utils /////////////////////////////////////////////////////////////////////////////////////////

/// Cast a bytes buffer to a u64.
fn u64_from_be_bytes(buf: sled::IVec) -> Result<u64, failure::Error> {
    use std::convert::TryInto;
    let (int_bytes, _) = buf.split_at(std::mem::size_of::<u64>());
    Ok(u64::from_be_bytes(int_bytes.try_into()?))
}

/// Cast a bytes buffer to a u64.
///
/// This routine should only be invoked when it is POSITIVE that the bytes are indeed a u64 BE.
/// This routine will not do bounds checks and unwraps the fallible cast.
#[allow(dead_code)]
fn unchecked_u64_from_be_bytes(buf: sled::IVec) -> u64 {
    use std::convert::TryInto;
    let (int_bytes, _) = buf.split_at(std::mem::size_of::<u64>());
    u64::from_be_bytes(int_bytes.try_into().unwrap())
}
