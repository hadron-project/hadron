//! Stream Partition Controller.
//!
//! This controller is responsible for a partition of a stream. All reads & writes for any specific
//! partition will be handled by the leader of the SPC group. Groups are composed of the replicas
//! of a partition.

mod repl;
mod storage;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use prost::Message;
use sled::Tree;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tonic::transport::Channel;
use tonic::Status;

use crate::ctl_stream::repl::SpcReplStream;
use crate::database::Database;
use crate::error::ShutdownError;
use crate::models::placement::{ControlGroup, StreamReplica};
use crate::models::schema::Stream;
use crate::models::{prelude::*, WithId};
use crate::network::StreamPub;
use crate::proto::client::{
    stream_pub_client::Request as StreamPubClientRequest, stream_pub_server::Response as StreamPubServerResponse, StreamPubClient,
    StreamPubPayloadRequest, StreamPubServer,
};
use crate::utils::{self, TonicResult};
use crate::NodeId;
use crate::{config::Config, proto::client::StreamPubConnectResponse};

/// An error message for failures to apply a batch to storage.
const ERR_APPLY_BATCH: &str = "error applying batch to storage";
/// An error message for failures to encode an event record for storage.
const ERR_ENCODE_EVENT_RECORD: &str = "error encoding event record for storage";

const ENTRY_PREFIX: &[u8; 6] = b"entry/";
const KEY_NEXT_OFFSET: &str = "meta/next_offset";
const KEY_TERM: &str = "meta/term";

type PubRequest = (StreamPubClient, mpsc::UnboundedSender<TonicResult<StreamPubServer>>);

/// The Stream Partition Controller (SPC).
#[allow(dead_code)]
pub struct Spc {
    /// The ID of the Hadron cluster node on which this controller is running.
    node_id: NodeId,
    /// The application's runtime config.
    config: Arc<Config>,
    /// An identifier used in logging to identify this stream controller.
    stream_cluster_id: String,

    /// A handle to the database of this controller.
    db: Database,
    /// The database tree backing this controller.
    tree: Tree,
    /// Application shutdown channel.
    shutdown: watch::Receiver<bool>,
    /// A signal mapping peer nodes to their communication channels.
    peers: watch::Receiver<Arc<HashMap<u64, Channel>>>,
    /// Application event channel.
    app_rx: mpsc::Receiver<SpcInput>,

    /// The object model of this stream replica.
    replica: Arc<StreamReplica>,
    /// The stream to which this replica belongs.
    stream: Arc<WithId<Stream>>,
    /// The control group to which this replica belongs.
    cg: Arc<ControlGroup>,

    /// The next offset available for use for storing event records.
    next_offset: u64,
    /// The last term number to be recorded on disk.
    last_term_on_disk: u64,
}

impl Spc {
    /// Create a new instance.
    pub async fn new(
        node_id: NodeId, config: Arc<Config>, db: Database, shutdown: watch::Receiver<bool>, peers: watch::Receiver<Arc<HashMap<u64, Channel>>>,
        app_rx: mpsc::Receiver<SpcInput>, replica: Arc<StreamReplica>, cg: Arc<ControlGroup>, stream: Arc<WithId<Stream>>,
    ) -> Result<Self> {
        // Initialize DB tree for this controller.
        let (ns, name, partition, replica_id) = (stream.model.namespace(), stream.model.name(), replica.partition, replica.id);
        let tree = db
            .get_stream_tree(ns, name, partition, replica_id)
            .await
            .context("error getting handle to database tree")?;

        // Fetch any needed state from disk in order to begin operation.
        let (next_offset, last_term_on_disk) = Self::recover_state(tree.clone()).await.context("error recovering state from disk")?;
        let stream_cluster_id = format!("{}/{}[id={}]/{}/{}", ns, name, stream.id, partition, replica_id);
        Ok(Self {
            node_id,
            config,
            stream_cluster_id,
            db,
            tree,
            shutdown,
            peers,
            app_rx,
            replica,
            stream,
            cg,
            next_offset,
            last_term_on_disk,
        })
    }

    /// Spawn this controller.
    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!("SPC started for {}", self.stream_cluster_id);

        /* TODO: CRITICAL PATH
        - listen for updates from the app to add and remove members.
        - handle client requests, writing data to disk and responding.
        - get other config items in place to drive behavior.
            - CleanupPolicy: delete or compact: delete based on timestamps, compact based on message key.
        */

        loop {
            // If we need to shutdown before going into our next state loop, then break.
            if *self.shutdown.borrow() {
                break;
            }
            // Run the state specific logic for this controller based on its current role.
            let res = match self.cg.leader {
                Some((leader, term)) if leader == self.node_id => SpcLeader::new(&mut self, term).run().await,
                _ => SpcFollower::new(&mut self).run().await,
            };
            // Check for any returned errors, if we have a shutdown error, then we propagate to the
            // app controller to begin system shutdown.
            if let Err(err) = res {
                match err.downcast::<ShutdownError>() {
                    Ok(shutdown) => return Err(shutdown.into()),
                    err => tracing::error!(error = ?err, "error during SPC control loop"),
                }
            }
        }

