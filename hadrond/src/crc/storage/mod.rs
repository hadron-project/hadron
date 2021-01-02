#![allow(clippy::unit_arg)]
#![allow(unused_imports)] // TODO: remove this.
#![allow(dead_code)] // TODO: remove this.

mod index;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_raft::async_trait::async_trait;
use async_raft::raft::{Entry, EntryPayload, MembershipConfig};
use async_raft::storage::{CurrentSnapshotData, HardState, InitialState};
use async_raft::RaftStorage;
use prost::Message;
use sled::{transaction::ConflictableTransactionResult as TxResult, Batch, Config as SledConfig, Db, IVec, Transactional, Tree};
use tokio::fs::File;
use tokio::stream::StreamExt;
use tokio::sync::RwLock;

use crate::auth::{Claims, ClaimsV1, UserRole};
use crate::config::Config;
use crate::crc::network::{RaftClientRequest, RaftClientResponse};
use crate::crc::storage::index::{HCoreIndex, IndexWriteBatch, IndexWriteOp, NamespaceIndex, PipelineMeta, StreamMeta};
use crate::error::AppError;
use crate::models;
use crate::proto::client::{PipelineStageSubClient, StreamUnsubRequest};
use crate::proto::client::{StreamPubRequest, StreamPubResponse, StreamSubClient};
use crate::proto::client::{TransactionClient, UpdateSchemaRequest, UpdateSchemaResponse};
use crate::proto::storage as proto_storage;
use crate::utils;
use crate::NodeId;

// Storage paths.
const CORE_DATA_DIR: &str = "core"; //<dataDir>/core
const CORE_DATA_DIR_DB: &str = "db"; // <dataDir>/core/db
const CORE_DATA_DIR_SNAPS: &str = "snaps"; // <dataDir>/core/snaps

///////////////////////////////
// DB KV Collections (Trees) //
//
// The trees listed here are the primary trees we use in the system. There are other trees:
// - each stream gets its own tree, where each key corresponds to a data entry.
// - each pipeline gets its own tree, where each key corresponds to a pipeline instance.
//
// Naming is as follows:
// - each stream gets a CF named as `streams/{namespace}/{stream}`
// - each pipeline gets a CF named as `pipelines/{namespace}/{stream}`
//
// NOTE: there is a default unnamed tree which is part of the plain DB handle in sled. Various
// top-level items which do not logically belong in any of the other trees go here.

/// The DB tree of the raft log.
const TREE_LOG: &str = "log";
/// The DB tree for namespaces data.
const TREE_NAMESPACES: &str = "namespaces";
// /// The DB tree for endpoints data.
// const TREE_ENDPOINTS: &str = "endpoints";
/// The DB tree for streams data.
const TREE_STREAMS: &str = "streams";
/// The DB tree for stream indices data.
const TREE_STREAM_INDICES: &str = "stream_indices";
/// The DB tree for pipelines data.
const TREE_PIPELINES: &str = "pipelines";
/// The DB tree for pipeline indices data.
const TREE_PIPELINE_INDICES: &str = "pipeline_indices";
// /// The DB tree for users data.
// const TREE_USERS: &str = "users";
// /// The DB tree for tokens data.
// const TREE_TOKENS: &str = "tokens";

// DB keys.
const KEY_HARD_STATE: &str = "hard_state";
const KEY_LAST_APPLIED_LOG: &str = "last_applied_log";

// Error messages.
const ERR_ENCODE_LOG_ENTRY: &str = "error encoding raft log entry";
const ERR_DECODE_LOG_ENTRY: &str = "error decoding raft log entry";
// const ERR_DB_TASK: &str = "error awaiting database task";
const ERR_FLUSH: &str = "error flushing to disk";
const ERR_ITER_FAILURE: &str = "error returned during key/value iteration from database";
const ERR_READING_SNAPS_DIR: &str = "error reading snapshots dir";
const ERR_ITER_SNAPS_DIR: &str = "error iterating snapshot file entries";

/// Hadron core data storage.
pub struct HCoreStorage {
    /// The ID of this node in the cluster.
    id: NodeId,
    /// The application's runtime config.
    _config: Arc<Config>,
    /// The directory where this Raft's snapshots are held.
    snapshot_dir: PathBuf,
    /// The database handle used for disk storage.
    db: Db,
    /// A mapping of all live DB trees in the system.
    trees: RwLock<HashMap<IVec, Tree>>,

    /// The system data index.
    ///
    /// Each individual field of the index must be independently locked for reading/writing. It is
    /// important to note that we must reduce lock contention as much as possible. This can easily
    /// be achieved by simply dropping the lock guard as soon as possible whenever a lock is taken.
    index: HCoreIndex,
}

