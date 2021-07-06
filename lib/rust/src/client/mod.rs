//! The Hadron client.

mod metadata;
mod pipeline;
mod publisher;
mod subscriber;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures::prelude::*;
use h2::client::handshake;
use http::Uri;
use proto::v1::{ClusterMetadata, MetadataChangeType};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use uuid::Uuid;

pub use crate::client::pipeline::PipelineSubscription;
pub use crate::client::publisher::PublisherClient;
pub use crate::client::subscriber::{StreamSubscription, SubscriberConfig, SubscriptionStartingPoint};
use crate::common::{ClientCreds, H2Conn};

/// HTTP2 `authorization` header.
pub(crate) const AUTH_HEADER: &str = "authorization";

/// A channel of events bound for a client core.
pub(crate) type ClientEventTx = mpsc::Sender<ClientEvent>;
/// A client initialization response channel.
pub(crate) type ClientInitTx = oneshot::Sender<(Uuid, ClientEventTx, PartitionConnectionsSignalRx)>;
/// A mapping of pod URIs to their corresponding connection states.
pub(crate) type ConnectionMap = HashMap<Arc<String>, Connection>;
/// A signal of partitions to their corresponding connections.
pub(crate) type PartitionConnectionsSignal = watch::Sender<BTreeMap<u8, (Arc<String>, ConnectionState)>>;
/// A signal of partitions to their corresponding connections.
pub(crate) type PartitionConnectionsSignalRx = watch::Receiver<BTreeMap<u8, (Arc<String>, ConnectionState)>>;

/// The Hadron client.
///
/// Spawn this object first to begin interfacing with the Hadron cluster. This object will begin
/// gathering metadata from the cluster and maintains a stream of metadata changes which the
/// cluster will push to this object.
///
/// This object can be cheaply cloned and passed around a program. All clones will use the same
/// backing client core. Once all instances of this object have been dropped, the underlying
/// client core will also be dropped.
///
/// ## Initialization
/// The client should be initialized with the URI of the Kubernetes service used for the
/// Hadron cluster. The client will make an initial asynchronous connection to one of the backing
/// pods of the given Kubernetes service URI, and will gather the Hadron cluster's metadata. This
/// connection is maintained as the server will push metadata updates to the client.
///
/// The client will lazily establish HTTP2 connections to individual pods of the Hadron cluster as
/// needed, and these connections will be re-used for as long as possible.
#[derive(Clone)]
pub struct Client(Arc<ClientInner>);

impl Client {
    /// Construct a new client instance.
    ///
    /// On success, this will spawn a backing client core which will begin communicating with the
    /// target Hadron cluster to establish a metadata stream for cluster topology and the like.
    /// The given URL and credentials will be used for the metadata stream.
    ///
    /// ## Parameters
    /// - `cluster_service_url`: the URL of the Kubernetes service used to address all nodes of
    /// the target Hadron cluster.
    /// - `credentials`: the credentials to be used for establishing the metadata stream and which
    ///   will be used as the default credentials for any sub-clients created from this instance.
    pub async fn new(cluster_service_url: String, credentials: ClientCreds) -> Result<Self> {
        Ok(ClientCore::new(cluster_service_url, credentials).await?.spawn())
    }

    /// Get a handle to the client's default credentials.
    pub fn credentials(&self) -> Arc<ClientCreds> {
        self.0.credentials.clone()
    }
}

struct ClientInner {
    handle: Option<JoinHandle<()>>,
    events: mpsc::Sender<ClientEvent>,
    shutdown: broadcast::Sender<()>,
    credentials: Arc<ClientCreds>,
}

impl Drop for ClientInner {
    fn drop(&mut self) {
        let _res = self.shutdown.send(());
        if let Some(handle) = self.handle.take() {
            tokio::spawn(async move {
                let _res = handle.await;
            });
        }
    }
}

