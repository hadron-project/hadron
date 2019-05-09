connections
===========
The connections actor is responsible for handling all network activity throughout the system.

The responsibility of the connections actor is as follows:
- Own and manage the network stack.
- Defines and upholds the systems network APIs, including the public facing client API & the internal server API.
- Initiate connections to discovered peers.
- Handle connection requests from peers.
- Perform connection handshake protocol with peer servers.
- Handle client connections.
- Inbound & outbound message routing.
- Maintain a set of connected peers internally. This is used for many of the other responsibilities.

This module also includes three other primary actors: `WsFromPeer`, `WsToPeer` & `WsClient`. They are described below.

### `WsFromPeer`
The `WsFromPeer` actor represents a WebSocket connection which was initialized by a peer cluster member.

### `WsToPeer`
The `WsToPeer` actor represents a WebSocket connection which was initialized by the source node and sent to a cluster peer.

### `WsClient`
The `WsClient` actor represents a WebSocket connection which was initialized by a client. The client may be written in any number of different languages, but all clients must adhere to the Railgun client wire-protocol (the Railgun Protocol, or `RGP` for short) in order to successfully communicate with the cluster.

Client requests may be forwarded along to other members of the cluster as needed in order to satisfy the clients request.
