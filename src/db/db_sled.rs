//! A module encapsulating all logic for interfacing with the data storage system.

use std::{
    collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Arc,
};

use actix::prelude::*;
use actix_raft::{
    RaftStorage,
    messages::{Entry, MembershipConfig},
    storage::{
        AppendLogEntry,
        ApplyToStateMachine,
        CreateSnapshot,
        CurrentSnapshotData,
        GetCurrentSnapshot,
        GetInitialState,
        GetLogEntries,
        HardState,
        InitialState,
        InstallSnapshot,
        // InstallSnapshotChunk,
        ReplicateLogEntries,
        SaveHardState,
    },
};
use bincode;
use failure;
use log::{debug, error, info};
use sled;
use uuid;

use crate::{
    NodeId,
    auth::User,
    db::{AppData, models::{Pipeline, Stream, StreamType, StreamEntry}},
    proto::client::api::{ClientError},
};

/// The path to the raft log.
const RAFT_LOG_PREFIX: &str = "/raft/log/";
/// The key under which the Raft log's hard state is kept.
const RAFT_HARDSTATE_KEY: &str = "/raft/hs";
/// The key under which the Raft last-applied-log index is kept.
const RAFT_LAL_KEY: &str = "/raft/lal";
/// The key used for storing the node ID of the current node.
const NODE_ID_KEY: &str = "id";

/// The DB path prefix for all users.
const OBJECTS_USERS: &str = "/objects/users/";
/// The DB path prefix for all users.
const OBJECTS_TOKENS: &str = "/objects/tokens/";
/// The DB path prefix for all namespaces.
const OBJECTS_NS: &str = "/objects/ns/";
/// The DB path prefix for all namespaces.
const OBJECTS_ENDPOINTS: &str = "/objects/endpoints/";
/// The DB path prefix for all streams.
const OBJECTS_STREAMS: &str = "/objects/streams/";
/// The DB path prefix for all pipelines.
const OBJECTS_PIPELINES: &str = "/objects/pipelines/";

/// The prefix under which all streams store their data.
///
/// Streams MUST index their data as `/streams/<namespace>/<stream_name>/<entry_index>`.
const STREAMS_DATA_PREFIX: &str = "/streams";

/// An error from deserializing an entry.
const ERR_DESERIALIZE_ENTRY: &str = "Failed to deserialize entry from log. Data is corrupt.";
/// An error from deserializing HardState record.
const ERR_DESERIALIZE_HS: &str = "Failed to deserialize HardState from storage. Data is corrupt.";
/// An error from deserializing a User record.
const ERR_DESERIALIZE_USER: &str = "Failed to deserialize User from storage. Data is corrupt.";
/// An error from deserializing a Pipeline record.
const ERR_DESERIALIZE_PIPELINE: &str = "Failed to deserialize Pipeline from storage. Data is corrupt.";
/// An error from deserializing a Stream record.
const ERR_DESERIALIZE_STREAM: &str = "Failed to deserialize Stream from storage. Data is corrupt.";
/// An error from deserializing a Stream Entry record.
const ERR_DESERIALIZE_STREAM_ENTRY: &str = "Failed to deserialize Stream Entry from storage. Data is corrupt.";
/// An initialization error where an entry was expected, but none was found.
const ERR_MISSING_ENTRY: &str = "Unable to access expected log entry.";
/// An error from an endpoint's index being malformed.
const ERR_MALFORMED_ENDPOINT_INDEX: &str = "Malformed index for endpoint.";

type RgEntry = Entry<AppData>;
type RgAppendLogEntry = AppendLogEntry<AppData, ClientError>;
type RgApplyToStateMachine = ApplyToStateMachine<AppData, ClientError>;
type RgCreateSnapshot = CreateSnapshot<ClientError>;
type RgGetCurrentSnapshot = GetCurrentSnapshot<ClientError>;
type RgGetInitialState = GetInitialState<ClientError>;
type RgGetLogEntries = GetLogEntries<AppData, ClientError>;
type RgInstallSnapshot = InstallSnapshot<ClientError>;
type RgReplicateLogEntries = ReplicateLogEntries<AppData, ClientError>;
type RgSaveHardState = SaveHardState<ClientError>;

//////////////////////////////////////////////////////////////////////////////////////////////////
// SledStorage ///////////////////////////////////////////////////////////////////////////////////

#[allow(dead_code)]
struct ObjectCollections {
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
    sled: Addr<SledActor>,
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
    last_applied_log: u64,
    object_collections: Arc<ObjectCollections>,
    #[allow(dead_code)] indexed_users: BTreeMap<String, User>,
    #[allow(dead_code)] indexed_tokens: BTreeSet<String>,
    #[allow(dead_code)] indexed_namespaces: BTreeSet<String>,
    #[allow(dead_code)] indexed_endpoints: BTreeSet<(String, String)>,
    #[allow(dead_code)] indexed_pipelines: BTreeMap<(String, String), Pipeline>,
    #[allow(dead_code)] indexed_streams: BTreeMap<(String, String), Stream>,
}

