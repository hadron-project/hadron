//! A module encapsulating all logic for interfacing with the data storage system.

use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
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
use log::{error, info};
use sled;
use uuid;

use crate::{
    NodeId,
    auth::User,
    config::Config,
    db::{AppData},
    proto::client::api::{ClientError, ErrorCode},
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
const USERS_PATH_PREFIX: &str = "/users/";
/// The DB path prefix for all users.
const _TOKENS_PATH_PREFIX: &str = "/tokens/";
/// The DB path prefix for all namespaces.
const _NS_PATH_PREFIX: &str = "/ns/";
/// The DB path prefix for all streams.
const _STREAMS_PATH_PREFIX: &str = "/streams/";
/// The DB path prefix for all pipelines.
const _PIPELINES_PATH_PREFIX: &str = "/pipelines/";
/// An error from deserializing an entry.
const ERR_DESERIALIZE_ENTRY: &str = "Failed to deserialize entry from log. Data is corrupt.";
/// An error from deserializing HardState record.
const ERR_DESERIALIZE_HS: &str = "Failed to deserialize HardState from storage. Data is corrupt.";
/// An error from deserializing a User record.
const ERR_DESERIALIZE_USER: &str = "Failed to deserialize User from storage. Data is corrupt.";
/// An initialization error where an entry was expected, but none was found.
const ERR_MISSING_ENTRY: &str = "Unable to access expected log entry.";

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

/// An implementation of the Railgun storage engine using Sled.
pub struct SledStorage {
    /// The ID of this node.
    id: NodeId,
    /// The addr of the sled sync actor for DB operations.
    sled: Addr<SledActor>,
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
}

impl SledStorage {
    /// Create a new instance.
    ///
    /// This will initialize the data store, and will ensure that the database has a node ID.
    pub fn new(config: &Config) -> Result<Self, failure::Error> {
        info!("Initializing storage engine SledStorage.");
        let db = sled::Db::open(&config.db_path)?;
        let mut hasher = DefaultHasher::default();
        let id: u64 = match db.get(NODE_ID_KEY)? {
            Some(id) => {
                hasher.write(id.as_ref());
                hasher.finish()
            },
            None => {
                uuid::Uuid::new_v4().hash(&mut hasher);
                let id: u64 = hasher.finish();
                db.insert(NODE_ID_KEY, id.to_le_bytes().as_ref())?;
                id
            }
        };

        // Initialize and restore any previous state from disk.
        let log = db.open_tree(RAFT_LOG_PREFIX)?;
        let state = Self::initialize(id, &db, &log)?;

        // Build indices.
        // - index all tokens: these are just simple IDs.
        // - index all namespaces: just namespace names.
        // - index all endpoints: namespace/endpoint as key.
        // - index all pipelines: namespace/pipeline as key.
        // - index all streams: namespace/stream as key.
        //     - value will include all details of the stream, including indexed unique IDs if applicable.
        let users_collection = db.open_tree(USERS_PATH_PREFIX)?;
        let _users = Self::index_users_data(&users_collection)?;

        let sled = SyncArbiter::start(3, move || SledActor{db: db.clone(), log: log.clone()}); // TODO: probably use `num_cores` crate.
        Ok(SledStorage{
            id, sled, hs: state.hard_state,
            last_log_index: state.last_log_index,
            last_log_term: state.last_log_term,
            last_applied_log: state.last_applied_log,
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
        let mut users = BTreeMap::new();
        for res in coll.iter() {
            let (_, model_raw) = res?;
            let user: User = bincode::deserialize(&model_raw).map_err(|err| {
                error!("{} {}", ERR_DESERIALIZE_USER, err);
                failure::err_msg(ERR_DESERIALIZE_USER)
            })?;
            users.insert(user.name.clone(), user);
        }
        Ok(users)
    }

    /// This node's ID.
    pub fn node_id(&self) -> NodeId {
        self.id
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

    fn handle(&mut self, _msg: RgAppendLogEntry, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Box::new(fut::ok(()))
    }
}

impl Handler<RgApplyToStateMachine> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, _msg: RgApplyToStateMachine, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Box::new(fut::ok(()))
    }
}

impl Handler<RgCreateSnapshot> for SledStorage {
    type Result = ResponseActFuture<Self, CurrentSnapshotData, ClientError>;

    fn handle(&mut self, _msg: RgCreateSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Box::new(fut::err(ClientError{code: ErrorCode::Internal as i32, message: String::from("")}))
    }
}

impl Handler<RgGetCurrentSnapshot> for SledStorage {
    type Result = ResponseActFuture<Self, Option<CurrentSnapshotData>, ClientError>;

    fn handle(&mut self, _msg: RgGetCurrentSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Box::new(fut::ok(None))
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

    fn handle(&mut self, _msg: RgGetLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Box::new(fut::ok(vec![]))
    }
}

impl Handler<RgInstallSnapshot> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, _msg: RgInstallSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Box::new(fut::ok(()))
    }
}

impl Handler<RgReplicateLogEntries> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, _msg: RgReplicateLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Box::new(fut::ok(()))
    }
}