/// A client event.
pub(crate) enum ClientEvent {
    /// Create a new stream subscriber client.
    CreateStreamSubscriber { tx: ClientInitTx, stream: String },
    /// A stream subscriber has closed.
    StreamSubscriberClosed { stream: String, id: Uuid },
    /// Create a new stream publisher client.
    CreateStreamPublisher { tx: ClientInitTx, stream: String },
    /// A stream publisher has closed.
    StreamPublisherClosed { stream: String, id: Uuid },
    /// Create a new pipeline subscriber client.
    CreatePipelineSubscriber { tx: ClientInitTx, pipeline: String },
    /// A pipeline subscriber has closed.
    PipelineSubscriberClosed { pipeline: String, id: Uuid },
    /// A new pod connection at the given URI has been established.
    NewConnection(Arc<String>, H2Conn),
    /// The connection to the given URI is dead, so create a new one if still applicable.
    DeadConnection(Arc<String>),
    /// The metadata connection task was terminated and should be recreated.
    MetadataTaskTerminated,
    /// A metadata change received from the cluster.
    MetadataChange(MetadataChangeType),
}

/// The client core which implements all business logic of the Hadron client.
///
/// This object is never used directly by library users, but instead is spawned by the client
/// to do all of its work asynchronously.
pub(crate) struct ClientCore {
    /// The URI of the Kubernetes service used to address all of the pods of the Hadron cluster.
    cluster_service_url: Arc<String>,
    /// The client credentials to use for the metadata connection.
    credentials: Arc<ClientCreds>,

    /// All known cluster metadata received from the target cluster.
    cluster_metadata: Option<ClusterMetadata>,
    /// A map of all Hadron connections along with their state.
    connections: ConnectionMap,

    /// All active stream subscribers.
    stream_subscribers: HashMap<String, HashMap<Uuid, PartitionConnectionsSignal>>,
    /// All active stream publishers.
    stream_publishers: HashMap<String, HashMap<Uuid, PartitionConnectionsSignal>>,
    /// All active pipeline subscribers.
    pipeline_subscribers: HashMap<String, HashMap<Uuid, PartitionConnectionsSignal>>,

    /// A channel used to access resources for client instantiation.
    events_tx: mpsc::Sender<ClientEvent>,
    /// A channel used to access resources for client instantiation.
    events_rx: ReceiverStream<ClientEvent>,
    /// A channel used to indicate that the core is shutting down.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used to indicate that the core is shutting down.
    shutdown_rx: BroadcastStream<()>,
}

impl Drop for ClientCore {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
    }
}

impl ClientCore {
    /// Construct a new core instance.
    async fn new(cluster_service_url: String, credentials: ClientCreds) -> Result<Self> {
        let _uri: Uri = cluster_service_url
            .parse()
            .context("failed to parse cluster service address")?;
        let cluster_service_url = Arc::new(cluster_service_url);
        let (events_tx, events_rx) = mpsc::channel(1000);
        let (shutdown_tx, shutdown_rx) = broadcast::channel(10);
        Ok(Self {
            cluster_service_url,
            credentials: Arc::new(credentials),
            cluster_metadata: None,
            connections: Default::default(),
            stream_subscribers: Default::default(),
            stream_publishers: Default::default(),
            pipeline_subscribers: Default::default(),
            events_tx,
            events_rx: ReceiverStream::new(events_rx),
            shutdown_tx,
            shutdown_rx: BroadcastStream::new(shutdown_rx),
        })
    }

    /// Spawn this instance onto the current runtime returning a client instance with resource handles.
    pub fn spawn(self) -> Client {
        let (shutdown, events) = (self.shutdown_tx.clone(), self.events_tx.clone());
        let credentials = self.credentials.clone();
        let handle = Some(tokio::spawn(self.run()));
        Client(Arc::new(ClientInner { handle, shutdown, events, credentials }))
    }

    /// The main loop of this task type.
    async fn run(mut self) {
        tracing::debug!(%self.cluster_service_url, "client instance has started");

        self.spawn_metadata_stream();
        loop {
            tokio::select! {
                Some(event) = self.events_rx.next() => self.handle_event(event).await,
                _ = self.shutdown_rx.next() => break,
            }
        }

        tracing::debug!(%self.cluster_service_url, "client instance has shut down");
    }