impl HCoreStorage {
    /// Create a new instance.
    ///
    /// This constructor is fallible as it will attempt to recover state from disk when first
    /// initialized.
    pub async fn new(id: NodeId, config: Arc<Config>) -> Result<Self> {
        // Ensure the needed paths are in place for the DB and other needed paths.
        let dbpath = PathBuf::from(&config.storage_data_path).join(CORE_DATA_DIR).join(CORE_DATA_DIR_DB);
        let snapshot_dir = PathBuf::from(&config.storage_data_path).join(CORE_DATA_DIR).join(CORE_DATA_DIR_SNAPS);
        tokio::fs::create_dir_all(&dbpath)
            .await
            .context("error creating dir for hadron core database")?;
        tokio::fs::create_dir_all(&snapshot_dir)
            .await
            .context("error creating dir for hadron core database snapshots")?;

        // Prep DB, open for usage & recover initial system state, building indices &c.
        let (db, trees) = Self::open_db(dbpath).await?;
        let index = Self::recover_system_state(&db).await?;

        // TODO: all DB interaction needs to flush_async

        Ok(Self {
            id,
            _config: config,
            snapshot_dir,
            db,
            trees: RwLock::new(trees),
            index,
        })
    }

    /// Open the database for usage.
    async fn open_db(dbpath: PathBuf) -> Result<(Db, HashMap<IVec, Tree>)> {
        Self::spawn_blocking(move || -> Result<(Db, HashMap<IVec, Tree>)> {
            let db = SledConfig::new().path(dbpath).mode(sled::Mode::HighThroughput).open()?;
            let trees = db
                .tree_names()
                .into_iter()
                .try_fold(HashMap::new(), |mut acc, name| -> Result<HashMap<IVec, Tree>> {
                    let tree = db.open_tree(&name)?;
                    acc.insert(name, tree);
                    Ok(acc)
                })?;
            Ok((db, trees))
        })
        .await?
    }
}

#[async_trait]
impl RaftStorage<RaftClientRequest, RaftClientResponse> for HCoreStorage {
    /// The storage engine's associated type used for exposing a snapshot for reading & writing.
    type Snapshot = tokio::fs::File;
    /// The error type used to indicate that Raft shutdown is required.
    type ShutdownError = ShutdownError;

