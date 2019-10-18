use actix::prelude::*;
use actix_raft::{
    RaftStorage,
    messages::EntryConfigChange,
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
use log;
use sled::{self, Transactional};

use crate::{
    app::{AppData, AppDataError, AppDataResponse, RgEntry, RgEntryNormal, RgEntryPayload},
    db::{
        db_sled::{
            SledStorage,
            consts,
        },
        models::{
            STREAM_NAME_PATTERN,
            Stream, StreamEntry, StreamType, StreamVisibility, StreamWrapper,
        },
    },
    proto::client,
    utils,
};

pub(super) type RgAppendEntryToLog = AppendEntryToLog<AppData, AppDataError>;
pub(super) type RgApplyEntryToStateMachine = ApplyEntryToStateMachine<AppData, AppDataResponse, AppDataError>;
pub(super) type RgCreateSnapshot = CreateSnapshot<AppDataError>;
pub(super) type RgGetCurrentSnapshot = GetCurrentSnapshot<AppDataError>;
pub(super) type RgGetInitialState = GetInitialState<AppDataError>;
pub(super) type RgGetLogEntries = GetLogEntries<AppData, AppDataError>;
pub(super) type RgInstallSnapshot = InstallSnapshot<AppDataError>;
pub(super) type RgReplicateToLog = ReplicateToLog<AppData, AppDataError>;
pub(super) type RgReplicateToStateMachine = ReplicateToStateMachine<AppData, AppDataError>;
pub(super) type RgSaveHardState = SaveHardState<AppDataError>;

impl RaftStorage<AppData, AppDataResponse, AppDataError> for SledStorage {
    type Actor = Self;
    type Context = Context<Self>;
}

//////////////////////////////////////////////////////////////////////////////
// RaftStorage RgAppendEntryToLog ////////////////////////////////////////////

impl Handler<RgAppendEntryToLog> for SledStorage {
    type Result = ResponseActFuture<Self, (), AppDataError>;

    fn handle(&mut self, msg: RgAppendEntryToLog, _ctx: &mut Self::Context) -> Self::Result {
        // Validate the given payload before writing it to the log.
        match &msg.entry.payload {
            RgEntryPayload::SnapshotPointer(_) => {
                log::error!("Received a request to write a snapshot pointer to the log. This should never happen.");
                return Box::new(fut::err(AppDataError::Internal));
            }
            RgEntryPayload::Blank | RgEntryPayload::ConfigChange(_) => (), // No validation needed on these variants.
            RgEntryPayload::Normal(entry) => if let Err(err) = self.validate_pre_log_entry(entry, msg.entry.index) {
                return Box::new(fut::err(err));
            }
        }

        // Entry checks out, send it over to be written to the log.
        let res = bincode::serialize(&*msg.entry).map_err(|err| {
            log::error!("Error serializing log entry: {}", err);
            AppDataError::Internal
        }).and_then(|data| {
            self.log.insert(msg.entry.index.to_be_bytes(), data.as_slice()).map_err(|err| {
                log::error!("Error serializing log entry: {}", err);
                AppDataError::Internal
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
    pub(super) fn validate_pre_log_entry(&mut self, entry: &RgEntryNormal, index: u64) -> Result<(), AppDataError> {
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
    fn validate_pub_stream(&self, entry: &client::PubStreamRequest) -> Result<(), AppDataError> {
        // Ensure the target stream exists.
        log::debug!("Validating pub stream request. {:?}", entry);
        if let None = self.indexed_streams.get(&format!("{}/{}", &entry.namespace, &entry.stream)) {
            return Err(AppDataError::new_unknown_stream(entry.namespace.clone(), entry.stream.clone()));
        }
        Ok(())
    }

    /// Validate the contents of a SubStreamRequest before it hits the log.
    fn validate_sub_stream(&self, _entry: &client::SubStreamRequest) -> Result<(), AppDataError> {
        Ok(())
    }

    /// Validate the contents of a SubPipelineRequest before it hits the log.
    fn validate_sub_pipeline(&self, _entry: &client::SubPipelineRequest) -> Result<(), AppDataError> {
        Ok(())
    }

    /// Validate the contents of a UnsubStreamRequest before it hits the log.
    fn validate_unsub_stream(&self, _entry: &client::UnsubStreamRequest) -> Result<(), AppDataError> {
        Ok(())
    }

    /// Validate the contents of a UnsubPipelineRequest before it hits the log.
    fn validate_unsub_pipeline(&self, _entry: &client::UnsubPipelineRequest) -> Result<(), AppDataError> {
        Ok(())
    }

    /// Validate the contents of a EnsureRpcEndpointRequest before it hits the log.
    fn validate_ensure_rpc_endpoint(&self, _entry: &client::EnsureRpcEndpointRequest) -> Result<(), AppDataError> {
        Ok(())
    }

    /// Validate the contents of a EnsureStreamRequest before it hits the log.
    fn validate_ensure_stream(&mut self, entry: &client::EnsureStreamRequest, index: u64) -> Result<(), AppDataError> {
        log::debug!("Validating ensure stream request. {:?}", entry);
        if !STREAM_NAME_PATTERN.is_match(&entry.name) {
            return Err(AppDataError::new_invalid_input(String::from("Stream names must match the pattern `[-_.a-zA-Z0-9]{1,100}`.")));
        }
        let fullname = format!("{}/{}", &entry.namespace, &entry.name);
        if let Some(_) = self.indexed_streams.get(&fullname) {
            return Err(AppDataError::TargetStreamExists);
        }
        if self.pending_streams.values().any(|e| e == &fullname) {
            return Err(AppDataError::TargetStreamExists);
        }
        self.pending_streams.insert(index, fullname);
        Ok(())
    }

    /// Validate the contents of a EnsurePipelineRequest before it hits the log.
    fn validate_ensure_pipeline(&self, _entry: &client::EnsurePipelineRequest) -> Result<(), AppDataError> {
        Ok(())
    }

    /// Validate the contents of a AckStreamRequest before it hits the log.
    fn validate_ack_stream(&self, _entry: &client::AckStreamRequest) -> Result<(), AppDataError> {
        Ok(())
    }

    /// Validate the contents of a AckPipelineRequest before it hits the log.
    fn validate_ack_pipeline(&self, _entry: &client::AckPipelineRequest) -> Result<(), AppDataError> {
        Ok(())
    }
}

//////////////////////////////////////////////////////////////////////////////
// RaftStorage RgReplicateToLog //////////////////////////////////////////////

impl Handler<RgReplicateToLog> for SledStorage {
    type Result = ResponseActFuture<Self, (), AppDataError>;

    fn handle(&mut self, msg: RgReplicateToLog, _ctx: &mut Self::Context) -> Self::Result {
        let mut batch = sled::Batch::default();
        let res = msg.entries.iter().try_for_each(|entry| {
            let data = bincode::serialize(entry).map_err(|err| {
                log::error!("Error serializing log entry: {}", err);
                AppDataError::Internal
            })?;
            batch.insert(&entry.index.to_be_bytes(), data.as_slice());
            Ok(())
        }).and_then(|_| {
            self.log.apply_batch(batch).map_err(|err| {
                log::error!("Error applying batch of Raft log entries to storage: {}", err);
                AppDataError::Internal
            })
        });
        Box::new(fut::result(res))
    }
}

//////////////////////////////////////////////////////////////////////////////
// RaftStorage RgApplyEntryToStateMachine & RgReplicateToStateMachine ////////

impl Handler<RgApplyEntryToStateMachine> for SledStorage {
    type Result = ResponseActFuture<Self, AppDataResponse, AppDataError>;

    fn handle(&mut self, msg: RgApplyEntryToStateMachine, _ctx: &mut Self::Context) -> Self::Result {
        Box::new(fut::result(self.apply_entry_to_state_machine(&*msg.payload)))
    }
}

impl Handler<RgReplicateToStateMachine> for SledStorage {
    type Result = ResponseActFuture<Self, (), AppDataError>;

    fn handle(&mut self, msg: RgReplicateToStateMachine, _ctx: &mut Self::Context) -> Self::Result {
        let res = msg.payload.iter().try_for_each(|entry| {
            self.apply_entry_to_state_machine(entry).map(|_| ())
        });
        Box::new(fut::result(res))
    }
}

impl SledStorage {
    /// Apply the given entry to the state machine.
    fn apply_entry_to_state_machine(&mut self, entry: &RgEntry) -> Result<AppDataResponse, AppDataError> {
        let res = match &entry.payload {
            RgEntryPayload::SnapshotPointer(_) => {
                log::error!("Received a request to write a snapshot pointer to state machine. This should never happen.");
                return Err(AppDataError::Internal);
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
    fn apply_pub_stream(&mut self, req: &client::PubStreamRequest, log_index: u64) -> Result<AppDataResponse, AppDataError> {
        log::debug!("Applying pub stream request for {:?}.", &req);

        // Get a handle to the target stream object.
        let stream = self.indexed_streams.get_mut(&format!("{}/{}", &req.namespace, &req.stream)).ok_or_else(|| {
            log::error!("Error while applying entry to state machine for a PubStreamRequest. Target stream does not exist in index.");
            AppDataError::Internal
        })?;

        // Create a new stream entry and serialize it.
        let entry = StreamEntry{index: stream.next_index, data: req.payload.clone()};
        let entry_bytes = bincode::serialize(&entry).map_err(|err| {
            log::error!("Error serializing StreamEntry. {}", err);
            AppDataError::Internal
        })?;

        // Update the value of the stream's next index.
        stream.next_index += 1;

        // Write the entry, the stream's next index value and the LAL transactionally.
        (&stream.data, &stream.meta, &self.db as &sled::Tree).transaction(|(data, meta, db)| {
            data.insert(&entry.index.to_be_bytes(), entry_bytes.as_slice())?;
            meta.insert(consts::STREAM_NEXT_INDEX_KEY, &stream.next_index.to_be_bytes())?;
            db.insert(consts::RAFT_LAL_KEY, &log_index.to_be_bytes())?;
            Ok(())
        }).map_err(|err| {
            log::error!("Error writing stream entry. {:?}", err);
            AppDataError::Internal
        })?;

        Ok(AppDataResponse::PubStream{index: entry.index})
    }

    /// Apply the given EnsureStreamRequest to the state machine.
    ///
    /// When this routine starts, we know that the stream ns/name has been reserved in the
    /// `pending_streams` index, which will be cleared after this routine.
    fn apply_ensure_stream(&mut self, req: &client::EnsureStreamRequest, log_index: u64) -> Result<AppDataResponse, AppDataError> {
        log::debug!("Applying ensure stream request for {:?}.", &req);

        // Open new trees for the new stream.
        let fullname = format!("{}/{}", req.namespace, req.name);
        let data = self.db.open_tree(Self::stream_keyspace_data(&req.namespace, &req.name)).map_err(|err| {
            log::error!("Error opening tree for new stream's data. {}", err);
            AppDataError::Internal
        })?;
        let meta = self.db.open_tree(Self::stream_keyspace_metadata(&req.namespace, &req.name)).map_err(|err| {
            log::error!("Error opening tree for new stream's metadata. {}", err);
            AppDataError::Internal
        })?;

        // Build stream object & serialize.
        let stream = Stream{
            namespace: req.namespace.clone(),
            name: req.name.clone(),
            stream_type: StreamType::Standard,
            visibility: StreamVisibility::Namespace,
        };
        let stream_bytes = bincode::serialize(&stream).map_err(|err| {
            log::error!("{} {}", consts::ERR_SERIALIZE_STREAM, err);
            AppDataError::Internal
        })?;

        // Initialize next index value for new stream & update LAL.
        (&meta, &self.object_collections.streams_collection, &self.db as &sled::Tree).transaction(|(meta, streams, db)| {
            streams.insert(fullname.as_bytes(), stream_bytes.as_slice())?;
            meta.insert(consts::STREAM_NEXT_INDEX_KEY, &0u64.to_be_bytes())?;
            db.insert(consts::RAFT_LAL_KEY, &log_index.to_be_bytes())?;
            Ok(())
        }).map_err(|err| {
            log::error!("Error creating new stream. {:?}", err);
            AppDataError::Internal
        })?;

        // Add new stream to index.
        self.indexed_streams.insert(fullname, StreamWrapper{next_index: 0, stream, data, meta});
        Ok(AppDataResponse::EnsureStream)
    }

    /// Apply the new config to storage.
    fn apply_entry_config_change(&mut self, config: &EntryConfigChange, log_index: u64) -> Result<AppDataResponse, AppDataError> {
        // Serialize data.
        self.hs = HardState{
            current_term: self.hs.current_term,
            voted_for: self.hs.voted_for.clone(),
            membership: config.membership.clone(),
        };
        let hs = bincode::serialize(&self.hs).map_err(|err| {
            log::error!("{} {:?}", consts::ERR_SERIALIZE_HARD_STATE, err);
            AppDataError::Internal
        })?;

        // Write to disk.
        self.db.transaction(|db| {
            db.insert(consts::RAFT_HARDSTATE_KEY, hs.as_slice())?;
            db.insert(consts::RAFT_LAL_KEY, &log_index.to_be_bytes())?;
            Ok(())
        }).map_err(|err: sled::TransactionError<()>| {
            log::error!("{} {:?}", consts::ERR_WRITING_HARD_STATE, err);
            AppDataError::Internal
        })?;

        Ok(AppDataResponse::Noop)
    }

    /// Update the "last applied log" value.
    ///
    /// This should only be used for routines which do not need to atomically update other values
    /// in storage.
    fn update_lal(&self, log_index: u64) -> Result<(), AppDataError> {
        self.db.insert(consts::RAFT_LAL_KEY, &log_index.to_be_bytes())
            .map_err(|err| {
                log::error!("Error updating last applied log value. {}", err);
                AppDataError::Internal
            })
            .map(|_| ())
    }
}

//////////////////////////////////////////////////////////////////////////////
// RaftStorage RgGetInitialState /////////////////////////////////////////////

impl Handler<RgGetInitialState> for SledStorage {
    type Result = Result<InitialState, AppDataError>;

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
    type Result = ResponseActFuture<Self, Vec<RgEntry>, AppDataError>;

    fn handle(&mut self, msg: RgGetLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        Box::new(fut::wrap_future(self.sled.send(SyncGetLogEntries(msg, self.log.clone())))
            .map_err(|err, _: &mut Self, _| utils::app_data_error_from_mailbox_error(err, consts::ERR_DURING_DB_MSG))
            .and_then(|res, _, _| fut::result(res))
        )
    }
}

impl Handler<RgSaveHardState> for SledStorage {
    type Result = ResponseActFuture<Self, (), AppDataError>;

    fn handle(&mut self, msg: RgSaveHardState, _ctx: &mut Self::Context) -> Self::Result {
        // Serialize data.
        self.hs = msg.hs.clone();
        let res = bincode::serialize(&msg.hs).map_err(|err| {
            log::error!("{} {:?}", consts::ERR_SERIALIZE_HARD_STATE, err);
            AppDataError::Internal
        })
        // Write to disk.
        .and_then(|hs| {
            self.db.insert(consts::RAFT_HARDSTATE_KEY, hs).map_err(|err| {
                log::error!("{} {:?}", consts::ERR_WRITING_HARD_STATE, err);
                AppDataError::Internal
            })
            .map(|_| ())
        });
        Box::new(fut::result(res))
    }
}

impl Handler<RgCreateSnapshot> for SledStorage {
    type Result = ResponseActFuture<Self, CurrentSnapshotData, AppDataError>;

    fn handle(&mut self, _msg: RgCreateSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

impl Handler<RgGetCurrentSnapshot> for SledStorage {
    type Result = ResponseActFuture<Self, Option<CurrentSnapshotData>, AppDataError>;

    fn handle(&mut self, _msg: RgGetCurrentSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

impl Handler<RgInstallSnapshot> for SledStorage {
    type Result = ResponseActFuture<Self, (), AppDataError>;

    fn handle(&mut self, _msg: RgInstallSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// SledActor /////////////////////////////////////////////////////////////////////////////////////

/// Sync actor for interfacing with Sled.
pub(super) struct SledActor;

impl Actor for SledActor {
    type Context = SyncContext<Self>;
}

struct SyncGetLogEntries(RgGetLogEntries, sled::Tree);

impl Message for SyncGetLogEntries {
    type Result = Result<Vec<RgEntry>, AppDataError>;
}

impl Handler<SyncGetLogEntries> for SledActor {
    type Result = Result<Vec<RgEntry>, AppDataError>;

    fn handle(&mut self, msg: SyncGetLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        let start = msg.0.start.to_be_bytes();
        let stop = msg.0.stop.to_be_bytes();
        let entries = msg.1.range(start..stop).values().try_fold(vec![], |mut acc, res| {
            let data = res.map_err(|err| {
                log::error!("Error from sled in GetLogEntries. {}", err);
                AppDataError::Internal
            })?;
            let entry = bincode::deserialize(&data).map_err(|err| {
                log::error!("Error deserializing entry in  GetLogEntries. {}", err);
                AppDataError::Internal
            })?;
            acc.push(entry);
            Ok(acc)
        })?;
        Ok(entries)
    }
}

impl Handler<RgCreateSnapshot> for SledActor {
    type Result = Result<CurrentSnapshotData, AppDataError>;

    fn handle(&mut self, _msg: RgCreateSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

impl Handler<RgGetCurrentSnapshot> for SledActor {
    type Result = Result<Option<CurrentSnapshotData>, AppDataError>;

    fn handle(&mut self, _msg: RgGetCurrentSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}

impl Handler<RgInstallSnapshot> for SledActor {
    type Result = Result<(), AppDataError>;

    fn handle(&mut self, _msg: RgInstallSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        unimplemented!() // TODO: impl
    }
}