        tracing::debug!("SPC has shutdown for {}", self.stream_cluster_id);
        Ok(())
    }

    /// Check if this node is the current partition leader.
    async fn recover_state(tree: Tree) -> Result<(u64, u64)> {
        Database::spawn_blocking(move || {
            let next_offset_opt = tree
                .get("meta/next_offset")
                .context("error fetching meta/next_offset key")?
                .map(|bytes| utils::decode_u64(&bytes))
                .transpose()?;
            let term_opt = tree
                .get("meta/term")
                .context("error fetching meta/term key")?
                .map(|bytes| utils::decode_u64(&bytes))
                .transpose()?;
            Ok((next_offset_opt.unwrap_or(0), term_opt.unwrap_or(0)))
        })
        .await?
    }
}

/// The input type used for communicating with SPC instances.
pub enum SpcInput {
    /// A client stream pub connection request.
    StreamPub(StreamPub),
}

/// The type state of an SPC when it is the leader of its control group.
struct SpcLeader<'a> {
    spc: &'a mut Spc,
    /// The term for which this controller is currently operating as leader.
    term: u64,
    followers: HashMap<NodeId, mpsc::Sender<()>>,

    /// The channel used for handling individual pub requests of active publisher connections.
    pub_tx: mpsc::Sender<PubRequest>,
    /// The channel used for handling individual pub requests of active publisher connections.
    pub_rx: mpsc::Receiver<PubRequest>,

    /// The channel used for handling feedback from repl streams.
    repl_feedback_tx: mpsc::Sender<()>,
    /// The channel used for handling feedback from repl streams.
    repl_feedback_rx: mpsc::Receiver<()>,

    /// The channel used to shutdown replication streams.
    repl_shutdown_tx: watch::Sender<bool>,
    /// The channel used to shutdown replication streams.
    repl_shutdown_rx: watch::Receiver<bool>,
}

impl<'a> SpcLeader<'a> {
    /// Create a new instance.
    fn new(spc: &'a mut Spc, term: u64) -> Self {
        let (pub_tx, pub_rx) = mpsc::channel(1000);
        let (repl_feedback_tx, repl_feedback_rx) = mpsc::channel(1000);
        let (repl_shutdown_tx, repl_shutdown_rx) = watch::channel(false);
        Self {
            spc,
            term,
            followers: Default::default(),
            pub_tx,
            pub_rx,
            repl_feedback_tx,
            repl_feedback_rx,
            repl_shutdown_tx,
            repl_shutdown_rx,
        }
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!("SPC is starting as leader for {}", self.spc.stream_cluster_id);

        // Establish connection to CPC for leadership heartbeat & syncing ISR data. We will not
        // resume until this has completed successfully.
        self.establish_cpc_connection().await;

        // Spawn replication streams for all other members of this control group.
        let node_id = self.spc.node_id;
        for node in self.spc.cg.nodes.clone().into_iter().filter(|node| node != &node_id) {
            self.spawn_replication_stream(&node).await;
        }

        loop {
            tokio::select! {
                Some(app_event) = self.spc.app_rx.next() => self.handle_input_event(app_event).await,
                Some(pub_req) = self.pub_rx.next() => self.handle_stream_pub_request(pub_req).await,
                Some(repl_msg) = self.repl_feedback_rx.next() => self.handle_repl_feedback(repl_msg).await,
                Some(needs_shutdown) = self.spc.shutdown.next() => if needs_shutdown { break } else { continue },
            }
        }

        tracing::debug!("SPC is stopping as leader for {}", self.spc.stream_cluster_id);
        let _ = self.repl_shutdown_tx.broadcast(true);
        Ok(())
    }

    /// Establish connection to CPC for leadership heartbeat & syncing ISR data.
    #[tracing::instrument(level = "trace", skip(self))]
    async fn establish_cpc_connection(&mut self) {
        // TODO: CRITICAL PATH
    }