impl SledStorage {
    /// Create a new instance.
    ///
    /// This will initialize the data store, and will ensure that the database has a node ID.
    pub fn new(db_path: &str) -> Result<Self, failure::Error> {
        info!("Initializing storage engine SledStorage.");
        let db = sled::Db::open(db_path)?;
        let mut hasher = DefaultHasher::default();
        let id: u64 = match db.get(NODE_ID_KEY)? {
            Some(id) => unchecked_u64_from_be_bytes(id),
            None => {
                uuid::Uuid::new_v4().hash(&mut hasher);
                let id: u64 = hasher.finish();
                db.insert(NODE_ID_KEY, id.to_be_bytes().as_ref())?;
                id
            }
        };
        debug!("Node ID: {}", id);

        // Initialize and restore any previous state from disk.
        let log = db.open_tree(RAFT_LOG_PREFIX)?;
        let state = Self::initialize(id, &db, &log)?;

        // Build indices.
        info!("Indexing data.");
        let users_collection = db.open_tree(OBJECTS_USERS)?;
        let indexed_users = Self::index_users_data(&users_collection)?;
        let tokens_collection = db.open_tree(OBJECTS_TOKENS)?;
        let indexed_tokens = Self::index_tokens_data(&tokens_collection)?;
        let ns_collection = db.open_tree(OBJECTS_NS)?;
        let indexed_namespaces = Self::index_ns_data(&ns_collection)?;
        let endpoints_collection = db.open_tree(OBJECTS_ENDPOINTS)?;
        let indexed_endpoints = Self::index_endpoints_data(&endpoints_collection)?;
        let pipelines_collection = db.open_tree(OBJECTS_PIPELINES)?;
        let indexed_pipelines = Self::index_pipelines_data(&pipelines_collection)?;
        let streams_collection = db.open_tree(OBJECTS_STREAMS)?;
        let indexed_streams = Self::index_streams_data(&db, &streams_collection)?;
        let object_collections = Arc::new(ObjectCollections{
            endpoints_collection, ns_collection,
            pipelines_collection, streams_collection,
            tokens_collection, users_collection,
        });
        info!("Finished indexing data.");

        let sled = SyncArbiter::start(3, move || SledActor); // TODO: probably use `num_cores` crate.
        Ok(SledStorage{
            id, sled, db, log,
            hs: state.hard_state, object_collections,
            last_log_index: state.last_log_index,
            last_log_term: state.last_log_term,
            last_applied_log: state.last_applied_log,
            indexed_endpoints, indexed_namespaces,
            indexed_pipelines, indexed_streams,
            indexed_tokens, indexed_users,
        })
    }

    /// Initialize and restore any previous state from disk.
    fn initialize(id: NodeId, db: &sled::Db, log: &sled::Tree) -> Result<InitialState, failure::Error> {
        // If the log is empty, then return the default initial state.
        if log.is_empty() {
            return Ok(InitialState{
                last_log_index: 0, last_log_term: 0, last_applied_log: 0,
                hard_state: HardState{
                    current_term: 0, voted_for: None,
                    membership: MembershipConfig{
                        is_in_joint_consensus: false,
                        members: vec![id], non_voters: Vec::new(), removing: Vec::new(),
                    },
                },
            });
        }

        // There is a log entry, so fetch it and extract needed values.
        let (_k, val) = log.iter().rev().nth(0).ok_or(failure::err_msg(ERR_MISSING_ENTRY))??;
        let entry: RgEntry = bincode::deserialize(&val).map_err(|err| {
            error!("{} {:?}", ERR_DESERIALIZE_ENTRY, err);
            failure::err_msg(ERR_DESERIALIZE_ENTRY)
        })?;
        let (last_log_index, last_log_term) = (entry.index, entry.term);

        // Check for the last applied log.
        let mut last_applied_log = 0;
        if let Some(idx_raw) = db.get(RAFT_LAL_KEY)? {
            last_applied_log = unchecked_u64_from_be_bytes(idx_raw);
        }

        // Check for hard state.
        let hard_state = if let Some(hs_raw) = db.get(RAFT_HARDSTATE_KEY)? {
            bincode::deserialize::<HardState>(&hs_raw).map_err(|err| {
                error!("{} {:?}", ERR_DESERIALIZE_HS, err);
                failure::err_msg(ERR_DESERIALIZE_HS)
            })?
        } else {
            HardState{
                current_term: 0, voted_for: None,
                membership: MembershipConfig{
                    is_in_joint_consensus: false,
                    members: vec![id], non_voters: Vec::new(), removing: Vec::new(),
                },
            }
        };
        Ok(InitialState{last_log_index, last_log_term, last_applied_log, hard_state})
    }

    /// Index users data.
    fn index_users_data(coll: &sled::Tree) -> Result<BTreeMap<String, User>, failure::Error> {
        let mut index = BTreeMap::new();
        for res in coll.iter() {
            let (_, model_raw) = res?;
            let user: User = bincode::deserialize(&model_raw).map_err(|err| {
                error!("{} {}", ERR_DESERIALIZE_USER, err);
                failure::err_msg(ERR_DESERIALIZE_USER)
            })?;
            index.insert(user.name.clone(), user);
        }
        Ok(index)
    }

    /// Index tokens data.
    fn index_tokens_data(coll: &sled::Tree) -> Result<BTreeSet<String>, failure::Error> {
        let mut index = BTreeSet::new();
        for res in coll.iter() {
            let (_, token) = res?;
            let val = String::from_utf8(token.to_vec())?;
            index.insert(val);
        }
        Ok(index)
    }

