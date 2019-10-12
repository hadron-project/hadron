//! A module encapsulating all logic for interfacing with the data storage system.

use std::{
    collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Arc,
};

use actix::prelude::*;
use actix_raft::{
    RaftStorage,
    messages::{EntryConfigChange, MembershipConfig},
    storage::{
        CreateSnapshot,
        CurrentSnapshotData,
        GetCurrentSnapshot,
        GetInitialState,
        GetLogEntries,
        HardState,
        InitialState,
        InstallSnapshot,
        AppendEntryToLog,
        ReplicateToLog,
        ApplyEntryToStateMachine,
        ReplicateToStateMachine,
        SaveHardState,
    },
};
use bincode;
use failure;
use log;
use sled::{self, Transactional};
use uuid;

use crate::{
    NodeId,
    app::{AppData, AppDataResponse, RgEntry, RgEntryNormal, RgEntryPayload},
    auth::User,
    db::models::{
        STREAM_NAME_PATTERN,
        Pipeline, Stream, StreamEntry, StreamType, StreamVisibility, StreamWrapper,
    },
    proto::client::api::{self, ClientError},
    utils,
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
/// The key used for tracking the next index for the next entry to be written to the respective stream.
const STREAM_NEXT_INDEX_KEY: &str = "index";

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
/// An error from serializing a Stream record.
const ERR_SERIALIZE_STREAM: &str = "Failed to serialize Stream for storage.";
// /// An error from deserializing a Stream Entry record.
// const ERR_DESERIALIZE_STREAM_ENTRY: &str = "Failed to deserialize Stream Entry from storage. Data is corrupt.";
/// An error from serializing a hard state record.
const ERR_SERIALIZE_HARD_STATE: &str = "Error while serializing Raft HardState object.";
/// An error from writing a hard state record to disk.
const ERR_WRITING_HARD_STATE: &str = "Error while writing Raft HardState to disk.";
/// An error while sending messages between the async/sync DB actors.
const ERR_DURING_DB_MSG: &str = "Actix MailboxError during messaging betweenn DB actors.";
/// An initialization error where an entry was expected, but none was found.
const ERR_MISSING_ENTRY: &str = "Unable to access expected log entry.";
/// An error from an endpoint's index being malformed.
const ERR_MALFORMED_ENDPOINT_INDEX: &str = "Malformed index for endpoint.";

type RgAppendEntryToLog = AppendEntryToLog<AppData, ClientError>;
type RgApplyEntryToStateMachine = ApplyEntryToStateMachine<AppData, AppDataResponse, ClientError>;
type RgCreateSnapshot = CreateSnapshot<ClientError>;
type RgGetCurrentSnapshot = GetCurrentSnapshot<ClientError>;
type RgGetInitialState = GetInitialState<ClientError>;
type RgGetLogEntries = GetLogEntries<AppData, ClientError>;
type RgInstallSnapshot = InstallSnapshot<ClientError>;
type RgReplicateToLog = ReplicateToLog<AppData, ClientError>;
type RgReplicateToStateMachine = ReplicateToStateMachine<AppData, ClientError>;
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
}