    /// Handle a client connection request.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_client_connection_request(&mut self, mut req: StreamPub) {
        // Spawn stream input handler.
        // TODO: use a watcher channel to trigger stream close when no longer leader.
        let mut tx = self.pub_tx.clone();
        tokio::spawn(async move {
            let res_chan = req.tx;
            while let Some(req) = req.req.next().await {
                let req = match req {
                    Ok(req) => req,
                    Err(err) => {
                        tracing::error!(error = ?err, "error receiving stream publisher input");
                        continue;
                    }
                };
                if tx.send((req, res_chan.clone())).await.is_err() {
                    return; // Channel closed, controller has shut down.
                }
            }
        });
    }

    /// Handle an input event from some other part of the cluster.
    #[tracing::instrument(level = "trace", skip(self, input))]
    async fn handle_input_event(&mut self, input: SpcInput) {
        match input {
            SpcInput::StreamPub(req) => self.handle_client_connection_request(req).await,
        }
    }

    /// Handle feedback from a repl stream.
    #[tracing::instrument(level = "trace", skip(self, msg))]
    async fn handle_repl_feedback(&mut self, msg: ()) {
        // TODO: impl this.
    }

    /// Handle a client stream publish request.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_stream_pub_request(&mut self, req: PubRequest) {
        // Unpack the request payload.
        let (req, res) = req;
        let mut payload = match req.request {
            Some(StreamPubClientRequest::Payload(payload)) => payload,
            // Stream is already connected, so just respond affirmatively.
            Some(StreamPubClientRequest::Connect(_)) => {
                let _ = res.send(Ok(StreamPubServer {
                    response: Some(StreamPubServerResponse::Connect(StreamPubConnectResponse {
                        is_ready: true,
                        leader: String::new(),
                    })),
                }));
                return;
            }
            // If no request frame was detected, respond with an error.
            None => {
                let _ = res.send(Err(Status::invalid_argument("no request frame detected")));
                return;
            }
        };

        // If payload is empty, then there is nothing to store.
        if payload.batch.is_empty() {
            let _ = res.send(Err(Status::invalid_argument("no event records found in given batch")));
            return;
        }

        // Create a batch to be applied to storage. Iterate through our payload of entries and
        // insert into the batch. For Standard streams, an id/offset is assigned before insertion.
        let mut batch = sled::Batch::default();
        for record in payload.batch.iter_mut() {
            // Assign offsets.
            record.offset = self.spc.next_offset;
            self.spc.next_offset += 1;

            // Encode message for storage.
            let mut buf = Vec::with_capacity(record.encoded_len());
            if let Err(err) = record.encode(&mut buf) {
                tracing::error!(error = ?err, ERR_ENCODE_EVENT_RECORD);
                let _ = res.send(Err(Status::internal(ERR_ENCODE_EVENT_RECORD)));
                return;
            }
            let key = utils::encode_entry_key(ENTRY_PREFIX, record.offset);
            batch.insert(&key, buf);
        }
        batch.insert(KEY_NEXT_OFFSET, &utils::encode_u64(self.spc.next_offset)); // Always keep this up-to-date.

        // Insert the batch into storage.
        let tree = self.spc.tree.clone();
        let batch_res = Database::spawn_blocking(move || tree.apply_batch(batch).context(ERR_APPLY_BATCH))
            .await
            .map_err(anyhow::Error::from)
            .and_then(|res| res);
        if let Err(err) = batch_res {
            tracing::error!(error = ?err, ERR_APPLY_BATCH);
            let _ = res.send(Err(Status::internal(ERR_APPLY_BATCH)));
            return;
            // TODO: quite likely need to shutdown the system if we hit something like this.
        }

        // Finally, we replicate this data to other members of this CG's ISR group. If the caller
        // does not want to wait for replication, then we respond now & then replicate.
        self.replicate_payload(Arc::new(payload), self.spc.next_offset - 1).await;
    }

    /// Replicate the given payload to all members of the control group.
    #[tracing::instrument(level = "trace", skip(self, payload, needed_commit))]
    async fn replicate_payload(&mut self, payload: Arc<StreamPubPayloadRequest>, needed_commit: u64) {
        // TODO: CRITICAL PATH: finish up replication
    }

    /// Spawn a replication stream to the target node.
    #[tracing::instrument(level = "trace", skip(self, node))]
    async fn spawn_replication_stream(&mut self, node: &NodeId) {
        let (tx, rx) = mpsc::channel(1000);
        SpcReplStream::new(self.spc.node_id, *node, rx, self.repl_feedback_tx.clone(), self.repl_shutdown_rx.clone()).spawn();
        self.followers.insert(*node, tx);
    }
}

/// The type state of an SPC when it is a follower in its control group.
struct SpcFollower<'a> {
    spc: &'a mut Spc,
}

impl<'a> SpcFollower<'a> {
    /// Create a new instance.
    fn new(spc: &'a mut Spc) -> Self {
        Self { spc }
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!("SPC is starting as follower for {}", self.spc.stream_cluster_id);

        // TODO: CRITICAL PATH:
        // - Will need to accept replication streams from the leader.
        loop {
            tokio::select! {
                Some(app_event) = self.spc.app_rx.next() => self.handle_input_event(app_event).await,
                Some(needs_shutdown) = self.spc.shutdown.next() => if needs_shutdown { break } else { continue },
            }
        }

        tracing::debug!("SPC is stopping as follower for {}", self.spc.stream_cluster_id);
        // TODO: drain app input channel before exiting.
        Ok(())
    }

    /// Handle an input event from some other part of the cluster.
    #[tracing::instrument(level = "trace", skip(self, input))]
    async fn handle_input_event(&mut self, input: SpcInput) {
        match input {
            SpcInput::StreamPub(req) => {
                let _ = req.tx.send(Err(tonic::Status::unavailable("stream controller is not partition leader")));
            }
        }
    }
}