    /// Index namespaces data.
    fn index_ns_data(coll: &sled::Tree) -> Result<BTreeSet<String>, failure::Error> {
        let mut index = BTreeSet::new();
        for res in coll.iter() {
            let (_, ns) = res?;
            let val = String::from_utf8(ns.to_vec())?;
            index.insert(val);
        }
        Ok(index)
    }

    /// Index endpoints data.
    fn index_endpoints_data(coll: &sled::Tree) -> Result<BTreeSet<(String, String)>, failure::Error> {
        let mut index = BTreeSet::new();
        for res in coll.iter() {
            let (_, endpoint) = res?;
            let val = String::from_utf8(endpoint.to_vec())?;
            let parts = val.splitn(2, "/").collect::<Vec<_>>();
            let ns = parts.get(0).ok_or_else(|| failure::err_msg(ERR_MALFORMED_ENDPOINT_INDEX))?.to_string();
            let endpoint = parts.get(1).ok_or_else(|| failure::err_msg(ERR_MALFORMED_ENDPOINT_INDEX))?.to_string();
            index.insert((ns, endpoint));
        }
        Ok(index)
    }

    /// Index pipelines data.
    fn index_pipelines_data(coll: &sled::Tree) -> Result<BTreeMap<(String, String), Pipeline>, failure::Error> {
        let mut index = BTreeMap::new();
        for res in coll.iter() {
            let (_, model_raw) = res?;
            let pipeline: Pipeline = bincode::deserialize(&model_raw).map_err(|err| {
                error!("{} {}", ERR_DESERIALIZE_PIPELINE, err);
                failure::err_msg(ERR_DESERIALIZE_PIPELINE)
            })?;
            index.insert((pipeline.namespace.clone(), pipeline.name.clone()), pipeline);
        }
        Ok(index)
    }

    /// Index streams data.
    fn index_streams_data(db: &sled::Db, coll: &sled::Tree) -> Result<BTreeMap<(String, String), Stream>, failure::Error> {
        let mut index = BTreeMap::new();
        for res in coll.iter() {
            let (_, model_raw) = res?;
            let mut stream: Stream = bincode::deserialize(&model_raw).map_err(|err| {
                error!("{} {}", ERR_DESERIALIZE_STREAM, err);
                failure::err_msg(ERR_DESERIALIZE_STREAM)
            })?;

            // If the stream has a unique ID constraint, then index its IDs.
            // TODO: we can parallelize this pretty aggressively with Rayon. See #26.
            if let StreamType::UniqueId{index: indexed_ids} = &mut stream.stream_type {
                let keyspace = SledStorage::stream_keyspace(&stream.namespace, &stream.name);
                for res in db.open_tree(keyspace)?.iter() {
                    let (_, rawmodel) = res?;
                    let entry: StreamEntry = bincode::deserialize(&rawmodel).map_err(|err| {
                        error!("{} {}", ERR_DESERIALIZE_STREAM_ENTRY, err);
                        failure::err_msg(ERR_DESERIALIZE_STREAM_ENTRY)
                    })?;
                    if let Some(id) = entry.id {
                        indexed_ids.insert(id);
                    }
                }
            }

            index.insert((stream.namespace.clone(), stream.name.clone()), stream);
        }
        Ok(index)
    }

    /// Get the keyspace for a stream given its namespace & name.
    fn stream_keyspace(ns: &str, name: &str) -> String {
        format!("{}/{}/{}/", STREAMS_DATA_PREFIX, ns, name)
    }

    /// This node's ID.
    pub fn node_id(&self) -> NodeId {
        self.id
    }

    /// Get a handle to the database.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn db(&self) -> sled::Db {
        self.db.clone()
    }

    /// Get a handle to the Raft log tree.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn log(&self) -> sled::Tree {
        self.log.clone()
    }
}

impl Actor for SledStorage {
    type Context = Context<Self>;
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Impl RaftStorage //////////////////////////////////////////////////////////////////////////////

impl RaftStorage<AppData, ClientError> for SledStorage {
    type Actor = Self;
    type Context = Context<Self>;
}

impl Handler<RgAppendLogEntry> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, msg: RgAppendLogEntry, _ctx: &mut Self::Context) -> Self::Result {
        Box::new(fut::wrap_future(self.sled.send(SyncAppendLogEntry(msg, self.log.clone())))
            .map_err(|_, _: &mut Self, _| ClientError::new_internal()) // TODO: log
            .and_then(|res, _, _| fut::result(res))
        )
    }
}

impl Handler<RgApplyToStateMachine> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, msg: RgApplyToStateMachine, _ctx: &mut Self::Context) -> Self::Result {
        Box::new(fut::wrap_future(self.sled.send(SyncApplyToStateMachine(msg, self.db.clone(), self.object_collections.clone())))
            .map_err(|_, _: &mut Self, _| ClientError::new_internal()) // TODO: log
            .and_then(|res, _, _| fut::result(res))
        )
    }
}

impl Handler<RgGetInitialState> for SledStorage {
    type Result = Result<InitialState, ClientError>;

    fn handle(&mut self, _msg: RgGetInitialState, _ctx: &mut Self::Context) -> Self::Result {
        Ok(InitialState{
            last_log_index: self.last_log_index,
            last_log_term: self.last_log_term,
            last_applied_log: self.last_applied_log,
            hard_state: self.hs.clone(),
        })
    }
}

