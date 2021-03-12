//! The networking layer.

#![allow(unused_imports)] // TODO: remove this.
#![allow(unused_variables)] // TODO: remove this.
#![allow(unused_mut)] // TODO: remove this.
#![allow(dead_code)] // TODO: remove this.
#![allow(clippy::redundant_clone)] // TODO: remove this.

mod client;
mod peer;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::future::{FutureExt, TryFutureExt};
use tokio::stream::StreamExt;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tonic::transport::{Channel, Server, Uri};

use crate::config::Config;
use crate::ctl_raft::CRCIndex;
use crate::discovery::{ObservedPeersChangeset, PeerSrv};
pub use crate::network::client::{
    forward_client_request, ClientClient, ClientRequest, EphemeralPub, EphemeralSub, PipelineStageSub, RpcPub, RpcSub, StreamPub, StreamSub,
    StreamUnsub, Transaction, UpdateSchema,
};
use crate::network::client::{ClientServer, ClientService};
pub use crate::network::peer::{
    send_append_entries, send_install_snapshot, send_vote, PeerClient, PeerRequest, RaftAppendEntries, RaftInstallSnapshot, RaftVote,
};
use crate::network::peer::{PeerServer, PeerService};
use crate::proto;
use crate::NodeId;

const ERR_BUILD_URI: &str = "error building URI for peer connection";

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
    /// The CRC data index.
    index: Arc<CRCIndex>,

    discovery_rx: mpsc::Receiver<ObservedPeersChangeset>,

    /// The channel used to receive requests coming from the client gRPC server.
    client_network_rx: mpsc::UnboundedReceiver<ClientRequest>,
    /// The channel used to receive requests coming from the peer gRPC server.
    peer_network_rx: mpsc::UnboundedReceiver<PeerRequest>,

    /// The channel used to send async control messages back to this actor.
    net_internal_tx: mpsc::UnboundedSender<NetworkInternal>,
    /// The channel used to receive async control messages sent to this actor from itself.
    net_internal_rx: mpsc::UnboundedReceiver<NetworkInternal>,

    /// An output channel used for propagating data into the app.
    output_tx: mpsc::UnboundedSender<NetworkOutput>,
    /// An output signal mapping peer node IDs to their communication channels.
    peers_tx: watch::Sender<Arc<HashMap<NodeId, Channel>>>,

    /// Application shutdown signal.
    shutdown: watch::Receiver<bool>,
    server_shutdown_tx: broadcast::Sender<()>,

    peer_server: JoinHandle<Result<()>>,
    client_server: JoinHandle<Result<()>>,

    socket_to_peer_map: HashMap<PeerSrv, PeerSocketState>,
    node_id_to_socket_map: HashMap<NodeId, Channel>,
}