    /// Get the latest membership config found in the log.
    ///
    /// This must always be implemented as a reverse search through the log to find the most
    /// recent membership config to be appended to the log.
    ///
    /// If a snapshot pointer is encountered, then the membership config embedded in that snapshot
    /// pointer should be used.
    ///
    /// If the system is pristine, then it should return the value of calling
    /// `MembershipConfig::new_initial(node_id)`. It is required that the storage engine persist
    /// the node's ID so that it is consistent across restarts.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn get_membership_config(&self) -> Result<MembershipConfig> {
        let log = self.get_tree(TREE_LOG).await?;
        let cfg = Self::spawn_blocking(move || -> Result<Option<MembershipConfig>> {
            for log_entry_res in log.iter().values().rev() {
                let log_entry = log_entry_res.context(ERR_ITER_FAILURE)?;
                let entry: Entry<RaftClientRequest> = utils::bin_decode(&log_entry).context(ERR_DECODE_LOG_ENTRY)?;
                match entry.payload {
                    EntryPayload::ConfigChange(cfg) => return Ok(Some(cfg.membership)),
                    _ => continue,
                }
            }
            Ok(None)
        })
        .await??
        .unwrap_or_else(|| MembershipConfig::new_initial(self.id));
        Ok(cfg)
    }

    /// Get Raft's state information from storage.
    ///
    /// When the Raft node is first started, it will call this interface on the storage system to
    /// fetch the last known state from stable storage. If no such entry exists due to being the
    /// first time the node has come online, then `InitialState::new_initial` should be used.
    ///
    /// ### pro tip
    /// The storage impl may need to look in a few different places to accurately respond to this
    /// request: the last entry in the log for `last_log_index` & `last_log_term`; the node's hard
    /// state record; and the index of the last log applied to the state machine.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn get_initial_state(&self) -> Result<InitialState> {
        // If the log is pristine, then return a pristine initial state.
        let (log, id) = (self.get_tree(TREE_LOG).await?, self.id);
        let log0 = log.clone();
        let pristine_opt = Self::spawn_blocking(move || -> Result<Option<InitialState>> {
            if log0.first()?.is_none() {
                return Ok(Some(InitialState::new_initial(id)));
            }
            Ok(None)
        })
        .await??;
        if let Some(state) = pristine_opt {
            return Ok(state);
        }

        // Else, the log is not pristine. Fetch all of the different state bits that we need.
        let db = self.db.clone();
        let membership = self.get_membership_config().await?;
        let state = Self::spawn_blocking(move || -> Result<InitialState> {
            // Get hard state.
            let hs: HardState = db
                .get(KEY_HARD_STATE)
                .context("error getting hard state from storage")?
                .map(|val| utils::bin_decode(val.as_ref()).context("error decoding hard state"))
                .ok_or_else(|| anyhow!("no hard state record found on disk"))??;
            // Get last log info.
            let last_log: Entry<RaftClientRequest> = log
                .last()?
                .ok_or_else(|| anyhow!("error fetching last entry of log, though first entry exists"))
                .map(|(_, val)| utils::bin_decode(&val).context(ERR_DECODE_LOG_ENTRY))??;
            // Get last applied log index.
            let last_applied_log_index = db
                .get(KEY_LAST_APPLIED_LOG)
                .context("error fetching last applied log index")?
                .map(|raw| utils::bin_decode::<u64>(raw.as_ref()).context("failed to decode last applied log index"))
                .unwrap_or(Ok(0))?;
            Ok(InitialState {
                last_log_index: last_log.index,
                last_log_term: last_log.term,
                last_applied_log: last_applied_log_index,
                membership,
                hard_state: hs,
            })
        })
        .await??;
        Ok(state)
    }

    /// Save Raft's hard-state.
    #[tracing::instrument(level = "trace", skip(self, hs), err)]
    async fn save_hard_state(&self, hs: &HardState) -> Result<()> {
        let raw = utils::bin_encode(hs).context("error encoding hard state")?;
        let db = self.db.clone();
        Self::spawn_blocking(move || -> Result<()> {
            db.insert(KEY_HARD_STATE, raw).context("error saving hard state to disk")?;
            Ok(())
        })
        .await??;
        self.db.flush_async().await?;
        Ok(())
    }

    /// Get a series of log entries from storage.
    ///
    /// The start value is inclusive in the search and the stop value is non-inclusive: `[start, stop)`.
    #[tracing::instrument(level = "trace", skip(self, start, stop), err)]
    async fn get_log_entries(&self, start: u64, stop: u64) -> Result<Vec<Entry<RaftClientRequest>>> {
        let (start, stop) = (utils::encode_u64(start), utils::encode_u64(stop));
        let log = self.get_tree(TREE_LOG).await?;
        let entries = Self::spawn_blocking(move || -> Result<Vec<Entry<RaftClientRequest>>> {
            let mut entries = vec![];
            for val_res in log.range(start..stop).values() {
                let val = val_res.context(ERR_ITER_FAILURE)?;
                let entry: Entry<RaftClientRequest> = utils::bin_decode(&val).context(ERR_DECODE_LOG_ENTRY)?;
                entries.push(entry);
            }
            Ok(entries)
        })
        .await??;
        Ok(entries)
    }

    /// Delete all logs starting from `start` and stopping at `stop`, else continuing to the end
    /// of the log if `stop` is `None`.
    #[tracing::instrument(level = "trace", skip(self, start, stop), err)]
    async fn delete_logs_from(&self, start: u64, stop: Option<u64>) -> Result<()> {
        let (log, start) = (self.get_tree(TREE_LOG).await?, utils::encode_u64(start));
        Self::spawn_blocking(move || -> Result<()> {
            // Build an atomic batch of delete operations.
            let mut batch = Batch::default();
            let iter = if let Some(stop) = stop {
                log.range(start..utils::encode_u64(stop))
            } else {
                log.range(start..)
            };
            for key_res in iter.keys() {
                let key = key_res.context(ERR_ITER_FAILURE)?;
                batch.remove(key);
            }
            // Apply batch.
            log.apply_batch(batch).context("error applying batch log deletion")?;
            Ok(())
        })
        .await??;
        self.db.flush_async().await?;
        Ok(())
    }

    /// Append a new entry to the log.
    #[tracing::instrument(level = "trace", skip(self, entry), err)]
    async fn append_entry_to_log(&self, entry: &Entry<RaftClientRequest>) -> Result<()> {
        let log = self.get_tree(TREE_LOG).await?;
        let entry_key = utils::encode_u64(entry.index);
        let entry_bytes = utils::bin_encode(&entry).context(ERR_ENCODE_LOG_ENTRY)?;
        Self::spawn_blocking(move || -> Result<()> {
            log.insert(entry_key, entry_bytes).context("error inserting log entry")?;
            Ok(())
        })
        .await??;
        self.db.flush_async().await?;
        Ok(())
    }

    /// Replicate a payload of entries to the log.
    ///
    /// Though the entries will always be presented in order, each entry's index should be used to
    /// determine its location to be written in the log.
    #[tracing::instrument(level = "trace", skip(self, entries), err)]
    async fn replicate_to_log(&self, entries: &[Entry<RaftClientRequest>]) -> Result<()> {
        // Prep data to be sent to thread for blocking op.
        let log = self.get_tree(TREE_LOG).await?;
        let mut batch = Batch::default();
        for entry in entries {
            let entry_key = utils::encode_u64(entry.index);
            let entry_bytes = utils::bin_encode(&entry).context(ERR_ENCODE_LOG_ENTRY)?;
            batch.insert(&entry_key, entry_bytes);
        }
        // Apply insert batch.
        Self::spawn_blocking(move || -> Result<()> {
            log.apply_batch(batch).context("error applying batch insert to log for replication")?;
            Ok(())
        })
        .await??;
        self.db.flush_async().await?;
        Ok(())
    }

    /// Apply the given log entry to the state machine.
    ///
    /// The Raft protocol guarantees that only logs which have been _committed_, that is, logs which
    /// have been replicated to a majority of the cluster, will be applied to the state machine.
    ///
    /// This is where the business logic of interacting with your application's state machine
    /// should live. This is 100% application specific. Perhaps this is where an application
    /// specific transaction is being started, or perhaps committed. This may be where a key/value
    /// is being stored. This may be where an entry is being appended to an immutable log.
    ///
    /// The behavior here is application specific, but errors should never be returned unless the
    /// error represents an actual failure to apply the entry. An error returned here will cause
    /// the Raft node to shutdown in order to preserve the safety of the data and avoid corruption.
    /// If instead some application specific error needs to be returned to the client, those
    /// variants must be encapsulated in the type `R`, which may have application specific success
    /// and error variants encoded in the type, perhaps using an inner `Result` type.
    #[tracing::instrument(level = "trace", skip(self, index, data))]
    async fn apply_entry_to_state_machine(&self, index: &u64, data: &RaftClientRequest) -> Result<RaftClientResponse> {
        let res = match data {
            RaftClientRequest::Transaction(req) => self.apply_transaction(index, req).await,
            RaftClientRequest::StreamPub(req) => self.apply_stream_pub(index, req).await,
            RaftClientRequest::StreamSub(req) => self.apply_stream_sub(index, req).await,
            RaftClientRequest::StreamUnsub(req) => self.apply_stream_unsub(index, req).await,
            RaftClientRequest::PipelineStageSub(req) => self.apply_pipeline_stage_sub(index, req).await,
            RaftClientRequest::UpdateSchema(req) => self.apply_update_schema(index, req).await,
        };
        let err = match res {
            Ok(res) => return Ok(res),
            Err(err) => err,
        };
        // If error is a shutdown error, then we need to propagate immediately.
        if err.downcast_ref::<ShutdownError>().is_some() {
            return Err(err);
        }
        // Else, we will write the last applied log.
        let (db, index_val) = (self.db.clone(), utils::encode_u64(*index));
        let task_res = Self::spawn_blocking(move || -> std::result::Result<(), ShutdownError> {
            db.insert(KEY_LAST_APPLIED_LOG, &index_val)
                .map(|_| ())
                .map_err(|err| ShutdownError::from(anyhow::Error::from(err)))
        })
        .await
        .and_then(|res| res);
        if let Err(err) = task_res {
            tracing::error!(error = %err, "error updating last applied log");
        }
        if let Err(err) = self.db.flush_async().await {
            tracing::error!(error = %err, ERR_FLUSH);
        }
        Err(err)
    }

    /// Apply the given payload of entries to the state machine, as part of replication.
    ///
    /// The Raft protocol guarantees that only logs which have been _committed_, that is, logs which
    /// have been replicated to a majority of the cluster, will be applied to the state machine.
    #[tracing::instrument(level = "trace", skip(self, entries), err)]
    async fn replicate_to_state_machine(&self, entries: &[(&u64, &RaftClientRequest)]) -> Result<()> {
        for (index, data) in entries {
            let res = match data {
                RaftClientRequest::Transaction(req) => self.apply_transaction(index, req).await,
                RaftClientRequest::StreamPub(req) => self.apply_stream_pub(index, req).await,
                RaftClientRequest::StreamSub(req) => self.apply_stream_sub(index, req).await,
                RaftClientRequest::StreamUnsub(req) => self.apply_stream_unsub(index, req).await,
                RaftClientRequest::PipelineStageSub(req) => self.apply_pipeline_stage_sub(index, req).await,
                RaftClientRequest::UpdateSchema(req) => self.apply_update_schema(index, req).await,
            };
            let err = match res {
                Ok(_) => continue,
                Err(err) => err,
            };
            // If error is a shutdown error, then we need to propagate immediately.
            if err.downcast_ref::<ShutdownError>().is_some() {
                return Err(err);
            }
            // Else, we will write the last applied log.
            let (db, index_val) = (self.db.clone(), utils::encode_u64(**index));
            Self::spawn_blocking(move || -> std::result::Result<(), ShutdownError> {
                db.insert(KEY_LAST_APPLIED_LOG, &index_val)
                    .map(|_| ())
                    .map_err(|err| ShutdownError::from(anyhow::Error::from(err)))
            })
            .await
            .and_then(|res| res)?;
        }
        self.db.flush_async().await?;
        Ok(())
    }

    /// Perform log compaction, returning a handle to the generated snapshot.
    ///
    /// ### implementation guide
    /// When performing log compaction, the compaction can only cover the breadth of the log up to
    /// the last applied log and under write load this value may change quickly. As such, the
    /// storage implementation should export/checkpoint/snapshot its state machine, and then use
    /// the value of that export's last applied log as the metadata indicating the breadth of the
    /// log covered by the snapshot.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn do_log_compaction(&self) -> Result<CurrentSnapshotData<Self::Snapshot>> {
        // NOTE: no blockers here, this is ready to rock.
        Err(anyhow!("log compaction not yet implemented"))
    }

    /// Create a new blank snapshot, returning a writable handle to the snapshot object along with
    /// the ID of the snapshot.
    ///
    /// ### implementation guide
    /// See the [storage chapter of the guide](https://async-raft.github.io/async-raft/storage.html)
    /// for details on how to implement this handler.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn create_snapshot(&self) -> Result<(String, Box<Self::Snapshot>)> {
        let snapname = format!("{}.snap.tmp", uuid::Uuid::new_v4().to_string());
        let snappath = self.snapshot_dir.join(&snapname);
        let snapfile = tokio::fs::File::create(snappath).await.context("error creating snapshot file")?;
        Ok((snapname, Box::new(snapfile)))
    }

    /// Finalize the installation of a snapshot which has finished streaming from the cluster leader.
    ///
    /// Delete all entries in the log through `delete_through`, unless `None`, in which case
    /// all entries of the log are to be deleted.
    ///
    /// Write a new snapshot pointer to the log at the given `index`. The snapshot pointer should be
    /// constructed via the `Entry::new_snapshot_pointer` constructor and the other parameters
    /// provided to this method.
    ///
    /// All other snapshots should be deleted at this point.
    ///
    /// ### snapshot
    /// A snapshot created from an earlier call to `created_snapshot` which provided the snapshot.
    /// By the time ownership of the snapshot object is returned here, its
    /// `AsyncWriteExt.shutdown()` method will have been called, so no additional writes should be
    /// made to the snapshot.
    #[tracing::instrument(level = "trace", skip(self, snapshot), err)]
    async fn finalize_snapshot_installation(
        &self, index: u64, term: u64, delete_through: Option<u64>, id: String, snapshot: Box<Self::Snapshot>,
    ) -> Result<()> {
        // Extract metadata from the snapshot.
        let metadata = match self.extract_snapshot_metadata(*snapshot).await? {
            Some(metadata) => metadata,
            None => return Err(anyhow!("error extracting snapshot metadata")),
        };
        let membership_proto = metadata.membership.unwrap_or_default();
        let membership = MembershipConfig {
            members: membership_proto.members.into_iter().collect(),
            members_after_consensus: if membership_proto.members_after_consensus.is_empty() {
                None
            } else {
                Some(membership_proto.members_after_consensus.into_iter().collect())
            },
        };

        // Create the snapshot pointer & update log.
        let snapshot_entry = Entry::<RaftClientRequest>::new_snapshot_pointer(index, term, id.clone(), membership);
        let snapshot_entry_enc = utils::bin_encode(&snapshot_entry).context(ERR_ENCODE_LOG_ENTRY)?;
        let snapshot_entry_idx = utils::encode_u64(snapshot_entry.index);
        let (log, start) = (self.get_tree(TREE_LOG).await?, utils::encode_u64(0));
        Self::spawn_blocking(move || -> Result<()> {
            // Build an atomic batch of delete operations.
            let mut batch = Batch::default();
            let iter = if let Some(stop) = delete_through {
                log.range(start..utils::encode_u64(stop))
            } else {
                log.range(start..)
            };
            for key_res in iter.keys() {
                let key = key_res.context(ERR_ITER_FAILURE)?;
                batch.remove(key);
            }
            // Insert the new snapshot pointer & apply the batch.
            // TODO: test to ensure that the insertion stays after the deletes.
            batch.insert(&snapshot_entry_idx, snapshot_entry_enc.as_slice());
            log.apply_batch(batch).context("error applying batch log operations")?;
            Ok(())
        })
        .await??;
        self.db.flush_async().await?;

        // Rename the current snapshot to ensure it is not treated as a stale snapshot.
        let old_path = self.snapshot_dir.join(&format!("{}.snap.tmp", id));
        let new_path = self.snapshot_dir.join(&format!("{}.snap", id));
        tokio::fs::rename(old_path, new_path).await.context("error updating snapshot name")?;

        // Ensure any old snapshots are pruned as they now no longer have any pointers.
        let mut dir = tokio::fs::read_dir(&self.snapshot_dir).await.context(ERR_READING_SNAPS_DIR)?;
        while let Some(snap_res) = dir.next().await {
            let snap = snap_res.context(ERR_ITER_SNAPS_DIR)?;
            let snappath = snap.path();
            if snap.file_name() == id.as_str() {
                continue;
            }
            tracing::trace!(path = ?snappath, "removing old snapshot file");
            tokio::fs::remove_file(&snappath).await.context("error removing old snapshot")?;
        }
        Ok(())
    }

    /// Get a readable handle to the current snapshot, along with its metadata.
    ///
    /// ### implementation algorithm
    /// Implementing this method should be straightforward. Check the configured snapshot
    /// directory for any snapshot files. A proper implementation will only ever have one
    /// active snapshot, though another may exist while it is being created. As such, it is
    /// recommended to use a file naming pattern which will allow for easily distinguishing between
    /// the current live snapshot, and any new snapshot which is being created.
    ///
    /// A proper snapshot implementation will store the term, index and membership config as part
    /// of the snapshot, which should be decoded for creating this method's response data.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn get_current_snapshot(&self) -> Result<Option<CurrentSnapshotData<Self::Snapshot>>> {
        // Search for a live snapshot.
        let mut dir = tokio::fs::read_dir(&self.snapshot_dir).await.context(ERR_READING_SNAPS_DIR)?;
        let mut snappath_opt = None;
        while let Some(snap_res) = dir.next().await {
            let snap = snap_res.context(ERR_ITER_SNAPS_DIR)?;
            let snappath = snap.path();
            if snappath.extension().map(|ext| ext == "snap").unwrap_or(false) {
                snappath_opt = Some(snappath);
                break;
            }
        }
        let snappath = match snappath_opt {
            Some(snappath) => snappath,
            None => return Ok(None),
        };

        // Once we have a snapshot, we need to crack it open and find its metadata file.
        let snapfile = File::open(&snappath).await.context("error opening snapshot file")?;
        let metadata = match self.extract_snapshot_metadata(snapfile).await? {
            Some(metadata) => metadata,
            None => return Ok(None),
        };

        // We now have all the data we need. Open a fresh handle to the snapshot file & build response.
        let snapfile = File::open(&snappath).await.context("error opening snapshot file")?;
        let membership = metadata.membership.unwrap_or_default();
        Ok(Some(CurrentSnapshotData {
            term: metadata.term,
            index: metadata.index,
            membership: MembershipConfig {
                members: membership.members.into_iter().collect(),
                members_after_consensus: if membership.members_after_consensus.is_empty() {
                    None
                } else {
                    Some(membership.members_after_consensus.into_iter().collect())
                },
            },
            snapshot: Box::new(snapfile),
        }))
    }
}

