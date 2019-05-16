networking
==========
All networking communications between peer nodes and from client to server is handled by the `Connections` actor.

All Railgun level network communications, including the handshake protocol, are given request/response semantics over the bidirectional socket. Network communications which are at a lower level than the Railgun protocol are not covered here, and generally speaking are nothing more than establishing the baseline connection and sending/receiving ping/pongs for healthchecking.

Everything discussed in this document is in reference to the internal & public framing APIs declared in the `protobuf/` files respectively.

### peer to peer
All peer to peer communication takes place in terms of the internal wire protocol API. Other actors may communicate with the connections actor to have it send out a messages to a specific peer node by ID. The connections actor offers two message types for this. One will send the message to the peer and will not attempt to return any sort of response to the calling actor. The other will return a future, and may be configured to timeout.

##### implementation
Internally, the connections actor will consult its connections table to access the correct socket to send the message over. A full response frame is expected over the same socket. The `OutboundRequest` actor message type is used, which simply wraps an internal API frame. The individual actor which is responsible for actually sending the message will wrap the outbound message in the `Request` frame along with metadata on the message, which includes the message ID and any other metadata needed for proper communication.

The request ID is placed in a map which is used to keep track of any outstanding requests. When the socket receives a response frame from a peer, the ID of the original request will be used, and the frame will be placed in the map.

The original message handler will return a future which polls to see if a response has come back over the socket. A deadline may be supplied as part of the original `OutboundRequest` message. If a deadline is presented, then the deadline will be added to the `Request` frame when it is sent, so that the receiving peer can discard it if needed. The original message handler will also wrap the response future in a timeout matching the deadline. The outstanding request info will be dropped from the request map, and the timeout error will be returned to the calling actor.