impl Handler<RgGetLogEntries> for SledStorage {
    type Result = ResponseActFuture<Self, Vec<RgEntry>, ClientError>;

    fn handle(&mut self, msg: RgGetLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        Box::new(fut::wrap_future(self.sled.send(SyncGetLogEntries(msg, self.log.clone())))
            .map_err(|_, _: &mut Self, _| ClientError::new_internal()) // TODO: log
            .and_then(|res, _, _| fut::result(res))
        )
    }
}

impl Handler<RgReplicateLogEntries> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, msg: RgReplicateLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        Box::new(fut::wrap_future(self.sled.send(SyncReplicateLogEntries(msg, self.log.clone())))
            .map_err(|_, _: &mut Self, _| ClientError::new_internal()) // TODO: log
            .and_then(|res, _, _| fut::result(res))
        )
    }
}

impl Handler<RgSaveHardState> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, msg: RgSaveHardState, _ctx: &mut Self::Context) -> Self::Result {
        let hs = msg.hs.clone();
        Box::new(fut::wrap_future(self.sled.send(SyncSaveHardState(msg, self.db.clone())))
            .map_err(|_, _: &mut Self, _| ClientError::new_internal()) // TODO: log
            .and_then(|res, _, _| fut::result(res))
            .and_then(move |_, act, _| {
                act.hs = hs;
                fut::ok(())
            })
        )
    }
}

impl Handler<RgCreateSnapshot> for SledStorage {
    type Result = ResponseActFuture<Self, CurrentSnapshotData, ClientError>;

    fn handle(&mut self, _msg: RgCreateSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

impl Handler<RgGetCurrentSnapshot> for SledStorage {
    type Result = ResponseActFuture<Self, Option<CurrentSnapshotData>, ClientError>;

    fn handle(&mut self, _msg: RgGetCurrentSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

impl Handler<RgInstallSnapshot> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, _msg: RgInstallSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// SledActor /////////////////////////////////////////////////////////////////////////////////////

/// Sync actor for interfacing with Sled.
struct SledActor;

impl SledActor {
    fn apply_entry_to_state_machine(&mut self, _entry: &RgEntry, _db: &sled::Db, _colls: &Arc<ObjectCollections>) -> Result<(), ClientError> {
        Ok(()) // TODO: impl this
    }
}

impl Actor for SledActor {
    type Context = SyncContext<Self>;
}

struct SyncAppendLogEntry(RgAppendLogEntry, sled::Tree);

impl Message for SyncAppendLogEntry {
    type Result = Result<(), ClientError>;
}

impl Handler<SyncAppendLogEntry> for SledActor {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, msg: SyncAppendLogEntry, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: this is where application level constraints may be enforced,
        // just before the proposed entry hits the log.

        // Entry data checks out, so append it to the log.
        let data = bincode::serialize(&*msg.0.entry).map_err(|err| {
            error!("Error serializing log entry: {}", err);
            ClientError::new_internal()
        })?;
        msg.1.insert(msg.0.entry.index.to_be_bytes(), data.as_slice()).map_err(|err| {
            error!("Error serializing log entry: {}", err);
            ClientError::new_internal()
        })?;
        Ok(())
    }
}

struct SyncApplyToStateMachine(RgApplyToStateMachine, sled::Db, Arc<ObjectCollections>);

impl Message for SyncApplyToStateMachine {
    type Result = Result<(), ClientError>;
}

impl Handler<SyncApplyToStateMachine> for SledActor {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, msg: SyncApplyToStateMachine, _ctx: &mut Self::Context) -> Self::Result {
        use actix_raft::storage::ApplyToStateMachinePayload::{Multi, Single};
        match &msg.0.payload {
            Single(entry) => self.apply_entry_to_state_machine(&*entry, &msg.1, &msg.2)?,
            Multi(entries) => for entry in entries {
                self.apply_entry_to_state_machine(entry, &msg.1, &msg.2)?
            }
        }
        Ok(())
    }
}

struct SyncGetLogEntries(RgGetLogEntries, sled::Tree);

impl Message for SyncGetLogEntries {
    type Result = Result<Vec<RgEntry>, ClientError>;
}

impl Handler<SyncGetLogEntries> for SledActor {
    type Result = Result<Vec<RgEntry>, ClientError>;

    fn handle(&mut self, msg: SyncGetLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        let start = msg.0.start.to_be_bytes();
        let stop = msg.0.stop.to_be_bytes();
        let entries = msg.1.range(start..stop).values().try_fold(vec![], |mut acc, res| {
            let data = res.map_err(|err| {
                error!("Error from sled in GetLogEntries. {}", err);
                ClientError::new_internal()
            })?;
            let entry = bincode::deserialize(&data).map_err(|err| {
                error!("Error deserializing entry in  GetLogEntries. {}", err);
                ClientError::new_internal()
            })?;
            acc.push(entry);
            Ok(acc)
        })?;
        Ok(entries)
    }
}

struct SyncReplicateLogEntries(RgReplicateLogEntries, sled::Tree);

impl Message for SyncReplicateLogEntries {
    type Result = Result<(), ClientError>;
}

impl Handler<SyncReplicateLogEntries> for SledActor {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, msg: SyncReplicateLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        let mut batch = sled::Batch::default();
        for entry in msg.0.entries.iter() {
            let data = bincode::serialize(entry).map_err(|err| {
                error!("Error serializing log entry: {}", err);
                ClientError::new_internal()
            })?;
            batch.insert(&entry.index.to_be_bytes(), data.as_slice());
        }
        msg.1.apply_batch(batch).map_err(|err| {
            error!("Error applying batch of Raft log entries to storage: {}", err);
            ClientError::new_internal()
        })
    }
}

struct SyncSaveHardState(RgSaveHardState, sled::Db);

impl Message for SyncSaveHardState {
    type Result = Result<(), ClientError>;
}

impl Handler<SyncSaveHardState> for SledActor {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, msg: SyncSaveHardState, _ctx: &mut Self::Context) -> Self::Result {
        // Serialize data.
        let hs = bincode::serialize(&msg.0.hs).map_err(|err| {
            error!("Error while serializing Raft HardState object: {:?}", err);
            ClientError::new_internal()
        })?;

