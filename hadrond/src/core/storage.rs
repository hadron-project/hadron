#![allow(clippy::unit_arg)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_raft::async_trait::async_trait;
use async_raft::raft::{Entry, EntryPayload, MembershipConfig};
use async_raft::storage::{CurrentSnapshotData, HardState, InitialState};
use async_raft::{AppData, AppDataResponse, RaftStorage};
use prost::Message;
use tokio::fs::File;
use tokio::stream::StreamExt;
use tokio::sync::mpsc;

use crate::auth::{Claims, ClaimsV1, UserRole};
use crate::config::Config;
use crate::core::network::{RaftClientRequest, RaftClientResponse};
use crate::core::HCore;
use crate::network::PeerClient;
use crate::proto::client::{PipelineStageSubClient, PipelineStageSubServer, StreamUnsubRequest, StreamUnsubResponse};
use crate::proto::client::{StreamPubRequest, StreamPubResponse, StreamSubClient, StreamSubServer};
use crate::proto::client::{TransactionClient, TransactionServer, UpdateSchemaRequest, UpdateSchemaResponse};
use crate::proto::peer;
use crate::proto::peer::{RaftAppendEntriesMsg, RaftInstallSnapshotMsg, RaftVoteMsg};
use crate::proto::storage as proto_storage;
use crate::utils;
use crate::NodeId;

// Storage paths.
const CORE_DATA_DIR: &str = "core"; //<dataDir>/core
const CORE_DATA_DIR_DB: &str = "db"; // <dataDir>/core/db
const CORE_DATA_DIR_SNAPS: &str = "snaps"; // <dataDir>/core/snaps

// DB trees.
const TREE_ENDPOINTS: &str = "endpoints";
const TREE_STREAMS: &str = "streams";
const TREE_PIPELINES: &str = "pipelines";
const TREE_USERS: &str = "users";
const TREE_TOKENS: &str = "tokens";

// DB keys.
const KEY_HARD_STATE: &str = "hard_state";
const KEY_LAST_APPLIED_LOG: &str = "last_applied_log";

// Error messages.
const ERR_ENCODE_LOG_ENTRY: &str = "error encoding raft log entry";
const ERR_DECODE_LOG_ENTRY: &str = "error decoding raft log entry";
const ERR_DB_TASK: &str = "error awaiting database task";
const ERR_FLUSH: &str = "error flushing to disk";
const ERR_ITER_FAILURE: &str = "error returned as value for key/value iteration from database";
const ERR_READING_SNAPS_DIR: &str = "error reading snapshots dir";
const ERR_ITER_SNAPS_DIR: &str = "error iterating snapshot file entries";

/// Hadron core data storage.
pub struct HCoreStorage {
    /// The ID of this node in the cluster.
    id: NodeId,
    /// The application's runtime config.
    config: Arc<Config>,
    /// A channel for emitting index updates.
    index_tx: mpsc::UnboundedSender<IndexUpdate>,
    /// The directory where this Raft's snapshots are held.
    snapshot_dir: PathBuf,
    /// The database handle used for disk storage.
    db: sled::Db,
    /// The database tree handle used for storing the replicated log.
    log: sled::Tree,
    /// The database tree handle used for storing endpoints.
    endpoints: sled::Tree,
    /// The database tree handle used for storing streams.
    streams: sled::Tree,
    /// The database tree handle used for storing pipelines.
    pipelines: sled::Tree,
    /// The database tree handle used for storing users.
    users: sled::Tree,
    /// The database tree handle used for storing tokens.
    tokens: sled::Tree,
}