impl SledStorage {
    /// Create a new instance.
    ///
    /// This will initialize the data store, and will ensure that the database has a node ID.
    pub fn new(db_path: &str) -> Result<Self, failure::Error> {
        log::info!("Initializing storage engine SledStorage.");
        let db = sled::Db::open(db_path)?;
        let mut hasher = DefaultHasher::default();
        let id: u64 = match db.get(NODE_ID_KEY)? {
            Some(id) => u64_from_be_bytes(id)?,
            None => {
                uuid::Uuid::new_v4().hash(&mut hasher);
                let id: u64 = hasher.finish();
                db.insert(NODE_ID_KEY, id.to_be_bytes().as_ref())?;
                id
            }
        };
        log::debug!("Node ID: {}", id);

        // Initialize and restore any previous state from disk.
        let log = db.open_tree(RAFT_LOG_PREFIX)?;
        let state = Self::initialize(id, &db, &log)?;

        // Build indices.
        log::info!("Indexing data.");
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
        log::info!("Finished indexing data.");

        let sled = SyncArbiter::start(3, move || SledActor); // TODO: probably use `num_cores` crate.
        let mut inst = SledStorage{
            id, sled, db, log,
            hs: state.hard_state, object_collections,
            last_log_index: state.last_log_index,
            last_log_term: state.last_log_term,
            last_applied_log: state.last_applied_log,
            indexed_endpoints, indexed_namespaces,
            indexed_pipelines, indexed_streams,
            indexed_tokens, indexed_users,
            pending_streams: Default::default(),
        };
        inst.index_unapplied_logs(state.last_log_index, state.last_applied_log)?;
        Ok(inst)
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
            log::error!("{} {:?}", ERR_DESERIALIZE_ENTRY, err);
            failure::err_msg(ERR_DESERIALIZE_ENTRY)
        })?;
        let (last_log_index, last_log_term) = (entry.index, entry.term);

        // Check for the last applied log.
        let mut last_applied_log = 0;
        if let Some(idx_raw) = db.get(RAFT_LAL_KEY)? {
            last_applied_log = u64_from_be_bytes(idx_raw)?;
        }

        // Check for hard state.
        let hard_state = if let Some(hs_raw) = db.get(RAFT_HARDSTATE_KEY)? {
            bincode::deserialize::<HardState>(&hs_raw).map_err(|err| {
                log::error!("{} {:?}", ERR_DESERIALIZE_HS, err);
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
                log::error!("{} {}", ERR_DESERIALIZE_USER, err);
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
    fn index_endpoints_data(coll: &sled::Tree) -> Result<BTreeSet<String>, failure::Error> {
        let mut index = BTreeSet::new();
        for res in coll.iter() {
            let (_, endpoint) = res?;
            let val = String::from_utf8(endpoint.to_vec())?;
            let parts = val.splitn(2, "/");
            if parts.count() != 2 {
                return Err(failure::err_msg(ERR_MALFORMED_ENDPOINT_INDEX));
            }
            index.insert(val);
        }
        Ok(index)
    }

    /// Index pipelines data.
    fn index_pipelines_data(coll: &sled::Tree) -> Result<BTreeMap<String, Pipeline>, failure::Error> {
        let mut index = BTreeMap::new();
        for res in coll.iter() {
            let (_, model_raw) = res?;
            let pipeline: Pipeline = bincode::deserialize(&model_raw).map_err(|err| {
                log::error!("{} {}", ERR_DESERIALIZE_PIPELINE, err);
                failure::err_msg(ERR_DESERIALIZE_PIPELINE)
            })?;
            index.insert(format!("{}/{}", &pipeline.namespace, &pipeline.name), pipeline);
        }
        Ok(index)
    }

    /// Index streams data.
    fn index_streams_data(db: &sled::Db, coll: &sled::Tree) -> Result<BTreeMap<String, StreamWrapper>, failure::Error> {
        let mut index = BTreeMap::new();
        for res in coll.iter() {
            let (_, model_raw) = res?;
            let stream: Stream = bincode::deserialize(&model_raw).map_err(|err| {
                log::error!("{} {}", ERR_DESERIALIZE_STREAM, err);
                failure::err_msg(ERR_DESERIALIZE_STREAM)
            })?;

            // Open stream data & metadata handles.
            let keyspace_data = SledStorage::stream_keyspace_data(&stream.namespace, &stream.name);
            let keyspace_meta = SledStorage::stream_keyspace_metadata(&stream.namespace, &stream.name);
            let data_tree = db.open_tree(&keyspace_data)?;
            let meta_tree = db.open_tree(&keyspace_meta)?;

            // Extract the stream's last written index, else 0.
            let next_index = match meta_tree.get(STREAM_NEXT_INDEX_KEY)? {
                Some(bin) => u64_from_be_bytes(bin)?,
                None => 0,
            };

            // Build stream wrapper.
            let key = format!("{}/{}", &stream.namespace, &stream.name);
            let stream_wrapper = StreamWrapper{stream, next_index, data: data_tree, meta: meta_tree};

            // // If the stream has a unique ID constraint, then index its IDs.
            // // TODO: we can parallelize this pretty aggressively with Rayon. See #26.
            // if let StreamType::UniqueId{index: indexed_ids} = &mut stream.stream_type {
            //     let keyspace = SledStorage::stream_keyspace(&stream.namespace, &stream.name);
            //     for res in db.open_tree(keyspace)?.iter() {
            //         let (_, rawmodel) = res?;
            //         let entry: StreamEntry = bincode::deserialize(&rawmodel).map_err(|err| {
            //             log::error!("{} {}", ERR_DESERIALIZE_STREAM_ENTRY, err);
            //             failure::err_msg(ERR_DESERIALIZE_STREAM_ENTRY)
            //         })?;
            //         if let Some(id) = entry.id {
            //             indexed_ids.insert(id);
            //         }
            //     }
            // }

            index.insert(key, stream_wrapper);
        }
        Ok(index)
    }

    fn index_unapplied_logs(&mut self, last_log: u64, last_applied: u64) -> Result<(), failure::Error> {
        // Basic checks to avoid noop & panic conditions (though last_applied will never be > last_log).
        if last_applied >= last_log {
            return Ok(());
        }

        // Perform validation/indexing logic on unapplied logs. These should never fail as they
        // have already been previously validated, this will simply ensure their data is indexed.
        let (start, stop) = (last_applied.to_be_bytes(), last_log.to_be_bytes());
        self.log.range(start..=stop).try_for_each(|res_entry| {
            let (_, data) = res_entry?;
            bincode::deserialize::<RgEntry>(&data)
                .map_err(|err| {
                    log::error!("{} {}", ERR_DESERIALIZE_ENTRY, err);
                    ClientError::new_internal()
                })
                .and_then(|entry| match &entry.payload {
                    RgEntryPayload::Blank
                        | RgEntryPayload::SnapshotPointer(_)
                        | RgEntryPayload::ConfigChange(_) => Ok(()),
                    RgEntryPayload::Normal(inner_entry) => self.validate_pre_log_entry(inner_entry, entry.index),
                })?;
            Ok(())
        })
    }

    /// Get the keyspace for a stream's data given its namespace & name.
    fn stream_keyspace_data(ns: &str, name: &str) -> String {
        format!("{}/{}/{}/data", STREAMS_DATA_PREFIX, ns, name)
    }

    /// Get the keyspace for a stream's metadata given its namespace & name.
    fn stream_keyspace_metadata(ns: &str, name: &str) -> String {
        format!("{}/{}/{}/metadata", STREAMS_DATA_PREFIX, ns, name)
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

impl RaftStorage<AppData, AppDataResponse, ClientError> for SledStorage {
    type Actor = Self;
    type Context = Context<Self>;
}

//////////////////////////////////////////////////////////////////////////////
// RaftStorage RgAppendEntryToLog ////////////////////////////////////////////

impl Handler<RgAppendEntryToLog> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, msg: RgAppendEntryToLog, _ctx: &mut Self::Context) -> Self::Result {
        // Validate the given payload before writing it to the log.
        match &msg.entry.payload {
            RgEntryPayload::SnapshotPointer(_) => {
                log::error!("Received a request to write a snapshot pointer to the log. This should never happen.");
                return Box::new(fut::err(ClientError::new_internal()));
            }
            RgEntryPayload::Blank | RgEntryPayload::ConfigChange(_) => (), // No validation needed on these variants.
            RgEntryPayload::Normal(entry) => if let Err(err) = self.validate_pre_log_entry(entry, msg.entry.index) {
                return Box::new(fut::err(err));
            }
        }

        // Entry checks out, send it over to be written to the log.
        let res = bincode::serialize(&*msg.entry).map_err(|err| {
            log::error!("Error serializing log entry: {}", err);
            ClientError::new_internal()
        }).and_then(|data| {
            self.log.insert(msg.entry.index.to_be_bytes(), data.as_slice()).map_err(|err| {
                log::error!("Error serializing log entry: {}", err);
                ClientError::new_internal()
            })
        }).map(|_| {
            self.last_log_index = msg.entry.index;
            self.last_log_term = msg.entry.term;
        });
        Box::new(fut::result(res))
    }
}

impl SledStorage {
    /// Validate the contents of the given entry before writing it to the log.
    fn validate_pre_log_entry(&mut self, entry: &RgEntryNormal, index: u64) -> Result<(), ClientError> {
        match &entry.data {
            AppData::PubStream(data) => self.validate_pub_stream(data),
            AppData::SubStream(data) => self.validate_sub_stream(data),
            AppData::SubPipeline(data) => self.validate_sub_pipeline(data),
            AppData::UnsubStream(data) => self.validate_unsub_stream(data),
            AppData::UnsubPipeline(data) => self.validate_unsub_pipeline(data),
            AppData::EnsureRpcEndpoint(data) => self.validate_ensure_rpc_endpoint(data),
            AppData::EnsureStream(data) => self.validate_ensure_stream(data, index),
            AppData::EnsurePipeline(data) => self.validate_ensure_pipeline(data),
            AppData::AckStream(data) => self.validate_ack_stream(data),
            AppData::AckPipeline(data) => self.validate_ack_pipeline(data),
        }
    }

    /// Validate the contents of a PubStreamRequest before it hits the log.
    fn validate_pub_stream(&self, entry: &api::PubStreamRequest) -> Result<(), ClientError> {
        // Ensure the target stream exists.
        log::debug!("Validating pub stream request. {:?}", entry);
        if let None = self.indexed_streams.get(&format!("{}/{}", &entry.namespace, &entry.stream)) {
            return Err(ClientError::new_unknown_stream(&entry.namespace, &entry.stream));
        }
        Ok(())
    }

    /// Validate the contents of a SubStreamRequest before it hits the log.
    fn validate_sub_stream(&self, _entry: &api::SubStreamRequest) -> Result<(), ClientError> {
        Ok(())
    }

    /// Validate the contents of a SubPipelineRequest before it hits the log.
    fn validate_sub_pipeline(&self, _entry: &api::SubPipelineRequest) -> Result<(), ClientError> {
        Ok(())
    }

    /// Validate the contents of a UnsubStreamRequest before it hits the log.
    fn validate_unsub_stream(&self, _entry: &api::UnsubStreamRequest) -> Result<(), ClientError> {
        Ok(())
    }

    /// Validate the contents of a UnsubPipelineRequest before it hits the log.
    fn validate_unsub_pipeline(&self, _entry: &api::UnsubPipelineRequest) -> Result<(), ClientError> {
        Ok(())
    }

    /// Validate the contents of a EnsureRpcEndpointRequest before it hits the log.
    fn validate_ensure_rpc_endpoint(&self, _entry: &api::EnsureRpcEndpointRequest) -> Result<(), ClientError> {
        Ok(())
    }

    /// Validate the contents of a EnsureStreamRequest before it hits the log.
    fn validate_ensure_stream(&mut self, entry: &api::EnsureStreamRequest, index: u64) -> Result<(), ClientError> {
        log::debug!("Validating ensure stream request. {:?}", entry);
        if !STREAM_NAME_PATTERN.is_match(&entry.name) {
            return Err(ClientError::new_invalid_input(String::from("Stream names must match the pattern `[-_.a-zA-Z0-9]{1,100}`.")));
        }
        let fullname = format!("{}/{}", &entry.namespace, &entry.name);
        if let Some(_) = self.indexed_streams.get(&fullname) {
            return Err(ClientError::new_stream_already_exists());
        }
        if self.pending_streams.values().any(|e| e == &fullname) {
            return Err(ClientError::new_stream_already_exists());
        }
        self.pending_streams.insert(index, fullname);
        Ok(())
    }

    /// Validate the contents of a EnsurePipelineRequest before it hits the log.
    fn validate_ensure_pipeline(&self, _entry: &api::EnsurePipelineRequest) -> Result<(), ClientError> {
        Ok(())
    }

    /// Validate the contents of a AckStreamRequest before it hits the log.
    fn validate_ack_stream(&self, _entry: &api::AckStreamRequest) -> Result<(), ClientError> {
        Ok(())
    }

    /// Validate the contents of a AckPipelineRequest before it hits the log.
    fn validate_ack_pipeline(&self, _entry: &api::AckPipelineRequest) -> Result<(), ClientError> {
        Ok(())
    }
}

//////////////////////////////////////////////////////////////////////////////
// RaftStorage RgReplicateToLog //////////////////////////////////////////////

impl Handler<RgReplicateToLog> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, msg: RgReplicateToLog, _ctx: &mut Self::Context) -> Self::Result {
        let mut batch = sled::Batch::default();
        let res = msg.entries.iter().try_for_each(|entry| {
            let data = bincode::serialize(entry).map_err(|err| {
                log::error!("Error serializing log entry: {}", err);
                ClientError::new_internal()
            })?;
            batch.insert(&entry.index.to_be_bytes(), data.as_slice());
            Ok(())
        }).and_then(|_| {
            self.log.apply_batch(batch).map_err(|err| {
                log::error!("Error applying batch of Raft log entries to storage: {}", err);
                ClientError::new_internal()
            })
        });
        Box::new(fut::result(res))
    }
}

