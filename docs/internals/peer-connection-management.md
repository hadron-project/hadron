peer connection management
==========================
This document describes the lifecycle of network connections between Railgun cluster peers.

#### discovery
The first phase of the lifecycle is discovery.

When a node IP is first picked up from the discovery system, it will be sent over to the connections actor to establish a connection with the discovered peer. Peers which were once in the observed set of discovered peers, but which are no longer appearing in discovery probes, will be sent over to the connections actor so that action can be taken to remove them if needed.

#### baseline connection
Newly discovered members will be immediately connected to as long as the connections actor determines that there is no live connection to the same target IP. This will typically only happen when:
- A peer is already connected to and is healthy.
- The discovery system has an interrupt where the peer is no longer showing up.
- The peer's connection is still live and healthy despite not showing up in the discovery system.
- The peer's discovery entry shows up again at some point in the future, at which point the discovery system would see it as a new peer, and would then publish a changeset to the discovery subscribers.
- The connections actor picks up the alleged new peer, but then sees that it already has an open connection to it.

Once it is determined that a peer should be connected to, a WebSocket connection will be established between the peers for baseline communication. If the initial connection fails, it will execute the [reconnect protocol](#reconnect) described below. The peer which initiated the connection will then immediately launch into the Railgun peer handshake protocol.

#### railgun peer handshake
Immediately after the baseline WebSocket connection is made between peers, the initiator of the connection will begin the Railgun peer handshake protocol. The protocol is composed of only a few steps:

- `initiate`: "initiate" frames are sent to present node IDs and establish if the connection is valid.
- `confirm`: "confirm" frames are sent to confirm the connection and exchange routing info.

It is important to note that it is always the initiator of the connection which will drive the Railgun peer handshake protocol, the receiving end will follow.

In this section, there are two nodes which are performing the handshake protocol. `NodeA` is the node which initiated the connection. `NodeB` is the node which is responding to the handshake.

###### initial
- NodeA will send the "initiate" frame to NodeB containing its node ID.
- NodeB will check that it does not already have an open connection to a node bearing the same ID.
    - If it does, NodeB will remove the connection from its routing table, respond to NodeA with a disconnect frame, drop the connection, and will do nothing else.
    - If it does not, then NodeB will send an "initiate" frame to NodeA with its node ID.
- NodeA will also check that it does not already have an open connection to a node bearing the same ID.
    - If it does, NodeA will remove the connection from its routing table, **register the new IP as a failover IP for the current open connection with the target node,** send a disconnect frame to NodeB, drop the connection, and will do nothing else.
    - If it does not, the handshake will continue to the next phase.

###### confirm
At this point in the handshake, the connection is well established and only a final exchange of information is needed.

- NodeA will send the "confirm" frame to NodeB containing a list of its connected peer IPs and routing information on any connected clients.
- NodeB will evaluate the received data, potentially attempting to establish new connections if new peer IPs are discovered, and updating its client routing table with any new information. It will then send a "confirm" frame to NodeA with the same information taken from its own state. The connection will node be considered "live" on NodeB's side.
- NodeA will evaluate the received data, performing the same routines as NodeB as needed. The connection will now be considered "live" on NodeA's side.

#### reconnect
Inevitibly, nodes within Railgun clusters will fail, sometimes partially, sometimes critically. This is often due to transient network issues, but is sometimes due to critical hardware failures. When such conditions arise, the actors responsible for holding open the connections to their peers will attempt to perform a reconnect by default. Only the initiator of the original connection will perform the reconnect protocol. The reconnect protocol is as follows:

- When a peer has gone non-responsive and the connection is deemed as being unhealthy, a reconnect will be attempted. The initiator of the original connection will begin the reconnect protocol, the receiver of the original connection will simply perform its cleanup routine on the connection and drop it.
- If the initial reconnect fails, the actor holding the connection will check to see if there is a failover IP for the connection.
    - If there is a failover IP, then the actor will clean up the current connection, removing it from the routing table, and will then command the connections actor to create a new connection to the peer using the failover IP. At that point, it will be a new connection.
    - If not, then the actor will check to see if the current connection has been flagged for removal due to its absence in the discovery system. If it has been flagged for removal, then the connection will be cleaned up, removed from the routing table, and the actor will shut itself down.
- If it does not have a failover IP and has not been flagged for removal, then the reconnect routine will continue in this cycle, until the connection is re-established or one of the above conditions is met.
