//! A generic wrapper around the async-raft crate.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use async_raft::async_trait::async_trait;
use async_raft::raft::{Entry, EntryPayload, MembershipConfig};
use async_raft::storage::{CurrentSnapshotData, HardState, InitialState};
use async_raft::RaftStorage;
use prost::Message;
use sled::{Batch, Tree};
use tokio::fs::File;
use tokio::stream::StreamExt;

use crate::database::Database;
use crate::error::{ShutdownError, ShutdownResult};
use crate::proto::storage as proto_storage;
use crate::utils;
use crate::NodeId;

// DB keys.
const KEY_HARD_STATE: &str = "/meta/hard_state";
const KEY_LAST_APPLIED_LOG: &str = "/meta/last_applied_log";

// Error messages.
const ERR_ENCODE_LOG_ENTRY: &str = "error encoding raft log entry";
const ERR_DECODE_LOG_ENTRY: &str = "error decoding raft log entry";
const ERR_FLUSH: &str = "error flushing to disk";
const ERR_ITER_FAILURE: &str = "error returned during key/value iteration from database";
const ERR_READING_SNAPS_DIR: &str = "error reading snapshots dir";
const ERR_ITER_SNAPS_DIR: &str = "error iterating snapshot file entries";

/// An object used to provide generic Sled-based storage backed by the wrapped provider.
pub struct RaftStorageSled<T: SledStorageProvider>(pub T);

/// Sled storage provider functionality needed for implementing RaftStorage support via Sled.
#[async_trait]
pub trait SledStorageProvider: Send + Sync + 'static {
    /// The Raft request data type used by this implementation.
    type Req: async_raft::AppData;
    /// The Raft response data type used by this implementation.
    type Res: async_raft::AppDataResponse;

    /// Get the node ID of this Raft instance.
    fn get_id(&self) -> NodeId;
    /// Get a handle to the database Tree used for the Raft log.
    fn get_log(&self) -> Tree;
    /// Get a handle to the database Tree used for the Raft state machine.
    fn get_sm(&self) -> Tree;
    /// The path to this Raft's snapshot dir.
    fn snapshot_dir(&self) -> &Path;
    /// Recover system state when first initialized or after snapshot installation.
    ///
    /// This hook is provided as a way to perform initialization, build indices, or any other such
    /// procedures which might be needed when a new state is installed.
    async fn recover_system_state(&self) -> Result<()>;
    /// The internal business logic implementation of applying an entry to the state machine.
    async fn do_apply_entry_to_state_machine(&self, index: &u64, data: &Self::Req) -> Result<Self::Res>;
}

