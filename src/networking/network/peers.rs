use std::{
    net::SocketAddr,
    time::Duration,
};

use actix::prelude::*;
use futures::{
    future::{Either, ok as fut_ok},
};
use log::{debug, error};

use crate::{
    NodeId,
    app::UpdatePeerInfo,
    networking::{
        network::Network,
        to_peer::{DiscoveryState, UpdateDiscoveryState, WsToPeer, WsToPeerServices},
        from_peer::WsFromPeer,
    },
    discovery::{ObservedPeersChangeset},
    proto::peer,
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// PeerAddr //////////////////////////////////////////////////////////////////////////////////////

/// A wrapper around the two possible peer connection actor types.
pub(in crate::networking) enum PeerAddr {
    ToPeer(Addr<WsToPeer>),
    FromPeer(Addr<WsFromPeer>),
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsToPeerState /////////////////////////////////////////////////////////////////////////////////

/// A type used to track the discovery state of a `WsToPeer`'s socket.
///
/// This helps to cut down on superfluous messaging.
pub(in crate::networking) struct WsToPeerState {
    addr: Addr<WsToPeer>,
    discovery_state: DiscoveryState,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ObservedPeersChangeset ////////////////////////////////////////////////////////////////////////

impl Handler<ObservedPeersChangeset> for Network {
    type Result = ();

    /// Handle changesets coming from the discovery system.
    ///
    /// See docs/internals/peer-connection-management.md for more details.
    ///
    /// Our main objectives here are:
    ///
    /// - Connect to new addrs if we don't already have connections to them.
    /// - Update the discovery state of connections. If they've disappeared from the discovery
    ///   system, then update its discovery state to `Disappeared`. If it had previously
    ///   disappeared, then update its discovery state to `Observed`.
    fn handle(&mut self, changeset: ObservedPeersChangeset, ctx: &mut Self::Context) -> Self::Result {
        // Determine which new addrs need to be spawned. Update discovery states & filter.
        let tospawn: Vec<_> = changeset.new_peers.into_iter()
            .filter(|socketaddr| match self.socketaddr_to_peer.get_mut(socketaddr) {
                // If a disappeared addr has come back, then update it, notify actor & filter.
                Some(ref mut state) if state.discovery_state == DiscoveryState::Disappeared => {
                    debug!("Updating discovery state of connection '{}' to 'Observed'.", &socketaddr);
                    state.discovery_state = DiscoveryState::Observed;
                    state.addr.do_send(UpdateDiscoveryState(DiscoveryState::Observed));
                    false
                }
                // Socket state is good, filter this element out.
                Some(_) => false,
                // New socket addrs will not be filtered out.
                None => true,
            }).collect();

        // Spawn new outbound connections to peers.
        tospawn.into_iter().for_each(|socketaddr| {
            debug!("Spawning a new outbound peer connection to '{}'.", &socketaddr);
            let services = WsToPeerServices::new(
                ctx.address().recipient(),
                ctx.address().recipient(),
                self.services.inbound_raft_request.clone(),
                ctx.address().recipient(),
            );
            let addr = WsToPeer::new(services, self.node_id, socketaddr, self.routing.clone()).start();
            let discovery_state = DiscoveryState::Observed;
            self.socketaddr_to_peer.insert(socketaddr, WsToPeerState{addr, discovery_state});
        });

        // For any addr which has disappeared from the discovery system, update it here if needed.
        changeset.purged_peers.into_iter()
            .for_each(|socketaddr| match self.socketaddr_to_peer.get_mut(&socketaddr) {
                None => (),
                Some(state) => {
                    debug!("Updating discovery state of connection '{}' to 'Disappeared'.", &socketaddr);
                    state.discovery_state = DiscoveryState::Disappeared;
                    state.addr.do_send(UpdateDiscoveryState(DiscoveryState::Disappeared));
                }
            });
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// PeerHandshakeState ////////////////////////////////////////////////////////////////////////////

/// The peer handshake protocol state.
///
/// The handshake protocol is a very simple protocol driven by the node responsible for
/// initializing the connection. The handshake protocol beings after the base WebSocket connection
/// has been successfully established. If the handshake protcol fails, the connection will be
/// droped.
///
/// [See the Railgun peer handshake docs](docs/internals/peer-connection-management.md#railgun-peer-handshake)
/// for more details on exactly how the protocol works, and thus how it is to be implemented here.
///
/// Once the handshake is finished, the connection will be available for general use.
pub(in crate::networking) enum PeerHandshakeState {
    /// The initial phase of the handshake protocol.
    Initial,
    /// The finished state of the handshake protocol.
    Done,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// PeerConnectionIdentifier //////////////////////////////////////////////////////////////////////

/// The element used to identify a peer connection.
///
/// For outbound connections, before a handshake is finished, only the socket addr will be
/// available to identify the connection. For inbound connections, neither will be available at
/// first, and the connection can only be identified by the NodeID accurately, which will only be
/// available after a successful Railgun protocol handshake.
pub(in crate::networking) enum PeerConnectionIdentifier {
    /// This variant is used by `WsFromPeer` connections which do not have the SocketAddr of the peer.
    NodeId(NodeId),
    /// This variant is used by `WsToPeer` connections which only have the SocketAddr of the peer before the handshake is complete.
    SocketAddr(SocketAddr),
    /// This variant is used by `WsToPeer` connections after a successful handshake as both data elements will be available.
    SocketAddrAndId(SocketAddr, NodeId),
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ClosingPeerConnection /////////////////////////////////////////////////////////////////////////

/// A message type used to indicate that a peer connection is closing.
#[derive(Message)]
pub(in crate::networking) struct ClosingPeerConnection(pub PeerConnectionIdentifier);

impl Handler<ClosingPeerConnection> for Network {
    type Result = ();

    /// Handle messages from peer connections indicating that the peer connection is closing.
    fn handle(&mut self, msg: ClosingPeerConnection, _ctx: &mut Self::Context) {
        match msg.0 {
            PeerConnectionIdentifier::NodeId(id) => {
                self.routing_table.remove(&id);
                let _ = self.services.update_peer_info.do_send(UpdatePeerInfo::Remove(id))
                    .map_err(|err| error!("Error sending `UpdatePeerInfo::Remove` from `Network` actor. {}", err));
            }
            PeerConnectionIdentifier::SocketAddr(addr) => {
                self.socketaddr_to_peer.remove(&addr);
            }
            PeerConnectionIdentifier::SocketAddrAndId(addr, id) => {
                self.routing_table.remove(&id);
                self.socketaddr_to_peer.remove(&addr);
                let _ = self.services.update_peer_info.do_send(UpdatePeerInfo::Remove(id))
                    .map_err(|err| error!("Error sending `UpdatePeerInfo::Remove` from `Network` actor. {}", err));
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// PeerConnectionLive ////////////////////////////////////////////////////////////////////////////

/// A message indicating that a connection with a specific peer is now live.
pub(in crate::networking) struct PeerConnectionLive {
    /// The ID of the peer with which the connection is now live.
    pub peer_id: NodeId,
    /// Routing info coming from the newly connected peer.
    pub routing: peer::RoutingInfo,
    /// The address of the actor which is responsible for the new connection.
    pub addr: PeerAddr,
}

impl Message for PeerConnectionLive {
    type Result = ();
}

impl Handler<PeerConnectionLive> for Network {
    type Result = ();

    /// Handle messages from child actors indicating that their connections are now live.
    ///
    /// This routine is responsible for a checking to ensure that the newly connected peer doesn't
    /// already have a connection. If it does, it will sever the old connection, replacing it with
    /// the new connection.
    ///
    /// It will also update this actor's internal state to map the peer's node ID to the actor's
    /// address for message routing, and will propagate the routing info of the newly connected
    /// peer to the App actor for high-level controls.
    fn handle(&mut self, msg: PeerConnectionLive, _ctx: &mut Self::Context) {
        // Update routing table with new information.
        let old = self.routing_table.insert(msg.peer_id, msg.addr);

        // If an old connection has been replaced, disconnect it.
        match old {
            Some(PeerAddr::FromPeer(addr)) => addr.do_send(DisconnectPeer),
            Some(PeerAddr::ToPeer(addr)) => addr.do_send(DisconnectPeer),
            None => (),
        }
        self.routing_table.keys().for_each(|peer| debug!("Is connected to: {}", peer));

        // Update app instance with new connection info.
        let _ = self.services.update_peer_info.do_send(UpdatePeerInfo::Update{peer: msg.peer_id, routing: msg.routing})
            .map_err(|err| error!("Error sending `UpdatePeerInfo::Update` from `Network` actor. {}", err));
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// DisconnectPeer ////////////////////////////////////////////////////////////////////////////////

/// A message instructing a peer connection to be severed.
#[derive(Message)]
pub(in crate::networking) struct DisconnectPeer;

//////////////////////////////////////////////////////////////////////////////////////////////////
// OutboundPeerRequest ///////////////////////////////////////////////////////////////////////////

/// A wrapper type for outbound requests destined for a specific peer.
///
/// The parent `Network` actor will receive messages from other higher-level actors to have
/// messages sent to specific destinations by Node ID. Different message types are used by other
/// higher-level actors, as different actors may require different intnerfaces, but they should
/// all use this message type in their handlers to interact with the peer connection actors.
pub(in crate::networking) struct OutboundPeerRequest {
    pub request: peer::Request,
    pub target_node: NodeId,
    pub timeout: Duration,
}

impl actix::Message for OutboundPeerRequest {
    type Result = Result<peer::Response, ()>;
}

impl Network {
    /// Send the outbound request to the target peer.
    ///
    /// NOTE WELL: any errors taking place within the actors which handle these requests will be
    /// transformed into a Response error variant.
    pub(in crate::networking) fn send_outbound_peer_request(&mut self, msg: OutboundPeerRequest, _: &mut Context<Self>) -> impl Future<Item=peer::Response, Error=()> {
        // Get a reference to actor which is holding the connection to the target node.
        let addr = match self.routing_table.get(&msg.target_node) {
            None => return Either::B(fut_ok(peer::Response::new_error(peer::Error::TargetPeerDisconnected))),
            Some(addr) => addr,
        };

        // Send the outbound request to the target node.
        match addr {
            PeerAddr::FromPeer(iaddr) => Either::A(Either::A(iaddr.send(msg).then(|res| match res {
                Ok(inner) => inner,
                Err(_) => Ok(peer::Response::new_error(peer::Error::Internal)),
            }))),
            PeerAddr::ToPeer(iaddr) => Either::A(Either::B(iaddr.send(msg).then(|res| match res {
                Ok(inner) => inner,
                Err(_) => Ok(peer::Response::new_error(peer::Error::Internal)),
            }))),
        }
    }
}