impl HCoreStorage {
    /// Create a new instance.
    ///
    /// This constructor is fallible as it will attempt to recover state from disk when first
    /// initialized.
    pub async fn new(id: NodeId, config: Arc<Config>) -> Result<(Self, mpsc::UnboundedReceiver<IndexUpdate>)> {
        // Ensure the needed paths are in place for the DB and other needed paths.
        let dbpath = PathBuf::from(&config.storage_data_path).join(CORE_DATA_DIR).join(CORE_DATA_DIR_DB);
        let snapshot_dir = PathBuf::from(&config.storage_data_path).join(CORE_DATA_DIR).join(CORE_DATA_DIR_SNAPS);
        tokio::fs::create_dir_all(&dbpath)
            .await
            .context("error creating dir for hadron core database")?;
        tokio::fs::create_dir_all(&snapshot_dir)
            .await
            .context("error creating dir for hadron core database snapshots")?;

        // Open database and trees.
        let db = sled::Config::new()
            .mode(sled::Mode::HighThroughput)
            .path(&dbpath)
            .open()
            .context("error opening core database")?;
        let log = db.open_tree("log").context("error opening database tree for the log")?;
        let endpoints = db.open_tree(TREE_ENDPOINTS).context("error opening database tree for endpoints")?;
        let streams = db.open_tree(TREE_STREAMS).context("error opening database tree for streams")?;
        let pipelines = db.open_tree(TREE_PIPELINES).context("error opening database tree for pipelines")?;
        let users = db.open_tree(TREE_USERS).context("error opening database tree for users")?;
        let tokens = db.open_tree(TREE_TOKENS).context("error opening database tree for tokens")?;

        let (index_tx, index_rx) = mpsc::unbounded_channel();
        Ok((
            Self {
                id,
                config,
                index_tx,
                snapshot_dir,
                db,
                log,
                endpoints,
                streams,
                pipelines,
                users,
                tokens,
            },
            index_rx,
        ))
    }
}

#[async_trait]
impl RaftStorage<RaftClientRequest, RaftClientResponse> for HCoreStorage {
    /// The storage engine's associated type used for exposing a snapshot for reading & writing.
    type Snapshot = tokio::fs::File;

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
        let log = self.log.clone();
        let cfg = tokio::task::spawn_blocking(move || -> Result<Option<MembershipConfig>> {
            for log_res in log.iter().values().rev() {
                let log = log_res.context(ERR_ITER_FAILURE)?;
                let entry: Entry<RaftClientRequest> = utils::bin_decode(&log).context(ERR_DECODE_LOG_ENTRY)?;
                match entry.payload {
                    EntryPayload::ConfigChange(cfg) => return Ok(Some(cfg.membership)),
                    _ => continue,
                }
            }
            Ok(None)
        })
        .await
        .context(ERR_DB_TASK)??
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
        let (log, id) = (self.log.clone(), self.id);
        let pristine_opt = tokio::task::spawn_blocking(move || -> Result<Option<InitialState>> {
            if log.first().context("error fetching first entry of log")?.is_none() {
                return Ok(Some(InitialState::new_initial(id)));
            }
            Ok(None)
        })
        .await
        .context(ERR_DB_TASK)??;
        if let Some(state) = pristine_opt {
            return Ok(state);
        }

