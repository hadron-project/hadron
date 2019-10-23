//! The networking layer.

mod client;
mod peer;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::future::TryFutureExt;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, oneshot::error::RecvError};
use tokio::task::JoinHandle;
use tonic::transport::{Channel, ClientTlsConfig, Server, ServerTlsConfig, Uri};
use tonic::Status;

use crate::config::Config;
use crate::discovery::{ObservedPeersChangeset, PeerSrv};
use crate::network::client::{ClientRequest, ClientServer, ClientService};
use crate::network::peer::{PeerClient, PeerRequest, PeerServer, PeerService};
use crate::proto;
use crate::NodeId;

const ERR_BUILD_URI: &str = "error building URI for peer connection";

pub(self) type TonicResult<T> = std::result::Result<T, Status>;

/// An actor responsible for managing network activity.
///
/// ### discovery
/// The discovery actor feeds changesets of peer SRV records into this actor via communication
/// channel. As changes arrive, this actor will establish new persistent HTTP2 connections to peers
/// and will perform an initial handshake with the peer to ensure proper connectivity.
///
/// If a peer disappears from the discovery backend, the peer's socket info will be updated to
/// indicate that it has disappeared, but the connection will not immediately be removed. Instead,
/// this actor has a routine wich will scan through the list of connected peers, and any which are
/// marked as having disappeared will be healthchecked. If the healthcheck fails and the node is
/// still absent from the discovery system, then we reap the connection and emit an update to over
/// this actor's network update channel for other components to observe.
pub struct Network {
    /// The ID of this node in the Raft cluster.
    node_id: NodeId,
    config: Arc<Config>,
    discovery_rx: mpsc::Receiver<ObservedPeersChangeset>,
    client_network_tx: mpsc::UnboundedSender<ClientRequest>,
    client_network_rx: mpsc::UnboundedReceiver<ClientRequest>,
    peer_network_tx: mpsc::UnboundedSender<PeerRequest>,
    peer_network_rx: mpsc::UnboundedReceiver<PeerRequest>,
    net_int_tx: mpsc::UnboundedSender<NetworkInternal>,
    net_int_rx: mpsc::UnboundedReceiver<NetworkInternal>,
    _peer_server: JoinHandle<Result<()>>,
    _client_server: JoinHandle<Result<()>>,
    socket_to_peer_map: HashMap<PeerSrv, PeerSocketState>,
    node_id_to_socket_map: HashMap<NodeId, Channel>,
}