impl HCoreStorage {
    /// Gets the target DB tree by name, creating it if it does not exist.
    async fn get_tree(&self, name: &str) -> std::result::Result<Tree, ShutdownError> {
        // If we already have the tree in our mapping, clone it and return.
        let trees = self.trees.read().await;
        if let Some(tree) = trees.get(name.as_bytes()).cloned() {
            return Ok(tree);
        }
        drop(trees);

        // Else, we need to create the tree.
        let (db, ivname) = (self.db.clone(), IVec::from(name));
        let tree = Self::spawn_blocking(move || -> Result<Tree> { Ok(db.open_tree(ivname)?) })
            .await
            .and_then(|res| res.map_err(|err| ShutdownError(anyhow!("could not open DB tree {}", err))))?;

        self.trees.write().await.insert(name.into(), tree.clone());
        Ok(tree)
    }

    /// Extract the metadata of the given snapshot file.
    #[tracing::instrument(level = "trace", skip(self, snap), err)]
    async fn extract_snapshot_metadata(&self, snap: File) -> Result<Option<proto_storage::SnapshotMetadata>> {
        // Once we have a snapshot, we need to crack it open and find its metadata file.
        let mut archive = tokio_tar::Archive::new(snap);
        let mut entries = archive.entries().context("error iterating over snapshot entries")?;
        while let Some(entry_res) = entries.next().await {
            let entry = entry_res.context("error iterating snapshot archive entries")?;
            let path = entry.path().context("error reading path in snapshot archive")?;
            let is_metadata = path.file_name().map(|name| name == "metadata").unwrap_or(false);
            if !is_metadata {
                continue;
            }
            let metadata_bytes = tokio::fs::read(&path).await.context("error reading snapshot metadata bytes")?;
            let metadata = proto_storage::SnapshotMetadata::decode(metadata_bytes.as_slice()).context("error reading snapshot metadata")?;
            return Ok(Some(metadata));
        }
        Ok(None)
    }