        // Write to disk.
        msg.1.insert(RAFT_HARDSTATE_KEY, hs).map_err(|err| {
            error!("Error while writing Raft HardState to disk: {:?}", err);
            ClientError::new_internal()
        })?;

        // Respond.
        Ok(())
    }
}

impl Handler<RgCreateSnapshot> for SledActor {
    type Result = Result<CurrentSnapshotData, ClientError>;

    fn handle(&mut self, _msg: RgCreateSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

impl Handler<RgGetCurrentSnapshot> for SledActor {
    type Result = Result<Option<CurrentSnapshotData>, ClientError>;

    fn handle(&mut self, _msg: RgGetCurrentSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

impl Handler<RgInstallSnapshot> for SledActor {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, _msg: RgInstallSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Utils /////////////////////////////////////////////////////////////////////////////////////////

/// Cast a bytes buffer to a u64.
///
/// This routine should only be invoked when it is POSITIVE that the bytes are indeed a u64 BE.
/// This routine will not do bounds checks and unwraps the fallible cast.
fn unchecked_u64_from_be_bytes(buf: sled::IVec) -> u64 {
    use std::convert::TryInto;
    let (int_bytes, _) = buf.split_at(std::mem::size_of::<u64>());
    u64::from_be_bytes(int_bytes.try_into().unwrap())
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Tests /////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::UserRole,
        db::models::StreamVisibility,
        proto::client::api,
    };
    use actix_raft::messages::{Entry, EntryType, EntryNormal};
    use std::sync::Arc;
    use proptest::prelude::*;
    use tempfile;
    use uuid;

    proptest! {
        #[test]
        fn unchecked_u64_from_be_bytes_should_succeed_for_all_be_u64_bytes(val in u64::min_value()..u64::max_value()) {
            let u64_bytes = sled::IVec::from(&val.to_be_bytes());
            let output = unchecked_u64_from_be_bytes(u64_bytes);
            assert_eq!(output, val);
        }
    }

    mod sled_storage {
        use super::*;

        #[test]
        fn index_users_data_should_return_empty_index_with_no_data() {
            let (_dir, db) = setup_db();
            let output = SledStorage::index_users_data(&db).expect("index users data");
            assert_eq!(output.len(), 0);
        }

        #[test]
        fn index_users_data_should_return_expected_index_with_populated_data() {
            let (_dir, db) = setup_db();
            let users_index = setup_base_users(&db);
            let output = SledStorage::index_users_data(&db).expect("index users data");
            assert_eq!(output, users_index);
            assert_eq!(output.len(), users_index.len());
            assert_eq!(output.len(), 3);
        }

        #[test]
        fn index_tokens_data_should_return_empty_index_with_no_data() {
            let (_dir, db) = setup_db();
            let output = SledStorage::index_tokens_data(&db).expect("index tokens data");
            assert_eq!(output.len(), 0);
        }

        #[test]
        fn index_tokens_data_should_return_expected_index_with_populated_data() {
            let (_dir, db) = setup_db();
            let tokens_index = setup_base_tokens(&db);
            let output = SledStorage::index_tokens_data(&db).expect("index tokens data");
            assert_eq!(output, tokens_index);
            assert_eq!(output.len(), tokens_index.len());
            assert_eq!(output.len(), 3);
        }

        #[test]
        fn index_ns_data_should_return_empty_index_with_no_data() {
            let (_dir, db) = setup_db();
            let output = SledStorage::index_ns_data(&db).expect("index ns data");
            assert_eq!(output.len(), 0);
        }

        #[test]
        fn index_ns_data_should_return_expected_index_with_populated_data() {
            let (_dir, db) = setup_db();
            let ns_index = setup_base_namespaces(&db);
            let output = SledStorage::index_ns_data(&db).expect("index ns data");
            assert_eq!(output, ns_index);
            assert_eq!(output.len(), ns_index.len());
            assert_eq!(output.len(), 3);
        }

        #[test]
        fn index_endpoints_data_should_return_empty_index_with_no_data() {
            let (_dir, db) = setup_db();
            let output = SledStorage::index_endpoints_data(&db).expect("index endpoints data");
            assert_eq!(output.len(), 0);
        }

        #[test]
        fn index_endpoints_data_should_return_expected_index_with_populated_data() {
            let (_dir, db) = setup_db();
            let endpoints_index = setup_base_endpoints(&db);
            let output = SledStorage::index_endpoints_data(&db).expect("index endpoints data");
            assert_eq!(output, endpoints_index);
            assert_eq!(output.len(), endpoints_index.len());
            assert_eq!(output.len(), 3);
        }

        #[test]
        fn index_pipelines_data_should_return_empty_index_with_no_data() {
            let (_dir, db) = setup_db();
            let output = SledStorage::index_pipelines_data(&db).expect("index pipelines data");
            assert_eq!(output.len(), 0);
        }

        #[test]
        fn index_pipelines_data_should_return_expected_index_with_populated_data() {
            let (_dir, db) = setup_db();
            let pipelines_index = setup_base_pipelines(&db);
            let output = SledStorage::index_pipelines_data(&db).expect("index pipelines data");
            assert_eq!(output, pipelines_index);
            assert_eq!(output.len(), pipelines_index.len());
            assert_eq!(output.len(), 3);
        }

        #[test]
        fn index_streams_data_should_return_empty_index_with_no_data() {
            let (_dir, db) = setup_db();
            let streams_tree = db.open_tree(OBJECTS_STREAMS).expect("open streams path prefix");
            let output = SledStorage::index_streams_data(&db, &streams_tree).expect("index streams data");
            assert_eq!(output.len(), 0);
        }

        #[test]
        fn index_streams_data_should_return_expected_index_with_populated_data() {
            let (_dir, db) = setup_db();
            let streams_tree = db.open_tree(OBJECTS_STREAMS).expect("open streams path prefix");
            let streams_index = setup_base_streams(&db, &streams_tree);
            let output = SledStorage::index_streams_data(&db, &streams_tree).expect("index streams data");
            assert_eq!(output, streams_index);
            assert_eq!(output.len(), streams_index.len());
            assert_eq!(output.len(), 3);
        }

        #[test]
        fn stream_keyspace_returns_expected_value() {
            let (ns, name) = ("default", "slipstream");
            let expected = format!("/streams/default/slipstream/");
            let output = SledStorage::stream_keyspace(ns, name);
            assert_eq!(expected, output);
        }

        //////////////////////////////////////////////////////////////////////
        // Handle AppendLogEntry /////////////////////////////////////////////

        #[test]
        fn handle_append_log_entry() {
            let mut sys = System::builder().name("test").stop_on_panic(true).build();
            let dir = tempfile::tempdir_in("/tmp").expect("new temp dir");
            let db_path = dir.path().join("db").to_string_lossy().to_string();
            let storage = SledStorage::new(&db_path).expect("instantiate storage");
            let log = storage.log();
            let storage_addr = storage.start();
            let entry = Entry{term: 20, index: 99999, entry_type: EntryType::Normal(EntryNormal{data: Some(AppData::from(api::PubStreamRequest{
                namespace: String::from("default"), name: String::from("events"), id: None, data: vec![],
            }))})};
            let msg = RgAppendLogEntry::new(Arc::new(entry.clone()));

            let f = storage_addr.send(msg).map_err(|err| panic!(err)).and_then(|res| res).map_err(|err| panic!(err));
            sys.block_on(f).expect("sys run");

            // Ensure the expected data was written to disk.
            let entries: Vec<_> = log.iter()
                .map(|res| res.expect("iter log entry"))
                .map(|(_, raw)| bincode::deserialize::<RgEntry>(&raw).expect("deserialize entry"))
                .collect();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].index, entry.index);
            assert_eq!(entries[0].term, entry.term);
            match &entries[0].entry_type {
                EntryType::Normal(entry) => match &entry.data {
                    Some(AppData::PubStream(data)) => {
                        assert_eq!(data.namespace.as_str(), "default");
                        assert_eq!(data.name.as_str(), "events");
                        assert_eq!(&data.id, &None);
                        assert_eq!(data.data.len(), 0);
                    }
                    _ => panic!("expected a populated PubStreamRequest entry"),
                }
                _ => panic!("unexpected entry type"),
            }
        }

        //////////////////////////////////////////////////////////////////////
        // Handle ReplicateLogEntries ////////////////////////////////////////

        #[test]
        fn handle_get_log_entries() {
            let mut sys = System::builder().name("test").stop_on_panic(true).build();
            let dir = tempfile::tempdir_in("/tmp").expect("new temp dir");
            let db_path = dir.path().join("db").to_string_lossy().to_string();
            let storage = SledStorage::new(&db_path).expect("instantiate storage");
            let log = storage.log();
            let storage_addr = storage.start();
            let entry0 = Entry{term: 1, index: 0, entry_type: EntryType::Normal(EntryNormal{data: Some(AppData::from(api::PubStreamRequest{
                namespace: String::from("default"), name: String::from("events0"), id: None, data: vec![],
            }))})};
            log.insert(entry0.index.to_be_bytes(), bincode::serialize(&entry0).expect("serialize entry")).expect("append to log");
            let entry1 = Entry{term: 1, index: 1, entry_type: EntryType::Normal(EntryNormal{data: Some(AppData::from(api::PubStreamRequest{
                namespace: String::from("default"), name: String::from("events1"), id: None, data: vec![],
            }))})};
            log.insert(entry1.index.to_be_bytes(), bincode::serialize(&entry1).expect("serialize entry")).expect("append to log");
            let msg = RgGetLogEntries::new(0, 500);

            let f = storage_addr.send(msg).map_err(|err| panic!(err)).and_then(|res| res).map_err(|err| panic!(err))
                .map(|entries| {
                    assert_eq!(entries.len(), 2);
                    assert_eq!(entries[0].index, 0);
                    assert_eq!(entries[1].index, 1);
                    assert_eq!(entries[0].term, 1);
                    assert_eq!(entries[1].term, 1);
                });
            sys.block_on(f).expect("sys run");
        }

        //////////////////////////////////////////////////////////////////////
        // Handle ReplicateLogEntries ////////////////////////////////////////

        #[test]
        fn handle_replicate_log_entries() {
            let mut sys = System::builder().name("test").stop_on_panic(true).build();
            let dir = tempfile::tempdir_in("/tmp").expect("new temp dir");
            let db_path = dir.path().join("db").to_string_lossy().to_string();
            let storage = SledStorage::new(&db_path).expect("instantiate storage");
            let log = storage.log();
            let storage_addr = storage.start();
            let msg0 = Entry{term: 1, index: 0, entry_type: EntryType::Normal(EntryNormal{data: Some(AppData::from(api::PubStreamRequest{
                namespace: String::from("default"), name: String::from("events0"), id: None, data: vec![],
            }))})};
            let msg1 = Entry{term: 1, index: 1, entry_type: EntryType::Normal(EntryNormal{data: Some(AppData::from(api::PubStreamRequest{
                namespace: String::from("default"), name: String::from("events1"), id: None, data: vec![],
            }))})};
            let msg = RgReplicateLogEntries::new(Arc::new(vec![msg0.clone(), msg1.clone()]));

            let f = storage_addr.send(msg).map_err(|err| panic!(err)).and_then(|res| res).map_err(|err| panic!(err));
            sys.block_on(f).expect("sys run");

            // Ensure the expected data was written to disk.
            let entries: Vec<_> = log.iter()
                .map(|res| res.expect("iter log entry"))
                .map(|(_, raw)| bincode::deserialize::<RgEntry>(&raw).expect("deserialize entry"))
                .collect();
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].index, msg0.index);
            assert_eq!(entries[0].term, msg0.term);
            match &entries[0].entry_type {
                EntryType::Normal(entry) => match &entry.data {
                    Some(AppData::PubStream(data)) => {
                        assert_eq!(data.namespace.as_str(), "default");
                        assert_eq!(data.name.as_str(), "events0");
                        assert_eq!(&data.id, &None);
                        assert_eq!(data.data.len(), 0);
                    }
                    _ => panic!("expected a populated PubStreamRequest entry"),
                }
                _ => panic!("unexpected entry type"),
            }
            assert_eq!(entries[1].index, msg1.index);
            assert_eq!(entries[1].term, msg1.term);
            match &entries[1].entry_type {
                EntryType::Normal(entry) => match &entry.data {
                    Some(AppData::PubStream(data)) => {
                        assert_eq!(data.namespace.as_str(), "default");
                        assert_eq!(data.name.as_str(), "events1");
                        assert_eq!(&data.id, &None);
                        assert_eq!(data.data.len(), 0);
                    }
                    _ => panic!("expected a populated PubStreamRequest entry"),
                }
                _ => panic!("unexpected entry type"),
            }
        }

        //////////////////////////////////////////////////////////////////////
        // Handle SaveHardState //////////////////////////////////////////////

        #[test]
        fn handle_save_hard_state() {
            let mut sys = System::builder().name("test").stop_on_panic(true).build();
            let dir = tempfile::tempdir_in("/tmp").expect("new temp dir");
            let db_path = dir.path().join("db").to_string_lossy().to_string();
            let storage = SledStorage::new(&db_path).expect("instantiate storage");
            let db = storage.db();
            let storage_addr = storage.start();
            let orig_hs = HardState{
                current_term: 666, voted_for: Some(6),
                membership: MembershipConfig{is_in_joint_consensus: false, members: vec![6], non_voters: Vec::new(), removing: Vec::new()},
            };
            let msg = RgSaveHardState::new(orig_hs.clone());

            let f = storage_addr.send(msg).map_err(|err| panic!(err)).and_then(|res| res).map_err(|err| panic!(err));
            sys.block_on(f).expect("sys run");

            // Ensure the expected data was written to disk.
            let raw_hs = db.get(RAFT_HARDSTATE_KEY).expect("get hardstate from disk").expect("hardstate value should exist");
            let hs: HardState = bincode::deserialize(&raw_hs).expect("deserialize hardstate");
            assert_eq!(orig_hs.current_term, hs.current_term);
            assert_eq!(orig_hs.voted_for, hs.voted_for);
            assert_eq!(orig_hs.membership, hs.membership);
        }
    }