//////////////////////////////////////////////////////////////////////////////
// RaftStorage RgApplyEntryToStateMachine & RgReplicateToStateMachine ////////

impl Handler<RgApplyEntryToStateMachine> for SledStorage {
    type Result = ResponseActFuture<Self, AppDataResponse, ClientError>;

    fn handle(&mut self, msg: RgApplyEntryToStateMachine, _ctx: &mut Self::Context) -> Self::Result {
        Box::new(fut::result(self.apply_entry_to_state_machine(&*msg.payload)))
    }
}

impl Handler<RgReplicateToStateMachine> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, msg: RgReplicateToStateMachine, _ctx: &mut Self::Context) -> Self::Result {
        let res = msg.payload.iter().try_for_each(|entry| {
            self.apply_entry_to_state_machine(entry).map(|_| ())
        });
        Box::new(fut::result(res))
    }
}

impl SledStorage {
    /// Apply the given entry to the state machine.
    fn apply_entry_to_state_machine(&mut self, entry: &RgEntry) -> Result<AppDataResponse, ClientError> {
        let res = match &entry.payload {
            RgEntryPayload::SnapshotPointer(_) => {
                log::error!("Received a request to write a snapshot pointer to state machine. This should never happen.");
                return Err(ClientError::new_internal());
            }
            RgEntryPayload::Blank => self.update_lal(entry.index).map(|_| AppDataResponse::Noop),
            RgEntryPayload::ConfigChange(config) => self.apply_entry_config_change(config, entry.index),
            RgEntryPayload::Normal(inner_entry) => match &inner_entry.data {
                AppData::PubStream(data) => self.apply_pub_stream(data, entry.index),
                // AppData::SubStream(data) => self.validate_sub_stream(data),
                // AppData::SubPipeline(data) => self.validate_sub_pipeline(data),
                // AppData::UnsubStream(data) => self.validate_unsub_stream(data),
                // AppData::UnsubPipeline(data) => self.validate_unsub_pipeline(data),
                // AppData::EnsureRpcEndpoint(data) => self.validate_ennsure_rpc_endpoint(data),
                AppData::EnsureStream(data) => self.apply_ensure_stream(data, entry.index),
                // AppData::EnsurePipeline(data) => self.validate_ensure_pipeline(data),
                // AppData::AckStream(data) => self.validate_ack_stream(data),
                // AppData::AckPipeline(data) => self.validate_ack_pipeline(data),
                _ => Ok(AppDataResponse::Noop),
            }
        };
        // If response was successful, update last applied log index (in mem only) & clear pending indexes.
        if res.is_ok() {
            self.last_applied_log = entry.index;
            self.pending_streams = self.pending_streams.split_off(&(entry.index + 1));
        }
        res
    }