    /// Handle a client event.
    #[tracing::instrument(level = "debug", skip(self, event))]
    async fn handle_event(&mut self, event: ClientEvent) {
        match event {
            ClientEvent::CreateStreamSubscriber { tx, stream } => self.create_stream_subscriber(tx, stream).await,
            ClientEvent::StreamSubscriberClosed { stream, id } => self.stream_subscriber_closed(stream, id),
            ClientEvent::CreateStreamPublisher { tx, stream } => self.create_stream_publisher(tx, stream).await,
            ClientEvent::StreamPublisherClosed { stream, id } => self.stream_publisher_closed(stream, id),
            ClientEvent::CreatePipelineSubscriber { tx, pipeline } => self.create_pipeline_subscriber(tx, pipeline).await,
            ClientEvent::PipelineSubscriberClosed { pipeline, id } => self.pipeline_subscriber_closed(pipeline, id),
            ClientEvent::NewConnection(pod, conn) => self.handle_new_connection(pod, conn),
            ClientEvent::DeadConnection(pod) => self.handle_dead_connection(pod).await,
            ClientEvent::MetadataTaskTerminated => self.spawn_metadata_stream(),
            ClientEvent::MetadataChange(change) => self.handle_metadata_change(change),
        }
    }

    #[tracing::instrument(level = "debug", skip(self, stream, id))]
    fn stream_subscriber_closed(&mut self, stream: String, id: Uuid) {
        self.stream_subscribers.get_mut(&stream).and_then(|subs| subs.remove(&id));
    }

    #[tracing::instrument(level = "debug", skip(self, stream, id))]
    fn stream_publisher_closed(&mut self, stream: String, id: Uuid) {
        self.stream_publishers.get_mut(&stream).and_then(|pubs| pubs.remove(&id));
    }

    #[tracing::instrument(level = "debug", skip(self, pipeline, id))]
    fn pipeline_subscriber_closed(&mut self, pipeline: String, id: Uuid) {
        self.pipeline_subscribers
            .get_mut(&pipeline)
            .and_then(|subs| subs.remove(&id));
    }

    #[tracing::instrument(level = "debug", skip(self, tx, stream))]
    async fn create_stream_subscriber(&mut self, tx: ClientInitTx, stream: String) {
        let id = Uuid::new_v4();
        let subs = self.stream_subscribers.entry(stream.clone()).or_default();
        let stream_opt = self.cluster_metadata.as_ref().and_then(|meta| meta.streams.get(&stream));
        let stream_meta = match stream_opt {
            Some(stream_meta) => stream_meta,
            None => {
                let (watch_tx, watch_rx) = watch::channel(Default::default());
                subs.insert(id, watch_tx);
                let _res = tx.send((id, self.events_tx.clone(), watch_rx));
                return;
            }
        };
        let conns_map = Self::build_new_partition_connections_map(stream_meta, &mut self.connections, &self.events_tx, &self.shutdown_tx);
        let (watch_tx, watch_rx) = watch::channel(conns_map);
        subs.insert(id, watch_tx);
        let _res = tx.send((id, self.events_tx.clone(), watch_rx));
    }

    #[tracing::instrument(level = "debug", skip(self, tx, stream))]
    async fn create_stream_publisher(&mut self, tx: ClientInitTx, stream: String) {
        let id = Uuid::new_v4();
        let pubs = self.stream_publishers.entry(stream.clone()).or_default();
        let stream_opt = self.cluster_metadata.as_ref().and_then(|meta| meta.streams.get(&stream));
        let stream_meta = match stream_opt {
            Some(stream_meta) => stream_meta,
            None => {
                let (watch_tx, watch_rx) = watch::channel(Default::default());
                pubs.insert(id, watch_tx);
                let _res = tx.send((id, self.events_tx.clone(), watch_rx));
                return;
            }
        };
        let conns_map = Self::build_new_partition_connections_map(stream_meta, &mut self.connections, &self.events_tx, &self.shutdown_tx);
        let (watch_tx, watch_rx) = watch::channel(conns_map);
        pubs.insert(id, watch_tx);
        let _res = tx.send((id, self.events_tx.clone(), watch_rx));
    }