    //////////////////////////////////////////////////////////////////////////
    // Fixtures //////////////////////////////////////////////////////////////

    fn setup_db() -> (tempfile::TempDir, sled::Db) {
        let dir = tempfile::tempdir_in("/tmp").expect("new temp dir");
        let dbpath = dir.path().join("db");
        let tree = sled::Db::open(&dbpath).expect("open database");
        (dir, tree)
    }

    fn setup_base_users(db: &sled::Db) -> BTreeMap<String, User> {
        let mut index = BTreeMap::new();
        let user0 = User{name: String::from("root-0"), role: UserRole::Root};
        let user1 = User{name: String::from("admin-0"), role: UserRole::Admin{namespaces: vec![String::from("default")]}};
        let user2 = User{name: String::from("viewer-0"), role: UserRole::Viewer{namespaces: vec![String::from("default")], metrics: true}};
        index.insert(user0.name.clone(), user0);
        index.insert(user1.name.clone(), user1);
        index.insert(user2.name.clone(), user2);

        for user in index.values() {
            let user_bytes = bincode::serialize(&user).expect("serialize user");
            db.insert(&user.name, user_bytes.as_slice()).expect("write user to disk");
        }

        index
    }

    fn setup_base_tokens(db: &sled::Db) -> BTreeSet<String> {
        let mut index = BTreeSet::new();
        index.insert(uuid::Uuid::new_v4().to_string());
        index.insert(uuid::Uuid::new_v4().to_string());
        index.insert(uuid::Uuid::new_v4().to_string());

        for token in index.iter() {
            db.insert(token.as_bytes(), token.as_bytes()).expect("write token to disk");
        }

        index
    }

