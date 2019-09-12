peer connection management
==========================
This document describes the lifecycle of network connections between peer nodes within a Railgun cluster.

#### discovery
The first phase of the lifecycle is discovery.

When a node IP is first picked up from the discovery system, it will be sent over to the `Network` actor to establish a connection with the discovered peer. Peers which were once in the observed set of discovered peers, but which are no longer appearing in discovery probes, will be sent over to the `Network` actor so that action can be taken to remove them if needed.

At this point in time, the Railgun networking system does not attempt to accommodate potential discovery misconfigurations. If a node within the cluster is given two IPs, and the discovery system detects both, one of them will be dropped due to peers already having a connection to the same node and peer nodes will not attempt to connect to the second IP again. The proper way to change the IP of a node is to bring the node down, change its IP, and then bring it back online.

#### baseline connection
Newly discovered members will be immediately connected to as long as the `Network` actor determines that there is no live connection to the same target IP.

Live connections to a discovered node may happen when:
- A peer is already connected and is healthy.
- The discovery system has an interrupt where the peer is no longer showing up.
- The peer's connection is still live and healthy despite not showing up in the discovery system.
- The peer's discovery entry shows up again at some point in the future, at which point the discovery system would see it as a new peer, and would then publish a changeset to the discovery subscribers.
- The `Network` actor picks up the alleged new peer, but then sees that it already has an open connection to it.

Once it is determined that a peer should be connected to, a WebSocket connection will be established between the peers for baseline communication. If the initial connection fails, it will execute the [reconnect protocol](#reconnect) described below. The peer which initiated the connection will then immediately launch into the Railgun peer handshake protocol.

#### railgun peer handshake
Immediately after the baseline WebSocket connection is made between peers, the initiator of the connection will begin the Railgun peer handshake protocol. The protocol is composed of only a single network round trip.

It is important to note that it is always the initiator of the connection which will drive the Railgun peer handshake protocol, the receiving end will follow.

In this section, there are two nodes which are performing the handshake protocol. `NodeA` is the node which initiated the connection. `NodeB` is the node which is responding to the handshake.

###### workflow
- NodeA will send the handshake frame to NodeB containing its node ID and routing information.
- NodeB will check if it already has an open connection to a node bearing the same ID.
    - If it does, NodeB will respond to NodeA with a disconnect frame indicating such, drop the connection, and will do nothing else.
    - If the presented node ID is NodeB's own node ID, then NodeB will respond to NodeA with a disconnect frame indicating such.
    - If it does not, then NodeB will send the handshake frame to NodeA with its node ID and routing information. NodeB will now reckon this connection as being live and will then update its routing table and connection info.
- NodeA will receive the handshake response frame and will also check if it already has an open connection to a node bearing the same ID.
    - If it does, NodeA will send a disconnect frame to NodeB incidating the reason, drop the connection, and will do nothing else.
    - If it does not, the handshake will be considered complete. NodeA will now reckon this connection as being live and will then update its routing table and connection info.

The routing information exchanged during the handshake is comprised of each peer's connected peer IPs and routing information on any connected clients.

#### reconnect
Inevitibly, nodes within Railgun clusters will fail, sometimes partially, sometimes critically. This is often due to transient network issues, but is sometimes due to critical hardware failures. When such conditions arise, the actors responsible for holding open the connections to their peers will attempt to perform a reconnect by default. Only the initiator of the original connection will perform the reconnect protocol. The reconnect protocol is as follows:

- When a peer has gone non-responsive and the connection is deemed as being unhealthy, a reconnect will be attempted. The initiator of the original connection will begin the reconnect protocol without fully closing down. The receiver of the original connection will simply perform its cleanup routine on the connection and drop it.
- If the initial reconnect fails, then the actor will check to see if the current connection has been flagged for removal due to its absence in the discovery system. If it has been flagged for removal, then the connection will be cleaned up, removed from the routing table, and the actor will shut itself down.
- If it has not been flagged for removal, then the reconnect routine will continue in this cycle, until the connection is re-established or one of the above conditions is met.