    /// Apply the given PubStreamRequest to the state machine.
    fn apply_pub_stream(&mut self, req: &api::PubStreamRequest, log_index: u64) -> Result<AppDataResponse, ClientError> {
        log::debug!("Applying pub stream request for {:?}.", &req);

        // Get a handle to the target stream object.
        let stream = self.indexed_streams.get_mut(&format!("{}/{}", &req.namespace, &req.stream)).ok_or_else(|| {
            log::error!("Error while applying entry to state machine for a PubStreamRequest. Target stream does not exist in index.");
            ClientError::new_internal()
        })?;

        // Create a new stream entry and serialize it.
        let entry = StreamEntry{index: stream.next_index, data: req.payload.clone()};
        let entry_bytes = bincode::serialize(&entry).map_err(|err| {
            log::error!("Error serializing StreamEntry. {}", err);
            ClientError::new_internal()
        })?;

        // Update the value of the stream's next index.
        stream.next_index += 1;

        // Write the entry, the stream's next index value and the LAL transactionally.
        (&stream.data, &stream.meta, &self.db as &sled::Tree).transaction(|(data, meta, db)| {
            data.insert(&entry.index.to_be_bytes(), entry_bytes.as_slice())?;
            meta.insert(STREAM_NEXT_INDEX_KEY, &stream.next_index.to_be_bytes())?;
            db.insert(RAFT_LAL_KEY, &log_index.to_be_bytes())?;
            Ok(())
        }).map_err(|err| {
            log::error!("Error writing stream entry. {}", err);
            ClientError::new_internal()
        })?;

        Ok(AppDataResponse::PubStream{index: entry.index})
    }

