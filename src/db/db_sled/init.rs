//! Initialization and start-up logic.

use std::{
    collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Arc,
};

use actix::prelude::*;
use actix_raft::{
    messages::MembershipConfig,
    storage::{
        HardState,
        InitialState,
    },
};
use bincode;
use failure;
use log;
use sled;
use uuid;

use crate::{
    NodeId,
    app::{AppDataError, RgEntry, RgEntryPayload},
    auth::User,
    db::{
        db_sled::{
            SledStorage, ObjectCollections,
            consts,
            raft::{SledActor},
            u64_from_be_bytes,
        },
        models::{
            Pipeline, Stream, StreamWrapper,
        },
    },
};

impl SledStorage {
    /// Create a new instance.
    ///
    /// This will initialize the data store, and will ensure that the database has a node ID.
    pub fn new(db_path: &str) -> Result<Self, failure::Error> {
        log::info!("Initializing storage engine SledStorage.");
        let db = sled::Db::open(db_path)?;
        let mut hasher = DefaultHasher::default();
        let id: u64 = match db.get(consts::NODE_ID_KEY)? {
            Some(id) => u64_from_be_bytes(id)?,
            None => {
                uuid::Uuid::new_v4().hash(&mut hasher);
                let id: u64 = hasher.finish();
                db.insert(consts::NODE_ID_KEY, id.to_be_bytes().as_ref())?;
                id
            }
        };
        log::debug!("Node ID: {}", id);

        // Initialize and restore any previous state from disk.
        let log = db.open_tree(consts::RAFT_LOG_PREFIX)?;
        let state = Self::initialize(id, &db, &log)?;

        // Build indices.
        log::info!("Indexing data.");
        let users_collection = db.open_tree(consts::OBJECTS_USERS)?;
        let indexed_users = Self::index_users_data(&users_collection)?;
        let tokens_collection = db.open_tree(consts::OBJECTS_TOKENS)?;
        let indexed_tokens = Self::index_tokens_data(&tokens_collection)?;
        let ns_collection = db.open_tree(consts::OBJECTS_NS)?;
        let indexed_namespaces = Self::index_ns_data(&ns_collection)?;
        let endpoints_collection = db.open_tree(consts::OBJECTS_ENDPOINTS)?;
        let indexed_endpoints = Self::index_endpoints_data(&endpoints_collection)?;
        let pipelines_collection = db.open_tree(consts::OBJECTS_PIPELINES)?;
        let indexed_pipelines = Self::index_pipelines_data(&pipelines_collection)?;
        let streams_collection = db.open_tree(consts::OBJECTS_STREAMS)?;
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
    pub(super) fn initialize(id: NodeId, db: &sled::Db, log: &sled::Tree) -> Result<InitialState, failure::Error> {
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
        let (_k, val) = log.iter().rev().nth(0).ok_or(failure::err_msg(consts::ERR_MISSING_ENTRY))??;
        let entry: RgEntry = bincode::deserialize(&val).map_err(|err| {
            log::error!("{} {:?}", consts::ERR_DESERIALIZE_ENTRY, err);
            failure::err_msg(consts::ERR_DESERIALIZE_ENTRY)
        })?;
        let (last_log_index, last_log_term) = (entry.index, entry.term);

        // Check for the last applied log.
        let mut last_applied_log = 0;
        if let Some(idx_raw) = db.get(consts::RAFT_LAL_KEY)? {
            last_applied_log = u64_from_be_bytes(idx_raw)?;
        }

        // Check for hard state.
        let hard_state = if let Some(hs_raw) = db.get(consts::RAFT_HARDSTATE_KEY)? {
            bincode::deserialize::<HardState>(&hs_raw).map_err(|err| {
                log::error!("{} {:?}", consts::ERR_DESERIALIZE_HS, err);
                failure::err_msg(consts::ERR_DESERIALIZE_HS)
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
    pub(super) fn index_users_data(coll: &sled::Tree) -> Result<BTreeMap<String, User>, failure::Error> {
        let mut index = BTreeMap::new();
        for res in coll.iter() {
            let (_, model_raw) = res?;
            let user: User = bincode::deserialize(&model_raw).map_err(|err| {
                log::error!("{} {}", consts::ERR_DESERIALIZE_USER, err);
                failure::err_msg(consts::ERR_DESERIALIZE_USER)
            })?;
            index.insert(user.name.clone(), user);
        }
        Ok(index)
    }

    /// Index tokens data.
    pub(super) fn index_tokens_data(coll: &sled::Tree) -> Result<BTreeSet<String>, failure::Error> {
        let mut index = BTreeSet::new();
        for res in coll.iter() {
            let (_, token) = res?;
            let val = String::from_utf8(token.to_vec())?;
            index.insert(val);
        }
        Ok(index)
    }

    /// Index namespaces data.
    pub(super) fn index_ns_data(coll: &sled::Tree) -> Result<BTreeSet<String>, failure::Error> {
        let mut index = BTreeSet::new();
        for res in coll.iter() {
            let (_, ns) = res?;
            let val = String::from_utf8(ns.to_vec())?;
            index.insert(val);
        }
        Ok(index)
    }

    /// Index endpoints data.
    pub(super) fn index_endpoints_data(coll: &sled::Tree) -> Result<BTreeSet<String>, failure::Error> {
        let mut index = BTreeSet::new();
        for res in coll.iter() {
            let (_, endpoint) = res?;
            let val = String::from_utf8(endpoint.to_vec())?;
            let parts = val.splitn(2, "/");
            if parts.count() != 2 {
                return Err(failure::err_msg(consts::ERR_MALFORMED_ENDPOINT_INDEX));
            }
            index.insert(val);
        }
        Ok(index)
    }

    /// Index pipelines data.
    pub(super) fn index_pipelines_data(coll: &sled::Tree) -> Result<BTreeMap<String, Pipeline>, failure::Error> {
        let mut index = BTreeMap::new();
        for res in coll.iter() {
            let (_, model_raw) = res?;
            let pipeline: Pipeline = bincode::deserialize(&model_raw).map_err(|err| {
                log::error!("{} {}", consts::ERR_DESERIALIZE_PIPELINE, err);
                failure::err_msg(consts::ERR_DESERIALIZE_PIPELINE)
            })?;
            index.insert(format!("{}/{}", &pipeline.namespace, &pipeline.name), pipeline);
        }
        Ok(index)
    }

    /// Index streams data.
    pub(super) fn index_streams_data(db: &sled::Db, coll: &sled::Tree) -> Result<BTreeMap<String, StreamWrapper>, failure::Error> {
        let mut index = BTreeMap::new();
        for res in coll.iter() {
            let (_, model_raw) = res?;
            let stream: Stream = bincode::deserialize(&model_raw).map_err(|err| {
                log::error!("{} {}", consts::ERR_DESERIALIZE_STREAM, err);
                failure::err_msg(consts::ERR_DESERIALIZE_STREAM)
            })?;

            // Open stream data & metadata handles.
            let keyspace_data = SledStorage::stream_keyspace_data(&stream.namespace, &stream.name);
            let keyspace_meta = SledStorage::stream_keyspace_metadata(&stream.namespace, &stream.name);
            let data_tree = db.open_tree(&keyspace_data)?;
            let meta_tree = db.open_tree(&keyspace_meta)?;

            // Extract the stream's last written index, else 0.
            let next_index = match meta_tree.get(consts::STREAM_NEXT_INDEX_KEY)? {
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
            //             log::error!("{} {}", consts::ERR_DESERIALIZE_STREAM_ENTRY, err);
            //             failure::err_msg(consts::ERR_DESERIALIZE_STREAM_ENTRY)
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

    pub(super) fn index_unapplied_logs(&mut self, last_log: u64, last_applied: u64) -> Result<(), failure::Error> {
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
                    log::error!("{} {}", consts::ERR_DESERIALIZE_ENTRY, err);
                    AppDataError::Internal
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
}
