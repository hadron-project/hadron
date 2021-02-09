#![allow(clippy::unit_arg)]
#![allow(unused_imports)] // TODO: remove this.
#![allow(unused_variables)] // TODO: remove this.
#![allow(unused_mut)] // TODO: remove this.
#![allow(dead_code)] // TODO: remove this.
#![allow(clippy::redundant_clone)] // TODO: remove this.

mod index;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_raft::async_trait::async_trait;
use async_raft::raft::{Entry, EntryPayload, MembershipConfig};
use async_raft::storage::{CurrentSnapshotData, HardState, InitialState};
use async_raft::RaftStorage;
use models::{SchemaBranch, SchemaUpdate};
use prost::Message;
use sled::{transaction::ConflictableTransactionResult as TxResult, Batch, Config as SledConfig, Db, IVec, Transactional, Tree};
use tokio::fs::File;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, RwLock};
use trust_dns_resolver::Name;

use crate::auth::{Claims, ClaimsV1, TokenCredentials, User, UserRole};
use crate::config::Config;
use crate::ctl_placement::models as placement_models;
use crate::ctl_raft::events::{CRCEvent, InitialEvent, PipelineCreated, PipelineReplicaCreated, StreamCreated, StreamReplicaCreated};
use crate::ctl_raft::network::{RaftClientRequest, RaftClientResponse};
use crate::ctl_raft::storage::index::{IndexWriteBatch, IndexWriteOp, NamespaceIndex};
use crate::database::Database;
use crate::error::{AppError, ShutdownError, ShutdownResult};
use crate::models::{self, Namespaced};
use crate::proto::client::{PipelineStageSubClient, StreamUnsubRequest};
use crate::proto::client::{StreamPubRequest, StreamPubResponse, StreamSubClient};
use crate::proto::client::{TransactionClient, UpdateSchemaRequest, UpdateSchemaResponse};
use crate::proto::storage as proto_storage;
use crate::utils;
use crate::NodeId;

pub use index::HCoreIndex;

// Storage paths.
const CRC_SNAPS_DIR: &str = "crc_snaps"; // <dataDir>/crc_snaps

// DB prefixes.
const PREFIX_NAMESPACE: &str = "/namespaces/";
const PREFIX_STREAMS: &str = "/streams/";
const PREFIX_STREAM_REPLICAS: &str = "/stream_replicas/";
const PREFIX_PIPELINES: &str = "/pipelines/";
const PREFIX_PIPELINE_REPLICAS: &str = "/pipeline_replicas/";
const PREFIX_BRANCHES: &str = "/branches/";

// DB keys.
const KEY_HARD_STATE: &str = "/meta/hard_state";
const KEY_LAST_APPLIED_LOG: &str = "/meta/last_applied_log";

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
    /// The database manager.
    db: Database,
    /// This controllers DB Tree used for its Raft log.
    log: Tree,
    /// This controllers DB Tree used for its Raft state machine.
    state: Tree,

    /// The system data index.
    ///
    /// Each individual field of the index must be independently locked for reading/writing. It is
    /// important to note that we must reduce lock contention as much as possible. This can easily
    /// be achieved by simply dropping the lock guard as soon as possible whenever a lock is taken.
    index: Arc<HCoreIndex>,
    /// A channel used for sending CRC commands.
    events: mpsc::UnboundedSender<CRCEvent>,
}