    /// Apply the given EnsureStreamRequest to the state machine.
    ///
    /// When this routine starts, we know that the stream ns/name has been reserved in the
    /// `pending_streams` index, which will be cleared after this routine.
    fn apply_ensure_stream(&mut self, req: &api::EnsureStreamRequest, log_index: u64) -> Result<AppDataResponse, ClientError> {
        log::debug!("Applying ensure stream request for {:?}.", &req);

        // Open new trees for the new stream.
        let fullname = format!("{}/{}", req.namespace, req.name);
        let data = self.db.open_tree(Self::stream_keyspace_data(&req.namespace, &req.name)).map_err(|err| {
            log::error!("Error opening tree for new stream's data. {}", err);
            ClientError::new_internal()
        })?;
        let meta = self.db.open_tree(Self::stream_keyspace_metadata(&req.namespace, &req.name)).map_err(|err| {
            log::error!("Error opening tree for new stream's metadata. {}", err);
            ClientError::new_internal()
        })?;

        // Build stream object & serialize.
        let stream = Stream{
            namespace: req.namespace.clone(),
            name: req.name.clone(),
            stream_type: StreamType::Standard,
            visibility: StreamVisibility::Namespace,
        };
        let stream_bytes = bincode::serialize(&stream).map_err(|err| {
            log::error!("{} {}", ERR_SERIALIZE_STREAM, err);
            ClientError::new_internal()
        })?;

        // Initialize next index value for new stream & update LAL.
        (&meta, &self.object_collections.streams_collection, &self.db as &sled::Tree).transaction(|(meta, streams, db)| {
            streams.insert(fullname.as_bytes(), stream_bytes.as_slice())?;
            meta.insert(STREAM_NEXT_INDEX_KEY, &0u64.to_be_bytes())?;
            db.insert(RAFT_LAL_KEY, &log_index.to_be_bytes())?;
            Ok(())
        }).map_err(|err| {
            log::error!("Error creating new stream. {}", err);
            ClientError::new_internal()
        })?;

        // Add new stream to index.
        self.indexed_streams.insert(fullname, StreamWrapper{next_index: 0, stream, data, meta});
        Ok(AppDataResponse::EnsureStream)
    }