impl Handler<RgSaveHardState> for SledStorage {
    type Result = ResponseActFuture<Self, (), ClientError>;

    fn handle(&mut self, msg: RgSaveHardState, _ctx: &mut Self::Context) -> Self::Result {
        let hs = msg.hs.clone();
        Box::new(fut::wrap_future(self.sled.send(msg))
            .map_err(|_, _: &mut Self, _| ClientError::new_internal()) // TODO: log
            .and_then(|res, _, _| fut::result(res))
            .and_then(move |_, act, _| {
                act.hs = hs;
                fut::ok(())
            })
        )
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// SledActor /////////////////////////////////////////////////////////////////////////////////////

/// Sync actor for interfacing with Sled.
struct SledActor {
    db: sled::Db,
    log: Arc<sled::Tree>,
}

impl Actor for SledActor {
    type Context = SyncContext<Self>;
}

impl Handler<RgAppendLogEntry> for SledActor {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, _msg: RgAppendLogEntry, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Ok(())
    }
}

impl Handler<RgApplyToStateMachine> for SledActor {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, _msg: RgApplyToStateMachine, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Ok(())
    }
}

impl Handler<RgCreateSnapshot> for SledActor {
    type Result = Result<CurrentSnapshotData, ClientError>;

    fn handle(&mut self, _msg: RgCreateSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Err(ClientError{code: ErrorCode::Internal as i32, message: String::from("")})
    }
}

impl Handler<RgGetCurrentSnapshot> for SledActor {
    type Result = Result<Option<CurrentSnapshotData>, ClientError>;

    fn handle(&mut self, _msg: RgGetCurrentSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Ok(None)
    }
}

impl Handler<RgGetLogEntries> for SledActor {
    type Result = Result<Vec<RgEntry>, ClientError>;

    fn handle(&mut self, _msg: RgGetLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Ok(vec![])
    }
}

impl Handler<RgInstallSnapshot> for SledActor {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, _msg: RgInstallSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Ok(())
    }
}

impl Handler<RgReplicateLogEntries> for SledActor {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, _msg: RgReplicateLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        // TODO: impl
        Ok(())
    }
}

impl Handler<RgSaveHardState> for SledActor {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, msg: RgSaveHardState, _ctx: &mut Self::Context) -> Self::Result {
        // Serialize data.
        let hs: Vec<u8> = bincode::serialize(&msg.hs).map_err(|err| {
            error!("Error while serializing Raft HardState object: {:?}", err);
            ClientError::new_internal()
        })?;

        // Write to disk.
        self.db.insert(RAFT_HARDSTATE_KEY, hs).map_err(|err| {
            error!("Error while writing Raft HardState to disk: {:?}", err);
            ClientError::new_internal()
        })?;

        // Respond.
        Ok(())
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
    use crate::auth::UserRole;

    use proptest::prelude::*;
    use tempfile;

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
        fn index_users_data_should_return_expected_index_with_populated_users() {
            let (_dir, db) = setup_db();
            let users_index = setup_base_users(&db);
            let output = SledStorage::index_users_data(&db).expect("index users data");
            assert_eq!(output, users_index);
            assert_eq!(output.len(), users_index.len());
            assert_eq!(output.len(), 3);
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
}