impl Network {
    pub fn new(
        node_id: NodeId, config: Arc<Config>, index: Arc<CRCIndex>, discovery_rx: mpsc::Receiver<ObservedPeersChangeset>,
        peers_tx: watch::Sender<Arc<HashMap<NodeId, Channel>>>, shutdown: watch::Receiver<bool>,
    ) -> Result<(Network, mpsc::UnboundedReceiver<NetworkOutput>)> {
        let (peer_network_tx, peer_network_rx) = mpsc::unbounded_channel();
        let (client_network_tx, client_network_rx) = mpsc::unbounded_channel();
        let (net_internal_tx, net_internal_rx) = mpsc::unbounded_channel();
        let (output_tx, output_rx) = mpsc::unbounded_channel();

        let (server_shutdown_tx, server_shutdown_rx) = broadcast::channel(1);
        let peer_server = Self::build_peer_server(
            node_id,
            peer_network_tx,
            client_network_tx.clone(),
            config.clone(),
            index.clone(),
            server_shutdown_rx,
        )?;
        let client_server = Self::build_client_server(node_id, client_network_tx, config.clone(), index.clone(), server_shutdown_tx.subscribe())?;

        // TODO: setup an unordered futures stream of peers which have disappeared from the discovery system
        // and which need to be healthchecked until we can safely remove them or until they reappear and we preserve.

        let this = Self {
            node_id,
            config,
            index,
            discovery_rx,
            client_network_rx,
            peer_network_rx,
            net_internal_tx,
            net_internal_rx,
            output_tx,
            peers_tx,
            peer_server,
            client_server,
            shutdown,
            server_shutdown_tx,
            socket_to_peer_map: Default::default(),
            node_id_to_socket_map: Default::default(),
        };
        Ok((this, output_rx))
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
                Some(net_int) = self.net_internal_rx.next() => self.handle_network_internal_update(net_int).await,
                Some(needs_shutdown) = self.shutdown.next() => if needs_shutdown { break } else { continue },
            }
        }

        // Graceful shutdown.
        tracing::debug!("network is shutting down");
        let _ = self.server_shutdown_tx.send(()); // Shutsdown all network listeners.
        let _ = self.peer_server.await;
        let _ = self.client_server.await;
        tracing::debug!("network shutdown");
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_client_request(&mut self, req: ClientRequest) {
        let _ = self.output_tx.send(NetworkOutput::ClientRequest(req));
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_peer_request(&mut self, req: PeerRequest) {
        let _ = self.output_tx.send(NetworkOutput::PeerRequest(req));
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
            if let Some(socket_info) = self.socket_to_peer_map.get_mut(&addr) {
                socket_info.discovery_state = DiscoveryState::Observed;
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
            let (node_id, net_int, config) = (self.node_id, self.net_internal_tx.clone(), self.config.clone());
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
        let _ = self.peers_tx.broadcast(Arc::new(self.node_id_to_socket_map.clone()));
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
        let (node_id, net_int, config) = (self.node_id, self.net_internal_tx.clone(), self.config.clone());
        tokio::spawn(async move {
            Self::build_peer_connection(addr, node_id, config, net_int).await;
        });
    }

    fn build_peer_server(
        id: NodeId, peer_tx: mpsc::UnboundedSender<peer::PeerRequest>, client_tx: mpsc::UnboundedSender<client::ClientRequest>, config: Arc<Config>,
        index: Arc<CRCIndex>, mut shutdown: broadcast::Receiver<()>,
    ) -> Result<JoinHandle<Result<()>>> {
        // TODO: integrate TLS configuration.
        let addr = format!("0.0.0.0:{}", config.server_port)
            .parse()
            .context("failed to parse socket addr for internal gRPC server, probably a bad port")?;
        let peer_svc = PeerService::new(id, peer_tx);
        let client_svc = ClientService::new(id, config, index, client_tx);
        Ok(tokio::spawn(async move {
            let server_fut = Server::builder()
                .tcp_keepalive(Some(Duration::from_secs(30))) // Raft heartbeats will keep this guy alive.
                .timeout(Duration::from_secs(10))
                .add_service(PeerServer::new(peer_svc))
                .add_service(ClientServer::new(client_svc))
                .serve_with_shutdown(addr, shutdown.next().map(|_| ()))
                .map_err(anyhow::Error::from);
            server_fut.await
        }))
    }

    fn build_client_server(
        id: NodeId, tx: mpsc::UnboundedSender<client::ClientRequest>, config: Arc<Config>, index: Arc<CRCIndex>,
        mut shutdown: broadcast::Receiver<()>,
    ) -> Result<JoinHandle<Result<()>>> {
        // TODO: integrate TLS configuration.
        let addr = format!("0.0.0.0:{}", config.client_port)
            .parse()
            .context("failed to parse socket addr for client gRPC server, probably a bad port")?;
        let svc = ClientService::new(id, config, index, tx);
        Ok(tokio::spawn(async move {
            let server_fut = Server::builder()
                .tcp_keepalive(Some(Duration::from_secs(10)))
                .timeout(Duration::from_secs(10))
                .add_service(ClientServer::new(svc))
                .serve_with_shutdown(addr, shutdown.next().map(|_| ()))
                .map_err(anyhow::Error::from);
            server_fut.await
        }))
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

/// Network output variants.
pub enum NetworkOutput {
    PeerRequest(PeerRequest),
    ClientRequest(ClientRequest),
}

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