    //////////////////////////////////////////////////////////////////////////////////////////////
    // Apply Entry to State Machine Handlers /////////////////////////////////////////////////////

    /// Apply a write function to the database.
    #[tracing::instrument(level = "trace", skip(f), err)]
    async fn spawn_blocking<F, R>(f: F) -> std::result::Result<R, ShutdownError>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|err| ShutdownError::from(anyhow::Error::from(err)))
    }

    /// Apply a write batch to the database.
    #[tracing::instrument(level = "trace", skip(tree, batch), err)]
    async fn spawn_blocking_batch(tree: Tree, batch: Batch) -> std::result::Result<(), ShutdownError> {
        tokio::task::spawn_blocking(move || tree.apply_batch(batch).map_err(|err| ShutdownError::from(anyhow::Error::from(err))))
            .await
            .map_err(|err| ShutdownError::from(anyhow::Error::from(err)))
            .and_then(|res| res)
    }

    #[tracing::instrument(level = "trace", skip(self, _index, _req), err)]
    async fn apply_transaction(&self, _index: &u64, _req: &TransactionClient) -> Result<RaftClientResponse> {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, index, req))]
    async fn apply_stream_pub(&self, index: &u64, req: &StreamPubRequest) -> Result<RaftClientResponse> {
        tracing::info!(index, "applying stream pub request to stream");
        // Fetch the stream's metadata, get the next index to use, and increment it for later use.
        let stream = self
            .index
            .get_stream(&req.namespace, &req.stream)
            .await
            .ok_or_else(|| AppError::InvalidInput("target stream does not exist".into()))?;
        let id = {
            let mut id = stream.next_id.write().await;
            let id_to_use = *id;
            *id += 1;
            id_to_use
        };

        // Write the payload to the target stream.
        let tree = self.get_tree(&format!("streams/{}/{}", &req.namespace, &req.stream)).await?;
        let key = utils::encode_u64(id);
        let payload = req.payload.clone();
        Self::spawn_blocking(move || {
            tree.insert(&key, payload.as_slice())
                .map_err(|err| ShutdownError::from(anyhow::Error::from(err)))
        })
        .await??;

        Ok(RaftClientResponse::StreamPub(StreamPubResponse { id }))
    }

    #[tracing::instrument(level = "trace", skip(self, _index, _req), err)]
    async fn apply_stream_sub(&self, _index: &u64, _req: &StreamSubClient) -> Result<RaftClientResponse> {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, _index, _req), err)]
    async fn apply_stream_unsub(&self, _index: &u64, _req: &StreamUnsubRequest) -> Result<RaftClientResponse> {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, _index, _req), err)]
    async fn apply_pipeline_stage_sub(&self, _index: &u64, _req: &PipelineStageSubClient) -> Result<RaftClientResponse> {
        todo!("")
    }

    /// Apply a schema update to the system.
    #[tracing::instrument(level = "trace", skip(self, index, req))]
    async fn apply_update_schema(&self, index: &u64, req: &UpdateSchemaRequest) -> Result<RaftClientResponse> {
        tracing::info!(index, schema = %req.schema, "applying update schema request to system");
        // TODO: DDL is in flux, update this once ready.
        // // Decode & statically validate the request.
        // let update = models::DDLSchemaUpdate::decode_and_validate(&req.schema)?;
        // TODO: perform full dynamic validation of this content, checking indices, and then falling back to
        // checking other statements in the given document.

        // // Now, we check to see if the changeset has already been applied to the system.
        // let is_applied = self.index.changesets.read().await.get(&update.changeset.name).is_some();
        // if is_applied {
        //     // TODO: probably add a no-op inner variant for better info
        //     return Ok(RaftClientResponse::UpdateSchema(UpdateSchemaResponse {}));
        // }

        // // If not, then we continue and attempt to apply each DDL entry to the system.
        // let (mut batch, mut nsmodels, mut streams, mut stream_indices, mut pipelines, mut pipeline_indices) = (
        //     Batch::default(),
        //     Batch::default(),
        //     Batch::default(),
        //     Batch::default(),
        //     Batch::default(),
        //     Batch::default(),
        // );
        // batch.insert(KEY_LAST_APPLIED_LOG.as_bytes(), &utils::encode_u64(*index));
        // let mut index_batch = IndexWriteBatch::default();
        // for entry in update.statements {
        //     match entry {
        //         models::DDL::Namespace(namespace) => self.create_namespace(namespace, &mut nsmodels, &mut index_batch).await?,
        //         models::DDL::Stream(stream) => self.create_stream(stream, &mut streams, &mut stream_indices, &mut index_batch).await?,
        //         models::DDL::Pipeline(pipeline) => {
        //             self.create_pipeline(pipeline, &mut pipelines, &mut pipeline_indices, &mut index_batch)
        //                 .await?
        //         }
        //     }
        // }

        // // TODO: also record the changeset to disk & index batch.

        // // Apply the batch of changes.
        // let db = self.db.clone();
        // let ns = self.get_tree(TREE_NAMESPACES).await?;
        // let st = self.get_tree(TREE_STREAMS).await?;
        // let sti = self.get_tree(TREE_STREAM_INDICES).await?;
        // let pi = self.get_tree(TREE_PIPELINES).await?;
        // let pii = self.get_tree(TREE_PIPELINE_INDICES).await?;
        // Self::spawn_blocking(move || {
        //     let trees = (&db as &Tree, &ns, &st, &sti, &pi, &pii);
        //     trees
        //         .transaction(
        //             |(tx_def, tx_ns, tx_streams, tx_streamidx, tx_pipe, tx_pipeidx)| -> TxResult<(), sled::Error> {
        //                 tx_def.apply_batch(&batch)?;
        //                 tx_ns.apply_batch(&nsmodels)?;
        //                 tx_streams.apply_batch(&streams)?;
        //                 tx_streamidx.apply_batch(&stream_indices)?;
        //                 tx_pipe.apply_batch(&pipelines)?;
        //                 tx_pipeidx.apply_batch(&pipeline_indices)?;
        //                 Ok(())
        //             },
        //         )
        //         .map_err(|err| ShutdownError::from(anyhow::Error::from(err)))
        // })
        // .await??;
        // self.index.apply_batch(index_batch).await?;
        Ok(RaftClientResponse::UpdateSchema(UpdateSchemaResponse {}))
    }

    // /// Create a new namespace in the database.
    // #[tracing::instrument(level = "trace", skip(self, ns, models, index_batch))]
    // async fn create_namespace(&self, ns: models::Namespace, models: &mut Batch, index_batch: &mut IndexWriteBatch) -> Result<()> {
    //     // Check if the namespace already exists. If so, done.
    //     if self.index.get_namespace(&ns.name).await.is_some() {
    //         return Ok(());
    //     }
    //     // Namespace does not yet exist, so we create an entry for it.
    //     let meta = utils::encode_proto(proto_storage::Namespace::from(&ns))?;
    //     models.insert(ns.name.as_str(), meta.as_slice());
    //     index_batch.push(IndexWriteOp::InsertNamespace {
    //         name: ns.name,
    //         description: ns.description,
    //     });
    //     Ok(())
    // }

    // /// Create a new stream in the database.
    // #[tracing::instrument(level = "trace", skip(self, stream, models, indices, index_batch))]
    // async fn create_stream(&self, stream: models::Stream, models: &mut Batch, indices: &mut Batch, index_batch: &mut IndexWriteBatch) -> Result<()> {
    //     // Check if the stream already exists. If so, done.
    //     if self.index.get_stream(&stream.namespace, &stream.name).await.is_some() {
    //         return Ok(());
    //     }
    //     // Stream does not yet exist, so we create an entry for it, setting its initial index to 0.
    //     let model = utils::encode_proto(proto_storage::Stream::from(&stream))?;
    //     let nsname = format!("{}/{}", &stream.namespace, &stream.name);
    //     models.insert(nsname.as_str(), model);
    //     indices.insert(nsname.as_str(), &utils::encode_u64(0));
    //     index_batch.push(IndexWriteOp::InsertStream {
    //         namespace: stream.namespace,
    //         name: stream.name,
    //         meta: StreamMeta::new(stream.description),
    //     });
    //     // Create a new DB Tree for the stream.
    //     self.get_tree(&format!("streams/{}", nsname)).await?;
    //     Ok(())
    // }

    // /// Create a new pipeline in the database.
    // #[tracing::instrument(level = "trace", skip(self, pipeline, models, indices, index_batch))]
    // async fn create_pipeline(
    //     &self, pipeline: models::Pipeline, models: &mut Batch, indices: &mut Batch, index_batch: &mut IndexWriteBatch,
    // ) -> Result<()> {
    //     // Check if the pipeline already exists. If so, done.
    //     if self.index.get_pipeline(&pipeline.namespace, &pipeline.name).await.is_some() {
    //         return Ok(());
    //     }
    //     // Stream does not yet exist, so we create an entry for it, setting its initial index to 0.
    //     let model = utils::encode_proto(proto_storage::Pipeline::from(&pipeline))?;
    //     let nsname = format!("{}/{}", &pipeline.namespace, &pipeline.name);
    //     models.insert(nsname.as_str(), model);
    //     indices.insert(nsname.as_str(), &utils::encode_u64(0));
    //     let meta = PipelineMeta::new(pipeline.trigger_stream, pipeline.description);
    //     index_batch.push(IndexWriteOp::InsertPipeline {
    //         namespace: pipeline.namespace,
    //         name: pipeline.name,
    //         meta,
    //     });
    //     // Create a new DB Tree for the pipeline.
    //     self.get_tree(&format!("pipelines/{}", nsname)).await?;
    //     Ok(())
    // }

    //////////////////////////////////////////////////////////////////////////////////////////////
    // System State Recovery /////////////////////////////////////////////////////////////////////

    /// Recover the system's state, building indices and the like.
    #[tracing::instrument(level = "trace", skip(db), err)]
    pub async fn recover_system_state(db: &Db) -> Result<HCoreIndex> {
        let users = RwLock::new(Self::recover_user_permissions(db).await?);
        let tokens = RwLock::new(Self::recover_token_permissions(db).await?);
        // TODO: finish recovering data from disk
        Ok(HCoreIndex {
            changesets: Default::default(),
            namespaces: Default::default(),
            users,
            tokens,
        })
    }

    /// Recover namespaces.
    #[tracing::instrument(level = "trace", skip(_db), err)]
    pub async fn recover_namespaces(_db: &Db) -> Result<HashMap<String, Arc<NamespaceIndex>>> {
        // Iterate over all namespaces and accumulate data.
        // let data = HashMap::new();
        // let iter = db.prefix_iterator_cf(Self::get_cf_namespaces(db)?, "namespaces/");
        // iter.status()?;
        // for (key, val) in iter {}
        Ok(Default::default())
    }

    /// Recover the system's user permissions state.
    #[tracing::instrument(level = "trace", skip(_db), err)]
    pub async fn recover_user_permissions(_db: &Db) -> Result<HashMap<String, Arc<UserRole>>> {
        Ok(Default::default()) // TODO: this will need to be updated so that we can check user PWs based on index.
    }

    /// Recover the system's token permissions state.
    #[tracing::instrument(level = "trace", skip(_db), err)]
    pub async fn recover_token_permissions(_db: &Db) -> Result<HashMap<u64, Arc<Claims>>> {
        let mut tokens = HashMap::new();
        tokens.insert(0, Arc::new(Claims::V1(ClaimsV1::All)));
        Ok(tokens) // TODO: recover token state.
    }

    /// Get the given token's claims, else return an auth error.
    pub async fn must_get_token_claims(&self, token_id: &u64) -> Result<Arc<Claims>> {
        match self.index.tokens.read().await.get(token_id).cloned() {
            Some(claims) => Ok(claims),
            None => Err(AppError::UnknownToken.into()),
        }
    }
}

/// The error type used to indicate that Raft shutdown is required.
#[derive(Debug, thiserror::Error)]
#[error("fatal storage error: {0}")]
pub struct ShutdownError(#[from] anyhow::Error);