    /// Apply the new config to storage.
    fn apply_entry_config_change(&mut self, config: &EntryConfigChange, log_index: u64) -> Result<AppDataResponse, ClientError> {
        // Serialize data.
        self.hs = HardState{
            current_term: self.hs.current_term,
            voted_for: self.hs.voted_for.clone(),
            membership: config.membership.clone(),
        };
        let hs = bincode::serialize(&self.hs).map_err(|err| {
            log::error!("{} {:?}", ERR_SERIALIZE_HARD_STATE, err);
            ClientError::new_internal()
        })?;

        // Write to disk.
        self.db.transaction(|db| {
            db.insert(RAFT_HARDSTATE_KEY, hs.as_slice())?;
            db.insert(RAFT_LAL_KEY, &log_index.to_be_bytes())?;
            Ok(())
        }).map_err(|err| {
            log::error!("{} {:?}", ERR_WRITING_HARD_STATE, err);
            ClientError::new_internal()
        })?;

        Ok(AppDataResponse::Noop)
    }

    /// Update the "last applied log" value.
    ///
    /// This should only be used for routines which do not need to atomically update other values
    /// in storage.
    fn update_lal(&self, log_index: u64) -> Result<(), ClientError> {
        self.db.insert(RAFT_LAL_KEY, &log_index.to_be_bytes())
            .map_err(|err| {
                log::error!("Error updating last applied log value. {}", err);
                ClientError::new_internal()
            })
            .map(|_| ())
    }
}

//////////////////////////////////////////////////////////////////////////////
// RaftStorage RgGetInitialState /////////////////////////////////////////////

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
            .map_err(|err, _: &mut Self, _| utils::proto_client_error_from_mailbox_error(err, ERR_DURING_DB_MSG))
            .and_then(|res, _, _| fut::result(res))
        )
    }
}