    #[tracing::instrument(level = "debug", skip(self, tx, pipeline))]
    async fn create_pipeline_subscriber(&mut self, tx: ClientInitTx, pipeline: String) {
        let id = Uuid::new_v4();
        let subs = self.pipeline_subscribers.entry(pipeline.clone()).or_default();
        let stream_opt = self
            .cluster_metadata
            .as_ref()
            .and_then(|meta| meta.pipelines.get(&pipeline).map(|pipeline_meta| (meta, pipeline_meta)))
            .and_then(|(meta, pipeline_meta)| meta.streams.get(&pipeline_meta.source_stream));
        let stream_meta = match stream_opt {
            Some(stream_meta) => stream_meta,
            None => {
                let (watch_tx, watch_rx) = watch::channel(Default::default());
                subs.insert(id, watch_tx);
                let _res = tx.send((id, self.events_tx.clone(), watch_rx));
                return;
            }
        };
        let conns_map = Self::build_new_partition_connections_map(stream_meta, &mut self.connections, &self.events_tx, &self.shutdown_tx);
        let (watch_tx, watch_rx) = watch::channel(conns_map);
        subs.insert(id, watch_tx);
        let _res = tx.send((id, self.events_tx.clone(), watch_rx));
    }

    /// Handle a newly established cluster connection.
    ///
    /// This handler will iterate over all leaf clients, and any client which has a connection to
    /// the matching pod will be updated with the new connection.
    #[tracing::instrument(level = "debug", skip(self, pod, conn))]
    fn handle_new_connection(&mut self, pod: Arc<String>, conn: H2Conn) {
        self.stream_publishers
            .iter()
            .chain(self.stream_subscribers.iter())
            .chain(self.pipeline_subscribers.iter())
            .for_each(|(_, groups)| {
                groups.iter().for_each(|(_, chan)| {
                    let conns_map = chan.borrow();
                    if let Some(partition) = conns_map
                        .iter()
                        .find(|(_, (old_pod, _))| old_pod.as_str() == pod.as_str())
                        .map(|(partition, _)| *partition)
                    {
                        let mut new_conns = conns_map.to_owned();
                        new_conns.insert(partition, (pod.clone(), ConnectionState::Connected(conn.clone())));
                        let _res = chan.send(new_conns);
                    }
                });
            });
    }

    /// Handle a dead connection report, typically coming from a leaf client.
    ///
    /// Note that multiple leaf clients may report a connection being dead at the same time, or in
    /// close succession, and if we were to indiscriminatnly recreate the connection without first
    /// verifying that the connection is indeed dead, then we may ultimately perform uneeded work.
    /// As such, we first check to verify that the connection is indeed dead before attempting to
    /// reconnect.
    ///
    /// If the target pod no longer exists, it will eventually disappear from the connections maps
    /// of the various clients and will cease to propagate through reconnect events.
    ///
    /// Once it has been established that the connection is indeed dead, all leaf clients which
    /// have a connection to the pod will be updated to indicate that a reconnect is taking place.
    #[tracing::instrument(level = "debug", skip(self, pod))]
    async fn handle_dead_connection(&mut self, pod: Arc<String>) {
        // Check the health of the connection.
        if let Some(conn) = self.connections.get(pod.as_ref()) {
            match &conn.state {
                ConnectionState::Pending => (),
                // A reconnect is already underway.
                ConnectionState::Connecting => return,
                ConnectionState::Connected(conn) => {
                    if conn.clone().ready().await.is_ok() {
                        // The connection is ok. This would happen if a leaf client emitted a dead
                        // connection event while a reconnect was already in-progress.
                        return;
                    }
                }
            }
        }

        // The connection is dead. Emit an update to all leaf clients which bear a connection to
        // the pod, and then spawn a new connection.
        self.stream_publishers
            .iter()
            .chain(self.stream_subscribers.iter())
            .chain(self.pipeline_subscribers.iter())
            .for_each(|(_, groups)| {
                groups.iter().for_each(|(_, chan)| {
                    let conns_map = chan.borrow();
                    if let Some(partition) = conns_map
                        .iter()
                        .find(|(_, (old_pod, _))| old_pod.as_str() == pod.as_str())
                        .map(|(partition, _)| *partition)
                    {
                        let mut new_conns = conns_map.clone();
                        new_conns.insert(partition, (pod.clone(), ConnectionState::Connecting));
                        let _res = chan.send(new_conns);
                    }
                });
            });
        spawn_connection_builder(&mut self.connections, pod, self.events_tx.clone(), self.shutdown_tx.subscribe());
    }
}