impl Network {
    pub fn new(node_id: NodeId, config: Arc<Config>, discovery_rx: mpsc::Receiver<ObservedPeersChangeset>) -> Result<Self> {
        let (peer_network_tx, peer_network_rx) = mpsc::unbounded_channel();
        let (client_network_tx, client_network_rx) = mpsc::unbounded_channel();
        let (net_int_tx, net_int_rx) = mpsc::unbounded_channel();
        let _peer_server = Self::build_peer_server(node_id, peer_network_tx.clone(), &*config)?;
        let _client_server = Self::build_client_server(node_id, client_network_tx.clone(), &*config)?;

        // TODO:
        // - `NetworkControl` will be used as an input stream into this actor. Used for registry and any
        // other control functions from other components.
        // - `NetworkUpdate` will be used as in a broadcast channel to update registered components with
        // the latest info from the network actor.
        // - create a network maintenance loop which will check all peer sockets. Any which have disappeared from
        // the discovery system will be liveness checked and reaped if they are dead.

        Ok(Self {
            node_id,
            config,
            discovery_rx,
            client_network_tx,
            client_network_rx,
            peer_network_tx,
            peer_network_rx,
            net_int_tx,
            net_int_rx,
            _peer_server,
            _client_server,
            socket_to_peer_map: Default::default(),
            node_id_to_socket_map: Default::default(),
        })
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    #[tracing::instrument(level = "trace", name = "network", skip(self))]
    async fn run(mut self) {
        tracing::trace!("network actor is running");
        loop {
            tokio::select! {
                Some(changeset) = self.discovery_rx.next() => self.handle_discovery_changeset(changeset).await,
                Some(peer_req) = self.peer_network_rx.next() => self.handle_peer_request(peer_req).await,
                Some(client_req) = self.client_network_rx.next() => self.handle_client_request(client_req).await,
                Some(net_int) = self.net_int_rx.next() => self.handle_network_internal_update(net_int).await,
            }
        }
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_client_request(&mut self, req: ClientRequest) {
        todo!()
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_peer_request(&mut self, req: PeerRequest) {
        todo!()
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn handle_network_internal_update(&mut self, update: NetworkInternal) {
        match update {
            NetworkInternal::PeerInitConnSuccess(addr, peer_id, channel) => {
                self.handle_network_internal_peer_init_conn_success(addr, peer_id, channel);
            }
            NetworkInternal::PeerInitConnFailed(addr) => {
                self.handle_network_internal_init_peer_conn_failed(addr);
            }
        }
    }

    /// Handle changesets coming from the discovery system.
    #[tracing::instrument(level = "trace", skip(self))]
    async fn handle_discovery_changeset(&mut self, changeset: ObservedPeersChangeset) {
        tracing::trace!("handling discovery changeset");
        // Create a new peer connection for each newly discovered peer.
        for addr in changeset.new_peers {
            if self.socket_to_peer_map.contains_key(&addr) {
                continue;
            }
            self.socket_to_peer_map.insert(
                addr.clone(),
                PeerSocketState {
                    node_id: None,
                    discovery_state: DiscoveryState::Observed,
                    connection_state: PeerConnectionState::Initializing,
                },
            );
            let (node_id, net_int, config) = (self.node_id, self.net_int_tx.clone(), self.config.clone());
            tokio::spawn(async move {
                Self::build_peer_connection(addr, node_id, config, net_int).await;
            });
        }

        // For any addr which has disappeared from the discovery system, update its state so that
        // it can be reaped if the peer has indeed disappeared.
        for addr in changeset.purged_peers {
            if let Some(socket_info) = self.socket_to_peer_map.get_mut(&addr) {
                socket_info.discovery_state = DiscoveryState::Disappeared;
            }
        }
    }

    /// Handle event for successfully establishing a peer connection.
    #[tracing::instrument(level = "trace", skip(self, addr, chan))]
    fn handle_network_internal_peer_init_conn_success(&mut self, addr: PeerSrv, peer_id: NodeId, chan: Channel) {
        tracing::trace!("peer connection established");
        // If this is a connection to self, then drop.
        if peer_id == self.node_id {
            tracing::trace!("peer connection is to self, dropping");
            self.socket_to_peer_map.remove(&addr);
            return;
        }
        if let Some(state) = self.socket_to_peer_map.get_mut(&addr) {
            state.node_id = Some(peer_id);
            state.connection_state = PeerConnectionState::Connected;
        }
        self.node_id_to_socket_map.insert(peer_id, chan);
    }

    /// Handle event for successfully establishing a peer connection.
    #[tracing::instrument(level = "trace", skip(self, addr))]
    fn handle_network_internal_init_peer_conn_failed(&mut self, addr: PeerSrv) {
        tracing::trace!(addr = ?addr, "failed to establish connection to peer");
        let state = match self.socket_to_peer_map.get_mut(&addr) {
            Some(state) => state,
            None => return,
        };
        // If the peer is no longer being observed by the discovery system, and we have not been
        // able to successfully establish a connection, then drop the socket state & stop retrying.
        if state.discovery_state == DiscoveryState::Disappeared {
            self.socket_to_peer_map.remove(&addr);
            return;
        }
        // Else, let's retry the connection. The called routine has a built-in rate-limiting
        // mechanism to ensure this isn't executed too aggressively.
        // TODO: in the future, look into using an exponential backoff pattern here.
        let (node_id, net_int, config) = (self.node_id, self.net_int_tx.clone(), self.config.clone());
        tokio::spawn(async move {
            Self::build_peer_connection(addr, node_id, config, net_int).await;
        });
    }

    fn build_peer_server(id: NodeId, tx: mpsc::UnboundedSender<peer::PeerRequest>, config: &Config) -> Result<JoinHandle<Result<()>>> {
        // TODO: integrate TLS configuration.
        let addr = format!("0.0.0.0:{}", config.server_port)
            .parse()
            .context("failed to parse socket addr for internal gRPC server, probably a bad port")?;
        let svc = PeerService::new(id, tx);
        let server_fut = Server::builder()
            .tcp_keepalive(Some(Duration::from_secs(30))) // Raft heartbeats will keep this guy alive.
            .timeout(Duration::from_secs(10))
            .add_service(PeerServer::new(svc))
            .serve(addr)
            .map_err(From::from);
        Ok(tokio::spawn(server_fut))
    }

    fn build_client_server(id: NodeId, tx: mpsc::UnboundedSender<client::ClientRequest>, config: &Config) -> Result<JoinHandle<Result<()>>> {
        // TODO: integrate TLS configuration.
        let addr = format!("0.0.0.0:{}", config.client_port)
            .parse()
            .context("failed to parse socket addr for client gRPC server, probably a bad port")?;
        let svc = ClientService::new(id, tx);
        let server_fut = Server::builder()
            .tcp_keepalive(Some(Duration::from_secs(10)))
            .timeout(Duration::from_secs(10))
            .add_service(ClientServer::new(svc))
            .serve(addr)
            .map_err(From::from);
        Ok(tokio::spawn(server_fut))
    }

    /// Build a peer connection, performing handshake after initial connection is established.
    ///
    /// NOTE: calls to this method should always be spawned, as to not block the networking main loop.
    #[tracing::instrument(level = "trace", skip(config, tx))]
    async fn build_peer_connection(addr: PeerSrv, node_id: NodeId, config: Arc<Config>, tx: mpsc::UnboundedSender<NetworkInternal>) {
        match Self::_build_peer_connection(&addr, node_id, &*config).await {
            Ok((peer_id, channel)) => {
                let _ = tx.send(NetworkInternal::PeerInitConnSuccess(addr, peer_id, channel));
            }
            Err(err) => {
                tracing::error!(error = ?err, "error establishing connection with peer");
                tokio::time::delay_for(Duration::from_secs(1)).await;
                let _ = tx.send(NetworkInternal::PeerInitConnFailed(addr));
            }
        }
    }

    #[tracing::instrument(level = "trace", skip(config))]
    async fn _build_peer_connection(addr: &PeerSrv, node_id: NodeId, config: &Config) -> Result<(NodeId, Channel)> {
        // Build connection object.
        let uri = Uri::builder()
            .authority(format!("{}:{}", &addr.fqdn, addr.port).as_str())
            .path_and_query("");
        let chan_builder = match config.tls_config() {
            Some(_tls_config) => {
                let uri = uri.scheme("https").build().context(ERR_BUILD_URI)?;
                Channel::builder(uri) // TODO: finish this up with TLS config
            }
            None => {
                let uri = uri.scheme("http").build().context(ERR_BUILD_URI)?;
                Channel::builder(uri)
            }
        };
        // Establish the base H2 connection to the peer & perform the peer handshake.
        let channel = chan_builder.connect().await.context("error connecting to peer node")?;
        let mut client = PeerClient::new(channel.clone());
        let res = client
            .handshake(proto::peer::HandshakeMsg { node_id })
            .await
            .context("error during peer handshake")?;
        let peer_id = res.into_inner().node_id;
        Ok((peer_id, channel))
    }
}

/// A type used to keep track of an associated peer IP address's state in the discovery system.
#[derive(Clone, Debug, Eq, PartialEq)]
enum DiscoveryState {
    /// The peer IP is currently being observed by the discovery system.
    Observed,
    /// The peer IP has disappeared from the discovery system.
    ///
    /// When this is the case, the likelihood of reconnect failure is higher. If a reconnect does
    /// fail and the peer has disappeared from the discovery system, then reconnects should be
    /// aborted and the actor responsible for the connection should shutdown.
    Disappeared,
}

/// An update which is internal to the network layer.
#[derive(Debug)]
enum NetworkInternal {
    /// A peer connection has been successfully established.
    PeerInitConnSuccess(PeerSrv, NodeId, Channel),
    /// Failed to establish a connection to the specified peer.
    PeerInitConnFailed(PeerSrv),
}

/// A control message to update the network layer.
#[derive(Debug)]
pub enum NetworkControl {}

/// An update from the network layer.
#[derive(Debug)]
pub enum NetworkUpdate {}

/// The state of a connection to a peer node.
#[derive(Debug)]
struct PeerSocketState {
    node_id: Option<NodeId>,
    discovery_state: DiscoveryState,
    connection_state: PeerConnectionState,
}

/// The connetion state of a connection to a peer node.
#[derive(Debug)]
enum PeerConnectionState {
    Initializing,
    Connected,
}

pub(self) fn status_from_rcv_error(_: RecvError) -> Status {
    Status::internal("target server dropped internal response channel")
}

pub(self) fn map_result_to_status<T>(res: Result<T>) -> TonicResult<T> {
    res.map_err(|err| Status::internal(err.to_string()))
}