        // Else, the log is not pristine. Fetch all of the different state bits that we need.
        let (db, log) = (self.db.clone(), self.log.clone());
        let membership = self.get_membership_config().await?;
        let state = tokio::task::spawn_blocking(move || -> Result<InitialState> {
            // Get hard state.
            let hs: HardState = db
                .get(KEY_HARD_STATE)
                .context("error getting hard state from storage")?
                .map(|val| utils::bin_decode(&val).context("error decoding hard state"))
                .ok_or_else(|| anyhow!("no hard state record found on disk"))??;
            // Get last log info.
            let last_log: Entry<RaftClientRequest> = log
                .last()
                .context("error fetching last entry of log")?
                .map(|(_, val)| utils::bin_decode(&val).context(ERR_DECODE_LOG_ENTRY))
                .ok_or_else(|| anyhow!("error fetching last entry of log, though first entry exists"))??;
            // Get last applied log index.
            let last_applied_log_index = db
                .get(KEY_LAST_APPLIED_LOG)
                .context("error fetching last applied log index")?
                .map(|raw| utils::bin_decode::<u64>(&raw).context("failed to decode last applied log index"))
                .unwrap_or(Ok(0))?;
            Ok(InitialState {
                last_log_index: last_log.index,
                last_log_term: last_log.term,
                last_applied_log: last_applied_log_index,
                membership,
                hard_state: hs,
            })
        })
        .await
        .context(ERR_DB_TASK)??;
        Ok(state)
    }

    /// Save Raft's hard-state.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn save_hard_state(&self, hs: &HardState) -> Result<()> {
        let raw = utils::bin_encode(hs).context("error encoding hard state")?;
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            db.insert(KEY_HARD_STATE, raw).context("error saving hard state to disk")?;
            Ok(())
        })
        .await
        .context(ERR_DB_TASK)??;
        self.db.flush_async().await.context(ERR_FLUSH)?;
        Ok(())
    }

    /// Get a series of log entries from storage.
    ///
    /// The start value is inclusive in the search and the stop value is non-inclusive: `[start, stop)`.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn get_log_entries(&self, start: u64, stop: u64) -> Result<Vec<Entry<RaftClientRequest>>> {
        let (start, stop) = (utils::encode_u64(start), utils::encode_u64(stop));
        let log = self.log.clone();
        let entries = tokio::task::spawn_blocking(move || -> Result<Vec<Entry<RaftClientRequest>>> {
            let mut entries = vec![];
            for val_res in log.range(start..stop).values() {
                let val = val_res.context(ERR_ITER_FAILURE)?;
                let entry: Entry<RaftClientRequest> = utils::bin_decode(&val).context(ERR_DECODE_LOG_ENTRY)?;
                entries.push(entry);
            }
            Ok(entries)
        })
        .await
        .context(ERR_DB_TASK)??;
        Ok(entries)
    }

    /// Delete all logs starting from `start` and stopping at `stop`, else continuing to the end
    /// of the log if `stop` is `None`.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn delete_logs_from(&self, start: u64, stop: Option<u64>) -> Result<()> {
        let log = self.log.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            // Build an atomic batch of delete operations.
            let mut batch = sled::Batch::default();
            if let Some(stop) = stop {
                for key_res in log.range(utils::encode_u64(start)..utils::encode_u64(stop)).keys() {
                    let key = key_res.context(ERR_ITER_FAILURE)?;
                    batch.remove(key);
                }
            } else {
                for key_res in log.range(utils::encode_u64(start)..).keys() {
                    let key = key_res.context(ERR_ITER_FAILURE)?;
                    batch.remove(key);
                }
            }
            // Apply batch.
            log.apply_batch(batch).context("error applying batch log deletion")?;
            Ok(())
        })
        .await
        .context(ERR_DB_TASK)??;
        self.log.flush_async().await.context(ERR_FLUSH)?;
        Ok(())
    }

    /// Append a new entry to the log.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn append_entry_to_log(&self, entry: &Entry<RaftClientRequest>) -> Result<()> {
        let log = self.log.clone();
        let entry_key = utils::encode_u64(entry.index);
        let entry_bytes = utils::bin_encode(&entry).context(ERR_ENCODE_LOG_ENTRY)?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            log.insert(entry_key, entry_bytes).context("error inserting log entry")?;
            Ok(())
        })
        .await
        .context(ERR_DB_TASK)??;
        self.log.flush_async().await.context(ERR_FLUSH)?;
        Ok(())
    }

    /// Replicate a payload of entries to the log.
    ///
    /// Though the entries will always be presented in order, each entry's index should be used to
    /// determine its location to be written in the log.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn replicate_to_log(&self, entries: &[Entry<RaftClientRequest>]) -> Result<()> {
        // Create a new batch operation which will be atomically applied to the log.
        let mut batch = sled::Batch::default();
        for entry in entries {
            let entry_key = utils::encode_u64(entry.index);
            let entry_bytes = utils::bin_encode(&entry).context(ERR_ENCODE_LOG_ENTRY)?;
            batch.insert(&entry_key, entry_bytes.as_slice());
        }

        // Apply insert batch.
        let log = self.log.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            log.apply_batch(batch).context("error applying batch insert to log for replication")?;
            Ok(())
        })
        .await
        .context(ERR_DB_TASK)??;
        self.log.flush_async().await.context(ERR_FLUSH)?;
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
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn apply_entry_to_state_machine(&self, index: &u64, data: &RaftClientRequest) -> Result<RaftClientResponse> {
        Ok(match data {
            RaftClientRequest::Transaction(req) => self.apply_transaction(index, req).await?,
            RaftClientRequest::StreamPub(req) => self.apply_stream_pub(index, req).await?,
            RaftClientRequest::StreamSub(req) => self.apply_stream_sub(index, req).await?,
            RaftClientRequest::StreamUnsub(req) => self.apply_stream_unsub(index, req).await?,
            RaftClientRequest::PipelineStageSub(req) => self.apply_pipeline_stage_sub(index, req).await?,
            RaftClientRequest::UpdateSchema(req) => self.apply_update_schema(index, req).await?,
        })
    }

    /// Apply the given payload of entries to the state machine, as part of replication.
    ///
    /// The Raft protocol guarantees that only logs which have been _committed_, that is, logs which
    /// have been replicated to a majority of the cluster, will be applied to the state machine.
    #[tracing::instrument(level = "trace", skip(self), err)]
    async fn replicate_to_state_machine(&self, entries: &[(&u64, &RaftClientRequest)]) -> Result<()> {
        for (index, data) in entries {
            let _ = match data {
                RaftClientRequest::Transaction(req) => self.apply_transaction(index, req).await?,
                RaftClientRequest::StreamPub(req) => self.apply_stream_pub(index, req).await?,
                RaftClientRequest::StreamSub(req) => self.apply_stream_sub(index, req).await?,
                RaftClientRequest::StreamUnsub(req) => self.apply_stream_unsub(index, req).await?,
                RaftClientRequest::PipelineStageSub(req) => self.apply_pipeline_stage_sub(index, req).await?,
                RaftClientRequest::UpdateSchema(req) => self.apply_update_schema(index, req).await?,
            };
        }
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
        // https://github.com/spacejam/sled/issues/1198
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
        let log = self.log.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            // Build an atomic batch of delete operations.
            let mut batch = sled::Batch::default();
            if let Some(stop) = delete_through {
                for key_res in log.range(utils::encode_u64(0)..utils::encode_u64(stop)).keys() {
                    let key = key_res.context(ERR_ITER_FAILURE)?;
                    batch.remove(key);
                }
            } else {
                for key_res in log.range(utils::encode_u64(0)..).keys() {
                    let key = key_res.context(ERR_ITER_FAILURE)?;
                    batch.remove(key);
                }
            }
            // Insert the new snapshot pointer & apply the batch.
            batch.insert(&snapshot_entry_idx, snapshot_entry_enc.as_slice());
            log.apply_batch(batch).context("error applying batch log operations")?;
            Ok(())
        })
        .await
        .context(ERR_DB_TASK)??;
        self.log.flush_async().await.context(ERR_FLUSH)?;

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
    #[tracing::instrument(level = "trace", skip(self, index, req), err)]
    async fn apply_transaction(&self, index: &u64, req: &TransactionClient) -> Result<RaftClientResponse> {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, index, req), err)]
    async fn apply_stream_pub(&self, index: &u64, req: &StreamPubRequest) -> Result<RaftClientResponse> {
        tracing::info!(index, "applying stream pub request to stream");
        Ok(RaftClientResponse::StreamPub(StreamPubResponse { id: format!("{}", index) }))
    }

    #[tracing::instrument(level = "trace", skip(self, index, req), err)]
    async fn apply_stream_sub(&self, index: &u64, req: &StreamSubClient) -> Result<RaftClientResponse> {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, index, req), err)]
    async fn apply_stream_unsub(&self, index: &u64, req: &StreamUnsubRequest) -> Result<RaftClientResponse> {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, index, req), err)]
    async fn apply_pipeline_stage_sub(&self, index: &u64, req: &PipelineStageSubClient) -> Result<RaftClientResponse> {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, index, req), err)]
    async fn apply_update_schema(&self, index: &u64, req: &UpdateSchemaRequest) -> Result<RaftClientResponse> {
        todo!("")
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

    /// Recover the systems' user permissions state.
    #[tracing::instrument(level = "trace", skip(self), err)]
    pub async fn recover_user_permissions(&self) -> Result<HashMap<u64, UserRole>> {
        Ok(Default::default()) // TODO: recover auth state.
    }

    /// Recover the systems' token permissions state.
    #[tracing::instrument(level = "trace", skip(self), err)]
    pub async fn recover_token_permissions(&self) -> Result<HashMap<u64, Claims>> {
        let mut tokens = HashMap::new();
        tokens.insert(0, Claims::V1(ClaimsV1::All));
        Ok(tokens) // TODO: recover auth state.
    }
}

/// An update for data indices.
pub enum IndexUpdate {}