/// Spawn a new task to build a connection to the target node.
fn spawn_connection_builder(
    connections: &mut ConnectionMap, target: Arc<String>, events: mpsc::Sender<ClientEvent>, shutdown: broadcast::Receiver<()>,
) -> ConnectionState {
    let mut conn = connections
        .entry(target.clone())
        .or_insert_with(|| Connection::new(target.clone()));
    match &conn.state {
        // If a new connection is already being established elsewhere, then do nothing.
        ConnectionState::Connecting => return conn.state.clone(),
        // If we already have a live connection and we do not need to drop it, then do nothing.
        ConnectionState::Connected(_) | ConnectionState::Pending => {
            conn.state = ConnectionState::Connecting;
        }
    }
    tokio::spawn(establish_connection(target, events, shutdown));
    conn.state.clone()
}

/// Spawn a new task to build a connection to the given target.
async fn establish_connection(target: Arc<String>, events: mpsc::Sender<ClientEvent>, mut shutdown: broadcast::Receiver<()>) {
    tracing::debug!(node = %&target, "establishing new connection to hadron node");
    // Establish H2 connection.
    let h2_res = tokio::select! {
        h2_res = establish_h2_connection_with_backoff(target.clone()) => h2_res,
        _ = shutdown.recv() => return, // Client is shutting down. Nothing else to do.
    };
    match h2_res {
        Ok(h2) => {
            let _res = events.send(ClientEvent::NewConnection(target, h2)).await;
        }
        Err(_) => {
            let _res = events.send(ClientEvent::DeadConnection(target)).await;
        }
    }
}

/// Establish a HTTP2 connection using an exponential backoff and retry.
async fn establish_h2_connection_with_backoff(target: Arc<String>) -> Result<H2Conn> {
    let backoff = backoff::ExponentialBackoff {
        max_elapsed_time: Some(Duration::from_secs(10)),
        ..Default::default()
    };
    backoff::future::retry(backoff, || async {
        tracing::debug!(node = %&target, "establishing TCP connection");
        // Establish TCP connection.
        let tcp_res = TcpStream::connect(target.as_str()).await;
        let tcp = match tcp_res {
            Ok(tcp) => tcp,
            Err(err) => {
                tracing::error!(error = ?err, "error establishing TCP connection to hadron node {}", &*target);
                return Err(backoff::Error::Transient(()));
            }
        };

        // Perform H2 handshake.
        let h2_res = handshake(tcp).await;
        let (h2_conn, connection) = match h2_res {
            Ok(h2_conn) => h2_conn,
            Err(err) => {
                tracing::error!(error = ?err, "error during HTTP2 handshake with hadron node {}", &*target);
                return Err(backoff::Error::Transient(()));
            }
        };

        // Spawn off the new connection for continual processing.
        let node = target.clone();
        tokio::spawn(async move {
            if let Err(err) = connection.await {
                tracing::error!(error = ?err, %node, "connection with hadron node severed");
            }
        });
        Ok(h2_conn)
    })
    .await
    .map_err(|_| anyhow!("refusing to continue reconnect attempts to hadron node {}", target))
}

/// Connection state of a connection to a node of a Hadron cluster.
pub(crate) struct Connection {
    /// The target pod's StatefulSet DNS name.
    pub uri: Arc<String>,
    /// The state of the connection.
    pub state: ConnectionState,
}

/// The internal state of a connection to a Hadron node.
#[derive(Clone)]
pub(crate) enum ConnectionState {
    /// No action related to this connection has taken place.
    Pending,
    /// The connection is being established.
    Connecting,
    /// The connection is live and ready for use.
    Connected(H2Conn),
}

impl Connection {
    /// Create a new instance.
    pub fn new(uri: Arc<String>) -> Self {
        Self { uri, state: ConnectionState::Pending }
    }
}