    fn setup_base_namespaces(db: &sled::Db) -> BTreeSet<String> {
        let mut index = BTreeSet::new();
        index.insert(String::from("default"));
        index.insert(String::from("railgun"));
        index.insert(String::from("rg"));

        for ns in index.iter() {
            db.insert(ns.as_bytes(), ns.as_bytes()).expect("write namespace to disk");
        }

        index
    }

    fn setup_base_endpoints(db: &sled::Db) -> BTreeSet<(String, String)> {
        let mut index = BTreeSet::new();
        let ns = String::from("identity-service");
        index.insert((ns.clone(), String::from("sign-up")));
        index.insert((ns.clone(), String::from("login")));
        index.insert((ns.clone(), String::from("reset-password")));

        for (val_ns, endpoint) in index.iter() {
            let key = format!("{}/{}", val_ns, endpoint);
            let keybytes = key.as_bytes();
            db.insert(keybytes, keybytes).expect("write endpoint to disk");
        }

        index
    }

    fn setup_base_pipelines(db: &sled::Db) -> BTreeMap<(String, String), Pipeline> {
        let mut index = BTreeMap::new();
        let ns = String::from("identity-service");
        let pipe0 = Pipeline{namespace: ns.clone(), name: String::from("sign-up")};
        let pipe1 = Pipeline{namespace: ns.clone(), name: String::from("login")};
        let pipe2 = Pipeline{namespace: ns.clone(), name: String::from("reset-password")};
        index.insert((pipe0.namespace.clone(), pipe0.name.clone()), pipe0);
        index.insert((pipe1.namespace.clone(), pipe1.name.clone()), pipe1);
        index.insert((pipe2.namespace.clone(), pipe2.name.clone()), pipe2);

        for (_, pipeline) in index.iter() {
            let pipebytes = bincode::serialize(pipeline).expect("serialize pipeline");
            let key = format!("{}/{}", pipeline.namespace, pipeline.name);
            let keybytes = key.as_bytes();
            db.insert(keybytes, pipebytes).expect("write pipeline to disk");
        }

        index
    }