impl Handler<RgSaveHardState> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, msg: RgSaveHardState, _ctx: &mut Self::Context) -> Self::Result {
        // Serialize data.
        self.hs = msg.hs.clone();
        let res = bincode::serialize(&msg.hs).map_err(|err| {
            log::error!("{} {:?}", ERR_SERIALIZE_HARD_STATE, err);
            ClientError::new_internal()
        })
        // Write to disk.
        .and_then(|hs| {
            self.db.insert(RAFT_HARDSTATE_KEY, hs).map_err(|err| {
                log::error!("{} {:?}", ERR_WRITING_HARD_STATE, err);
                ClientError::new_internal()
            })
            .map(|_| ())
        });
        Box::new(fut::result(res))
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

impl Actor for SledActor {
    type Context = SyncContext<Self>;
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
                log::error!("Error from sled in GetLogEntries. {}", err);
                ClientError::new_internal()
            })?;
            let entry = bincode::deserialize(&data).map_err(|err| {
                log::error!("Error deserializing entry in  GetLogEntries. {}", err);
                ClientError::new_internal()
            })?;
            acc.push(entry);
            Ok(acc)
        })?;
        Ok(entries)
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

//////////////////////////////////////////////////////////////////////////////////////////////////
// Tests /////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::UserRole,
        db::models::{StreamType, StreamVisibility},
        proto::client::api,
    };
    use actix_raft::messages::{Entry, EntryPayload, EntryNormal};
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
            assert_eq!(output.len(), streams_index.len());
            assert_eq!(output.len(), 2); // TODO: change this back to 3 once we have indexed streams.
            for (s0, s1) in output.values().zip(streams_index.values()) {
                assert_eq!(s0.stream, s1.stream);
            }
        }

        #[test]
        fn stream_keyspace_data_returns_expected_value() {
            let (ns, name) = ("default", "slipstream");
            let expected = format!("/streams/default/slipstream/data");
            let output = SledStorage::stream_keyspace_data(ns, name);
            assert_eq!(expected, output);
        }

        #[test]
        fn stream_keyspace_metadata_returns_expected_value() {
            let (ns, name) = ("default", "slipstream");
            let expected = format!("/streams/default/slipstream/metadata");
            let output = SledStorage::stream_keyspace_metadata(ns, name);
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
            let entry = Entry{term: 20, index: 99999, payload: EntryPayload::Normal(EntryNormal{data: AppData::from(api::PubStreamRequest{
                namespace: String::from("default"), stream: String::from("events"), payload: vec![],
            })})};
            let msg = RgAppendEntryToLog::new(Arc::new(entry.clone()));

            let f = storage_addr.send(msg).map_err(|err| panic!("MailboxError: {}", err)).and_then(|res| res).map_err(|err| panic!("ClientError {:?}", err));
            sys.block_on(f).expect("sys run");

            // Ensure the expected data was written to disk.
            let entries: Vec<_> = log.iter()
                .map(|res| res.expect("iter log entry"))
                .map(|(_, raw)| bincode::deserialize::<RgEntry>(&raw).expect("deserialize entry"))
                .collect();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].index, entry.index);
            assert_eq!(entries[0].term, entry.term);
            match &entries[0].payload {
                EntryPayload::Normal(entry) => match &entry.data {
                    AppData::PubStream(data) => {
                        assert_eq!(data.namespace.as_str(), "default");
                        assert_eq!(data.stream.as_str(), "events");
                        assert_eq!(data.payload.len(), 0);
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
            let entry0 = Entry{term: 1, index: 0, payload: EntryPayload::Normal(EntryNormal{data: AppData::from(api::PubStreamRequest{
                namespace: String::from("default"), stream: String::from("events0"), payload: vec![],
            })})};
            log.insert(entry0.index.to_be_bytes(), bincode::serialize(&entry0).expect("serialize entry")).expect("append to log");
            let entry1 = Entry{term: 1, index: 1, payload: EntryPayload::Normal(EntryNormal{data: AppData::from(api::PubStreamRequest{
                namespace: String::from("default"), stream: String::from("events1"), payload: vec![],
            })})};
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
            let msg0 = Entry{term: 1, index: 0, payload: EntryPayload::Normal(EntryNormal{data: AppData::from(api::PubStreamRequest{
                namespace: String::from("default"), stream: String::from("events0"), payload: vec![],
            })})};
            let msg1 = Entry{term: 1, index: 1, payload: EntryPayload::Normal(EntryNormal{data: AppData::from(api::PubStreamRequest{
                namespace: String::from("default"), stream: String::from("events1"), payload: vec![],
            })})};
            let msg = RgReplicateToLog::new(Arc::new(vec![msg0.clone(), msg1.clone()]));

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
            match &entries[0].payload {
                EntryPayload::Normal(entry) => match &entry.data {
                    AppData::PubStream(data) => {
                        assert_eq!(data.namespace.as_str(), "default");
                        assert_eq!(data.stream.as_str(), "events0");
                        assert_eq!(data.payload.len(), 0);
                    }
                    _ => panic!("expected a populated PubStreamRequest entry"),
                }
                _ => panic!("unexpected entry type"),
            }
            assert_eq!(entries[1].index, msg1.index);
            assert_eq!(entries[1].term, msg1.term);
            match &entries[1].payload {
                EntryPayload::Normal(entry) => match &entry.data {
                    AppData::PubStream(data) => {
                        assert_eq!(data.namespace.as_str(), "default");
                        assert_eq!(data.stream.as_str(), "events1");
                        assert_eq!(data.payload.len(), 0);
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

    fn setup_base_endpoints(db: &sled::Db) -> BTreeSet<String> {
        let mut index = BTreeSet::new();
        let ns = "identity-service";
        index.insert(format!("{}/{}", ns, "sign-up"));
        index.insert(format!("{}/{}", ns, "login"));
        index.insert(format!("{}/{}", ns, "reset-password"));

        for val in index.iter() {
            let keybytes = val.as_bytes();
            db.insert(keybytes, keybytes).expect("write endpoint to disk");
        }

        index
    }

    fn setup_base_pipelines(db: &sled::Db) -> BTreeMap<String, Pipeline> {
        let mut index = BTreeMap::new();
        let ns = String::from("identity-service");
        let pipe0 = Pipeline{namespace: ns.clone(), name: String::from("sign-up")};
        let pipe1 = Pipeline{namespace: ns.clone(), name: String::from("login")};
        let pipe2 = Pipeline{namespace: ns.clone(), name: String::from("reset-password")};
        index.insert(format!("{}/{}", &pipe0.namespace, &pipe0.name), pipe0);
        index.insert(format!("{}/{}", &pipe1.namespace, &pipe1.name), pipe1);
        index.insert(format!("{}/{}", &pipe2.namespace, &pipe2.name), pipe2);

        for (key, pipeline) in index.iter() {
            let pipebytes = bincode::serialize(pipeline).expect("serialize pipeline");
            let keybytes = key.as_bytes();
            db.insert(keybytes, pipebytes).expect("write pipeline to disk");
        }

        index
    }

    fn setup_base_streams(db: &sled::Db, tree: &sled::Tree) -> BTreeMap<String, StreamWrapper> {
        let mut index = BTreeMap::new();
        let stream_name = String::from("events");

        // Setup some indexed IDs.
        let mut indexed_ids = BTreeSet::new();
        indexed_ids.insert(String::from("testing"));

        let stream0 = Stream{namespace: String::from("identity-service"), name: stream_name.clone(), stream_type: StreamType::Standard, visibility: StreamVisibility::Namespace};
        let stream1 = Stream{namespace: String::from("projects-service"), name: stream_name.clone(), stream_type: StreamType::Standard, visibility: StreamVisibility::Private(String::from("pipelineX"))};
        // let stream2 = Stream{namespace: String::from("billing-service"), name: stream_name.clone(), stream_type: StreamType::UniqueId{index: indexed_ids}, visibility: StreamVisibility::Namespace};
        for stream in vec![stream0, stream1] {
            let keyspace_data = SledStorage::stream_keyspace_data(&stream.namespace, &stream.name);
            let keyspace_meta = SledStorage::stream_keyspace_metadata(&stream.namespace, &stream.name);
            let data_tree = db.open_tree(&keyspace_data).expect("open stream data keyspace");
            let meta_tree = db.open_tree(&keyspace_meta).expect("open stream metadata keyspace");
            index.insert(format!("{}/{}", &stream.namespace, &stream.name), StreamWrapper{stream, next_index: 0, data: data_tree, meta: meta_tree});
        }

        for (key, stream_wrapper) in index.iter() {
            let streambytes = bincode::serialize(&stream_wrapper.stream).expect("serialize stream");
            let keybytes = key.as_bytes();
            tree.insert(keybytes, streambytes).expect("write stream to disk");
        }

        // Write a single entry to stream "billing-service/events" for testing.
        let keyspace = SledStorage::stream_keyspace_data("billing-service", &stream_name);
        let entry_bytes = bincode::serialize(&StreamEntry{index: 0, data: vec![]}).expect("serialize stream entry");
        db.open_tree(&keyspace).expect("open stream keyspace").insert(0u64.to_be_bytes(), entry_bytes).expect("write stream entry");

        index
    }
}