#[async_trait]
impl<T> RaftStorage<T::Req, T::Res> for RaftStorageSled<T>
where
    T: SledStorageProvider,
{
    /// The storage engine's associated type used for exposing a snapshot for reading & writing.
    type Snapshot = File;
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
        let log = self.0.get_log();
        let cfg = Database::spawn_blocking(move || -> Result<Option<MembershipConfig>> {
            for log_entry_res in log.iter().values().rev() {
                let log_entry = log_entry_res.context(ERR_ITER_FAILURE)?;
                let entry: Entry<T::Req> = utils::decode_flexbuf(&log_entry).context(ERR_DECODE_LOG_ENTRY)?;
                match entry.payload {
                    EntryPayload::ConfigChange(cfg) => return Ok(Some(cfg.membership)),
                    _ => continue,
                }
            }
            Ok(None)
        })
        .await??
        .unwrap_or_else(|| MembershipConfig::new_initial(self.0.get_id()));
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
        let (log, id) = (self.0.get_log(), self.0.get_id());
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
        let tree = self.0.get_sm();
        let membership = self.get_membership_config().await?;
        let state = Database::spawn_blocking(move || -> Result<InitialState> {
            // Get hard state.
            let hs: HardState = tree
                .get(KEY_HARD_STATE)
                .context("error getting hard state from storage")?
                .map(|val| utils::decode_flexbuf(val.as_ref()).context("error decoding hard state"))
                .ok_or_else(|| anyhow!("no hard state record found on disk"))??;
            // Get last log info.
            let last_log: Entry<T::Req> = log
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
        let tree = self.0.get_sm();
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
    async fn get_log_entries(&self, start: u64, stop: u64) -> Result<Vec<Entry<T::Req>>> {
        let (start, stop) = (utils::encode_u64(start), utils::encode_u64(stop));
        let log = self.0.get_log();
        let entries = Database::spawn_blocking(move || -> Result<Vec<Entry<T::Req>>> {
            let mut entries = vec![];
            for val_res in log.range(start..stop).values() {
                let val = val_res.context(ERR_ITER_FAILURE)?;
                let entry: Entry<T::Req> = utils::decode_flexbuf(&val).context(ERR_DECODE_LOG_ENTRY)?;
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
        let (log, start) = (self.0.get_log(), utils::encode_u64(start));
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
    async fn append_entry_to_log(&self, entry: &Entry<T::Req>) -> Result<()> {
        let log = self.0.get_log();
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
    async fn replicate_to_log(&self, entries: &[Entry<T::Req>]) -> Result<()> {
        // Prep data to be sent to thread for blocking op.
        let log = self.0.get_log();
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
    async fn apply_entry_to_state_machine(&self, index: &u64, data: &T::Req) -> Result<T::Res> {
        let res = self.0.do_apply_entry_to_state_machine(index, data).await;
        let err = match res {
            Ok(res) => return Ok(res),
            Err(err) => err,
        };
        // If error is a shutdown error, then we need to propagate immediately.
        if err.downcast_ref::<ShutdownError>().is_some() {
            return Err(err);
        }
        // Else, we will write the last applied log.
        let (tree, index_val) = (self.0.get_sm(), utils::encode_u64(*index));
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
    async fn replicate_to_state_machine(&self, entries: &[(&u64, &T::Req)]) -> Result<()> {
        for (index, data) in entries {
            let res = self.0.do_apply_entry_to_state_machine(index, data).await;
            let err = match res {
                Ok(_) => continue,
                Err(err) => err,
            };
            // If error is a shutdown error, then we need to propagate immediately.
            if err.downcast_ref::<ShutdownError>().is_some() {
                return Err(err);
            }
            // Else, we will write the last applied log.
            let (tree, index_val) = (self.0.get_sm(), utils::encode_u64(**index));
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
        let snappath = self.0.snapshot_dir().join(&snapname);
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
        let metadata = match extract_snapshot_metadata(*snapshot).await? {
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
        let snapshot_entry = Entry::<T::Req>::new_snapshot_pointer(index, term, id.clone(), membership);
        let snapshot_entry_enc = utils::encode_flexbuf(&snapshot_entry).context(ERR_ENCODE_LOG_ENTRY)?;
        let snapshot_entry_idx = utils::encode_u64(snapshot_entry.index);
        let (log, start) = (self.0.get_log(), utils::encode_u64(0));
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
        let old_path = self.0.snapshot_dir().join(&format!("{}.snap.tmp", id));
        let new_path = self.0.snapshot_dir().join(&format!("{}.snap", id));
        tokio::fs::rename(old_path, new_path).await.context("error updating snapshot name")?;

        // Ensure any old snapshots are pruned as they now no longer have any pointers.
        let mut dir = tokio::fs::read_dir(&self.0.snapshot_dir()).await.context(ERR_READING_SNAPS_DIR)?;
        while let Some(snap_res) = dir.next().await {
            let snap = snap_res.context(ERR_ITER_SNAPS_DIR)?;
            let snappath = snap.path();
            if snap.file_name() == id.as_str() {
                continue;
            }
            tracing::trace!(path = ?snappath, "removing old snapshot file");
            tokio::fs::remove_file(&snappath).await.context("error removing old snapshot")?;
        }

        // If the system was just completely re-built from a snapshot, then give an opportunity
        // to recover system state, building new indices &c.
        if needs_recovery {
            self.0.recover_system_state().await?;
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
        let mut dir = tokio::fs::read_dir(&self.0.snapshot_dir()).await.context(ERR_READING_SNAPS_DIR)?;
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
        let metadata = match extract_snapshot_metadata(snapfile).await? {
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

/// Extract the metadata of the given snapshot file.
#[tracing::instrument(level = "trace", skip(snap), err)]
async fn extract_snapshot_metadata(snap: File) -> Result<Option<proto_storage::SnapshotMetadata>> {
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
