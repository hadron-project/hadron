//! A module encapsulating the `WsClient` actor and its logic.
//!
//! The `WsClient` actor represents a WebSocket connection which was initialized by a client. The
//! client may be written in any number of different languages, but all clients must adhere to the
//! Railgun Client Wire Protocol in order to successfully communicate with the cluster.
//!
//! Client requests may be forwarded along to other members of the cluster as needed in order
//! to satisfy the clients request.