impl HCoreStorage {
    /// Create a new instance.
    ///
    /// This constructor is fallible as it will attempt to recover state from disk when first
    /// initialized.
    pub async fn new(id: NodeId, config: Arc<Config>, db: Database) -> Result<(Self, Arc<HCoreIndex>, mpsc::UnboundedReceiver<CRCEvent>)> {
        // Ensure the needed paths are in place for the DB and other needed paths.
        let snapshot_dir = PathBuf::from(&config.storage_data_path).join(CRC_SNAPS_DIR);
        tokio::fs::create_dir_all(&snapshot_dir)
            .await
            .context("error creating dir for hadron core database snapshots")?;

        // Build the CRC command channel.
        let (events, events_rx) = mpsc::unbounded_channel();

        // Recover initial system state, building indices &c. After state is recovered, emit the
        // initial CRC event describing the starting state of this controller.
        let log = db.crc_log().await?;
        let state = db.crc_sm().await?;
        let (index, initial_event) = Self::recover_system_state(&state).await?;
        let index = Arc::new(index);
        let _ = events.send(CRCEvent::Initial(initial_event));

        let this = Self {
            id,
            _config: config,
            snapshot_dir,
            db,
            log,
            state,
            index: index.clone(),
            events,
        };
        Ok((this, index, events_rx))
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
        let log = self.log.clone();
        let cfg = Database::spawn_blocking(move || -> Result<Option<MembershipConfig>> {
            for log_entry_res in log.iter().values().rev() {
                let log_entry = log_entry_res.context(ERR_ITER_FAILURE)?;
                let entry: Entry<RaftClientRequest> = utils::decode_flexbuf(&log_entry).context(ERR_DECODE_LOG_ENTRY)?;
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
        let (log, id) = (self.log.clone(), self.id);
        let log0 = log.clone();
        let pristine_opt = Database::spawn_blocking(move || -> Result<Option<InitialState>> {
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
        let tree = self.state.clone();
        let membership = self.get_membership_config().await?;
        let state = Database::spawn_blocking(move || -> Result<InitialState> {
            // Get hard state.
            let hs: HardState = tree
                .get(KEY_HARD_STATE)
                .context("error getting hard state from storage")?
                .map(|val| utils::decode_flexbuf(val.as_ref()).context("error decoding hard state"))
                .ok_or_else(|| anyhow!("no hard state record found on disk"))??;
            // Get last log info.
            let last_log: Entry<RaftClientRequest> = log
                .last()?
                .ok_or_else(|| anyhow!("error fetching last entry of log, though first entry exists"))
                .map(|(_, val)| utils::decode_flexbuf(&val).context(ERR_DECODE_LOG_ENTRY))??;
            // Get last applied log index.
            let last_applied_log_index = tree
                .get(KEY_LAST_APPLIED_LOG)
                .context("error fetching last applied log index")?
                .map(|raw| utils::decode_u64(raw.as_ref()).context("failed to decode last applied log index"))
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
        let raw = utils::encode_flexbuf(hs).context("error encoding hard state")?;
        let tree = self.state.clone();
        Database::spawn_blocking(move || -> Result<()> {
            tree.insert(KEY_HARD_STATE, raw).context("error saving hard state to disk")?;
            tree.flush().context(ERR_FLUSH)?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    /// Get a series of log entries from storage.
    ///
    /// The start value is inclusive in the search and the stop value is non-inclusive: `[start, stop)`.
    #[tracing::instrument(level = "trace", skip(self, start, stop), err)]
    async fn get_log_entries(&self, start: u64, stop: u64) -> Result<Vec<Entry<RaftClientRequest>>> {
        let (start, stop) = (utils::encode_u64(start), utils::encode_u64(stop));
        let log = self.log.clone();
        let entries = Database::spawn_blocking(move || -> Result<Vec<Entry<RaftClientRequest>>> {
            let mut entries = vec![];
            for val_res in log.range(start..stop).values() {
                let val = val_res.context(ERR_ITER_FAILURE)?;
                let entry: Entry<RaftClientRequest> = utils::decode_flexbuf(&val).context(ERR_DECODE_LOG_ENTRY)?;
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
        let (log, start) = (self.log.clone(), utils::encode_u64(start));
        Database::spawn_blocking(move || -> Result<()> {
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
            log.flush().context(ERR_FLUSH)?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    /// Append a new entry to the log.
    #[tracing::instrument(level = "trace", skip(self, entry), err)]
    async fn append_entry_to_log(&self, entry: &Entry<RaftClientRequest>) -> Result<()> {
        let log = self.log.clone();
        let entry_key = utils::encode_u64(entry.index);
        let entry_bytes = utils::encode_flexbuf(&entry).context(ERR_ENCODE_LOG_ENTRY)?;
        Database::spawn_blocking(move || -> Result<()> {
            log.insert(entry_key, entry_bytes).context("error inserting log entry")?;
            log.flush().context(ERR_FLUSH)?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    /// Replicate a payload of entries to the log.
    ///
    /// Though the entries will always be presented in order, each entry's index should be used to
    /// determine its location to be written in the log.
    #[tracing::instrument(level = "trace", skip(self, entries), err)]
    async fn replicate_to_log(&self, entries: &[Entry<RaftClientRequest>]) -> Result<()> {
        // Prep data to be sent to thread for blocking op.
        let log = self.log.clone();
        let mut batch = Batch::default();
        for entry in entries {
            let entry_key = utils::encode_u64(entry.index);
            let entry_bytes = utils::encode_flexbuf(&entry).context(ERR_ENCODE_LOG_ENTRY)?;
            batch.insert(&entry_key, entry_bytes.as_slice());
        }
        // Apply insert batch.
        Database::spawn_blocking(move || -> Result<()> {
            log.apply_batch(batch).context("error applying batch insert to log for replication")?;
            log.flush().context(ERR_FLUSH)?;
            Ok(())
        })
        .await??;
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
            RaftClientRequest::UpdateSchema(inner) => self.apply_update_schema(index, &inner.validated, &inner.token_id).await,
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
        let (tree, index_val) = (self.state.clone(), utils::encode_u64(*index));
        let task_res = Database::spawn_blocking(move || -> ShutdownResult<()> {
            tree.insert(KEY_LAST_APPLIED_LOG, &index_val)
                .context("error updating last applied log index")
                .map(|_| ())?;
            tree.flush().context(ERR_FLUSH)?;
            Ok(())
        })
        .await
        .and_then(|res| res);
        if let Err(err) = task_res {
            tracing::error!(error = %err, "error updating last applied log");
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
                RaftClientRequest::UpdateSchema(inner) => self.apply_update_schema(index, &inner.validated, &inner.token_id).await,
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
            let (tree, index_val) = (self.state.clone(), utils::encode_u64(**index));
            Database::spawn_blocking(move || -> ShutdownResult<()> {
                tree.insert(KEY_LAST_APPLIED_LOG, &index_val)
                    .context("error updating last applied log index")
                    .map(|_| ())?;
                tree.flush().context(ERR_FLUSH)?;
                Ok(())
            })
            .await
            .and_then(|res| res)?;
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
        let snapshot_entry_enc = utils::encode_flexbuf(&snapshot_entry).context(ERR_ENCODE_LOG_ENTRY)?;
        let snapshot_entry_idx = utils::encode_u64(snapshot_entry.index);
        let (log, start) = (self.log.clone(), utils::encode_u64(0));
        let needs_recovery = Database::spawn_blocking(move || -> Result<bool> {
            // Build an atomic batch of delete operations.
            let needs_recovery: bool;
            let mut batch = Batch::default();
            let iter = if let Some(stop) = delete_through {
                needs_recovery = false;
                log.range(start..utils::encode_u64(stop))
            } else {
                needs_recovery = true;
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
            log.flush().context(ERR_FLUSH)?;
            Ok(needs_recovery)
        })
        .await??;

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

        // If the system was just completely re-built from a snapshot, then we need to
        // re-build the index & emit a new Initial state event.
        if needs_recovery {
            let (index, initial_event) = Self::recover_system_state(&self.state).await?;
            self.index.replace(index);
            let _ = self.events.send(CRCEvent::Initial(initial_event));
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

    /// Apply a schema update to the system.
    #[tracing::instrument(level = "trace", skip(self, index, req, token_id))]
    async fn apply_update_schema(&self, index: &u64, req: &models::SchemaUpdate, token_id: &u64) -> Result<RaftClientResponse> {
        tracing::debug!(index, "applying update schema request to system");

        // If this is a managed schema change, then we check the branch and timestamp to see if
        // the schema change has already been applied to the system.
        let (statements, branch, timestamp) = match &req {
            models::SchemaUpdate::Managed(inner) => (&inner.statements, Some(&inner.branch), Some(inner.timestamp)),
            models::SchemaUpdate::OneOff(inner) => (&inner.statements, None, None),
        };
        if let (Some(branch), Some(timestamp)) = (branch, timestamp) {
            if let Some(last_applied) = self.index.schema_branches.get(branch).map(|val| *val.value()) {
                if last_applied >= timestamp {
                    return Ok(RaftClientResponse::UpdateSchema(UpdateSchemaResponse { was_noop: true }));
                }
            }
        }

        // Perform full dynamic validation of this content, checking indices, and then falling back to
        // checking other statements in the given document.
        // TODO: ^^^

        // First we prep all needed batches & buffers used for atomically updating the state
        // machine, indexes & event stream.
        let mut batch = Batch::default();
        let mut index_batch = IndexWriteBatch::default();
        let mut events = Vec::with_capacity(statements.len());

        // Record the last applied log index as first part of the batch.
        batch.insert(KEY_LAST_APPLIED_LOG.as_bytes(), &utils::encode_u64(*index));

        // Now we iterate over our schema statements and perform their corresponding actions
        // operating only on our batches, which will be transactionally applied below.
        for statement in statements {
            match statement {
                models::SchemaStatement::Namespace(namespace) => self.create_namespace(namespace, &mut batch, &mut index_batch).await?,
                models::SchemaStatement::Stream(stream) => self.create_stream(stream, &mut batch, &mut index_batch, &mut events).await?,
                models::SchemaStatement::Pipeline(pipeline) => self.create_pipeline(pipeline, &mut batch, &mut index_batch, &mut events).await?,
                models::SchemaStatement::Endpoint(endpoint) => todo!("finish up endpoint creation"),
            }
        }

        // Record the branch name & timestamp to disk & index if applicable.
        if let (Some(branch), Some(timestamp)) = (branch, timestamp) {
            self.update_schema_branch(branch, timestamp, &mut batch, &mut index_batch)?;
        }

        // Apply the batch of changes.
        let sm = self.state.clone();
        Database::spawn_blocking(move || -> ShutdownResult<()> {
            sm.apply_batch(batch).context("error applying batch to CRC Raft state machine")?;
            sm.flush().context("error flushing CRC Raft state machine updates")?;
            Ok(())
        })
        .await??;

        // Finally apply the index batch & publish events.
        self.index.apply_batch(index_batch).await?;
        events.into_iter().for_each(|event| {
            let _ = self.events.send(event);
        });
        Ok(RaftClientResponse::UpdateSchema(UpdateSchemaResponse { was_noop: false }))
    }

    /// Create a new namespace in the database.
    #[tracing::instrument(level = "trace", skip(self, ns, models, index_batch))]
    async fn create_namespace(&self, ns: &models::Namespace, models: &mut Batch, index_batch: &mut IndexWriteBatch) -> Result<()> {
        // Check if the namespace already exists. If so, done.
        if self.index.get_namespace(&ns.name).is_some() {
            return Ok(());
        }
        // Namespace does not yet exist, so we create an entry for it.
        let proto = utils::encode_flexbuf(&ns)?;
        let nskey = format!("{}{}", PREFIX_NAMESPACE, ns.name.as_str());
        models.insert(nskey.as_bytes(), proto.as_slice());
        index_batch.push(IndexWriteOp::InsertNamespace {
            name: ns.name.clone(),
            description: ns.description.clone(),
        });
        Ok(())
    }

    /// Create a new stream in the database.
    #[tracing::instrument(level = "trace", skip(self, stream, models, index_batch, events))]
    async fn create_stream(
        &self, stream: &models::Stream, models: &mut Batch, index_batch: &mut IndexWriteBatch, events: &mut Vec<CRCEvent>,
    ) -> Result<()> {
        // Check if the stream already exists. If so, done.
        if self.index.get_stream(stream.namespace(), stream.name()).is_some() {
            return Ok(());
        }
        // Stream does not yet exist, so we create an entry for it, setting its initial index to 0.
        let model = utils::encode_flexbuf(&stream)?;
        let stream_key = format!("{}{}/{}", PREFIX_STREAMS, stream.namespace(), stream.name());
        models.insert(stream_key.as_bytes(), model);
        index_batch.push(IndexWriteOp::InsertStream { stream: stream.clone() });
        // Push a new event onto the event buf.
        events.push(CRCEvent::StreamCreated(StreamCreated { stream: stream.clone() }));

        // Create stream replica records.
        for partition in 0..stream.partitions() {
            for replica in 0..stream.replication_factor() {
                // Create stream replica in an unassigned state & add it to the write batch.
                let id = self.db.generate_id()?;
                let replica_key = format!(
                    "{}{}/{}/{}/{}",
                    PREFIX_STREAM_REPLICAS,
                    stream.namespace(),
                    stream.name(),
                    partition,
                    replica
                );
                let replica_model = placement_models::StreamReplica::new(id, stream.namespace(), stream.name(), partition, replica);
                let model = utils::encode_flexbuf(&replica_model).context("error encoding stream replica model")?;
                models.insert(replica_key.as_bytes(), model.as_slice());

                // Push a new event onto the event buf.
                events.push(CRCEvent::StreamReplicaCreated(StreamReplicaCreated { replica: replica_model }));
            }
        }

        Ok(())
    }

    /// Create a new pipeline in the database.
    #[tracing::instrument(level = "trace", skip(self, pipeline, models, index_batch, events))]
    async fn create_pipeline(
        &self, pipeline: &models::Pipeline, models: &mut Batch, index_batch: &mut IndexWriteBatch, events: &mut Vec<CRCEvent>,
    ) -> Result<()> {
        // Check if the pipeline already exists. If so, done.
        if self.index.get_pipeline(&pipeline.metadata.namespace, &pipeline.metadata.name).is_some() {
            return Ok(());
        }
        // Stream does not yet exist, so we create an entry for it, setting its initial index to 0.
        let model = utils::encode_flexbuf(&pipeline)?;
        let pipeline_key = format!("{}{}/{}", PREFIX_PIPELINES, pipeline.namespace(), pipeline.name());
        models.insert(pipeline_key.as_bytes(), model);
        index_batch.push(IndexWriteOp::InsertPipeline { pipeline: pipeline.clone() });
        // Push a new event onto the event buf.
        events.push(CRCEvent::PipelineCreated(PipelineCreated { pipeline: pipeline.clone() }));

        // Create pipeline replica records.
        for replica in 0..pipeline.replication_factor {
            // Create stream replica in an unassigned state & add it to the write batch.
            let id = self.db.generate_id()?;
            let replica_key = format!("{}{}/{}/{}", PREFIX_PIPELINE_REPLICAS, pipeline.namespace(), pipeline.name(), replica);
            let replica_model = placement_models::PipelineReplica::new(id, pipeline.namespace(), pipeline.name(), replica);
            let model = utils::encode_flexbuf(&replica_model).context("error encoding pipeline replica model")?;
            models.insert(replica_key.as_bytes(), model.as_slice());

            // Push a new event onto the event buf.
            events.push(CRCEvent::PipelineReplicaCreated(PipelineReplicaCreated { replica: replica_model }));
        }

        Ok(())
    }

    /// Update a schema branch.
    #[tracing::instrument(level = "trace", skip(self, branch, timestamp, branches, index_batch))]
    fn update_schema_branch(&self, branch: &str, timestamp: i64, branches: &mut Batch, index_batch: &mut IndexWriteBatch) -> Result<()> {
        let model = utils::encode_flexbuf(&models::SchemaBranch {
            name: branch.into(),
            timestamp,
        })
        .context("error encoding schema branch model")?;
        let key = format!("{}{}", PREFIX_BRANCHES, branch);
        branches.insert(key.as_bytes(), model);
        index_batch.push(IndexWriteOp::UpdateSchemaBranch {
            branch: branch.to_string(),
            timestamp,
        });
        Ok(())
    }

    //////////////////////////////////////////////////////////////////////////////////////////////
    // System State Recovery /////////////////////////////////////////////////////////////////////

    /// Recover the system's state, building indices and the like.
    #[tracing::instrument(level = "trace", skip(state), err)]
    pub async fn recover_system_state(state: &Tree) -> Result<(HCoreIndex, InitialEvent)> {
        // Perform parallel recovery of system state.
        let (users, tokens, namespaces, streams, stream_replicas, pipelines, pipeline_replicas, branches) = tokio::try_join!(
            Self::recover_user_permissions(&state),
            Self::recover_token_permissions(&state),
            Self::recover_namespaces(&state),
            Self::recover_streams(&state),
            Self::recover_stream_replicas(&state),
            Self::recover_pipelines(&state),
            Self::recover_pipeline_replicas(&state),
            Self::recover_schema_branches(&state),
        )
        .context("error during parallel state recovery of CRC data")?;

        // Update initial CRC event with recovered data.
        let mut initial_event = InitialEvent::new(streams.clone(), stream_replicas, pipelines.clone(), pipeline_replicas);

        // Update index with recovered data.
        let index = HCoreIndex::new(users, tokens, namespaces, streams, pipelines, branches);

        Ok((index, initial_event))
    }

    /// Recover namespaces.
    #[tracing::instrument(level = "trace", skip(state), err)]
    pub async fn recover_namespaces(state: &Tree) -> Result<Vec<models::Namespace>> {
        let tree = state.clone();
        let data = Database::spawn_blocking(move || -> Result<Vec<models::Namespace>> {
            let mut data = vec![];
            for entry_res in tree.scan_prefix(PREFIX_NAMESPACE) {
                let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
                let model: models::Namespace = utils::decode_flexbuf(entry.as_ref()).context("error decoding namespace from storage")?;
                data.push(model);
            }
            Ok(data)
        })
        .await??;
        tracing::debug!(count = data.len(), "recovered namespaces");
        Ok(data)
    }

    /// Recover streams.
    #[tracing::instrument(level = "trace", skip(state), err)]
    pub async fn recover_streams(state: &Tree) -> Result<Vec<models::Stream>> {
        let tree = state.clone();
        let data = Database::spawn_blocking(move || -> Result<Vec<models::Stream>> {
            let mut data = vec![];
            for entry_res in tree.scan_prefix(PREFIX_STREAMS) {
                let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
                let model: models::Stream = utils::decode_flexbuf(entry.as_ref()).context("error decoding stream from storage")?;
                data.push(model);
            }
            Ok(data)
        })
        .await??;
        tracing::debug!(count = data.len(), "recovered streams");
        Ok(data)
    }

    /// Recover stream replicas.
    #[tracing::instrument(level = "trace", skip(state), err)]
    pub async fn recover_stream_replicas(state: &Tree) -> Result<Vec<placement_models::StreamReplica>> {
        let tree = state.clone();
        let data = Database::spawn_blocking(move || -> Result<Vec<placement_models::StreamReplica>> {
            let mut data = vec![];
            for entry_res in tree.scan_prefix(PREFIX_STREAM_REPLICAS) {
                let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
                let model: placement_models::StreamReplica =
                    utils::decode_flexbuf(entry.as_ref()).context("error decoding stream replica from storage")?;
                data.push(model);
            }
            Ok(data)
        })
        .await??;
        tracing::debug!(count = data.len(), "recovered stream replicas");
        Ok(data)
    }

    /// Recover pipelines.
    #[tracing::instrument(level = "trace", skip(state), err)]
    pub async fn recover_pipelines(state: &Tree) -> Result<Vec<models::Pipeline>> {
        let tree = state.clone();
        let data = Database::spawn_blocking(move || -> Result<Vec<models::Pipeline>> {
            let mut data = vec![];
            for entry_res in tree.scan_prefix(PREFIX_PIPELINES) {
                let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
                let model: models::Pipeline = utils::decode_flexbuf(entry.as_ref()).context("error decoding pipeline from storage")?;
                data.push(model);
            }
            Ok(data)
        })
        .await??;
        tracing::debug!(count = data.len(), "recovered pipelines");
        Ok(data)
    }

    /// Recover pipeline replicas.
    #[tracing::instrument(level = "trace", skip(state), err)]
    pub async fn recover_pipeline_replicas(state: &Tree) -> Result<Vec<placement_models::PipelineReplica>> {
        let tree = state.clone();
        let data = Database::spawn_blocking(move || -> Result<Vec<placement_models::PipelineReplica>> {
            let mut data = vec![];
            for entry_res in tree.scan_prefix(PREFIX_PIPELINE_REPLICAS) {
                let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
                let model: placement_models::PipelineReplica =
                    utils::decode_flexbuf(entry.as_ref()).context("error decoding pipeline replica from storage")?;
                data.push(model);
            }
            Ok(data)
        })
        .await??;
        tracing::debug!(count = data.len(), "recovered pipeline replicas");
        Ok(data)
    }

    /// Recover pipeline replicas.
    #[tracing::instrument(level = "trace", skip(state), err)]
    pub async fn recover_schema_branches(state: &Tree) -> Result<Vec<models::SchemaBranch>> {
        let tree = state.clone();
        let data = Database::spawn_blocking(move || -> Result<Vec<models::SchemaBranch>> {
            let mut data = vec![];
            for entry_res in tree.scan_prefix(PREFIX_BRANCHES) {
                let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
                let model: models::SchemaBranch = utils::decode_flexbuf(entry.as_ref()).context("error decoding schema branch from storage")?;
                data.push(model);
            }
            Ok(data)
        })
        .await??;
        tracing::debug!(count = data.len(), "recovered schema branches");
        Ok(data)
    }

    /// Recover the system's user permissions state.
    #[tracing::instrument(level = "trace", skip(state), err)]
    pub async fn recover_user_permissions(state: &Tree) -> Result<Vec<User>> {
        Ok(Default::default()) // TODO: recover user state.
    }

    /// Recover the system's token permissions state.
    #[tracing::instrument(level = "trace", skip(state), err)]
    pub async fn recover_token_permissions(state: &Tree) -> Result<Vec<(u64, Claims)>> {
        Ok(vec![(0, Claims::V1(ClaimsV1::All))]) // TODO: recover token state.
    }

    /// Get the given token's claims, else return an auth error.
    pub async fn must_get_token_claims(&self, token_id: &u64) -> Result<Arc<Claims>> {
        match self.index.tokens.get(token_id).map(|val| val.value().clone()) {
            Some(claims) => Ok(claims),
            None => Err(AppError::UnknownToken.into()),
        }
    }
}