    fn setup_base_streams(db: &sled::Db, tree: &sled::Tree) -> BTreeMap<(String, String), Stream> {
        let mut index = BTreeMap::new();
        let stream_name = String::from("events");

        // Setup some indexed IDs.
        let mut indexed_ids = BTreeSet::new();
        indexed_ids.insert(String::from("testing"));

        let stream0 = Stream{namespace: String::from("identity-service"), name: stream_name.clone(), stream_type: StreamType::Standard, visibility: StreamVisibility::Namespace};
        let stream1 = Stream{namespace: String::from("projects-service"), name: stream_name.clone(), stream_type: StreamType::Standard, visibility: StreamVisibility::Private(String::from("pipelineX"))};
        let stream2 = Stream{namespace: String::from("billing-service"), name: stream_name.clone(), stream_type: StreamType::UniqueId{index: indexed_ids}, visibility: StreamVisibility::Namespace};
        index.insert((stream0.namespace.clone(), stream0.name.clone()), stream0);
        index.insert((stream1.namespace.clone(), stream1.name.clone()), stream1);
        index.insert((stream2.namespace.clone(), stream2.name.clone()), stream2);

        for (_, stream) in index.iter() {
            let streambytes = bincode::serialize(stream).expect("serialize stream");
            let key = format!("{}/{}", stream.namespace, stream.name);
            let keybytes = key.as_bytes();
            tree.insert(keybytes, streambytes).expect("write stream to disk");
        }

        // Write a single entry to stream "billing-service/events" for testing.
        let keyspace = SledStorage::stream_keyspace("billing-service", &stream_name);
        let entry_bytes = bincode::serialize(&StreamEntry{id: Some(String::from("testing")), data: vec![]}).expect("serialize stream entry");
        db.open_tree(&keyspace).expect("open stream keyspace")
            .insert(format!("{}{}", keyspace, "0").as_bytes(), entry_bytes).expect("write stream entry");

        index
    }
}
