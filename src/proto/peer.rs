/// Metadata related to an API frame.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Meta {
    /// The ID of this request or response frame.
    #[prost(string, tag="1")]
    pub id: std::string::String,
}
/// A peer to peer message frame.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Frame {
    /// The metadata of the frame.
    #[prost(message, optional, tag="1")]
    pub meta: ::std::option::Option<Meta>,
    /// The payload of data for this frame.
    #[prost(oneof="frame::Payload", tags="2, 3, 4")]
    pub payload: ::std::option::Option<frame::Payload>,
}
pub mod frame {
    /// The payload of data for this frame.
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Payload {
        #[prost(enumeration="super::Disconnect", tag="2")]
        Disconnect(i32),
        #[prost(message, tag="3")]
        Request(super::Request),
        #[prost(message, tag="4")]
        Response(super::Response),
    }
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// Frame Variants ////////////////////////////////////////////////////////////////////////////////

/// A request frame sent between cluster peers.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Request {
    #[prost(oneof="request::Segment", tags="1, 2, 3, 4")]
    pub segment: ::std::option::Option<request::Segment>,
}
pub mod request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Segment {
        #[prost(message, tag="1")]
        Handshake(super::Handshake),
        #[prost(message, tag="2")]
        Raft(super::RaftRequest),
        #[prost(message, tag="3")]
        Routing(super::RoutingInfo),
        #[prost(message, tag="4")]
        Forwarded(super::ForwardedClientRequest),
    }
}
/// A response to an earlier sent request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Response {
    #[prost(oneof="response::Segment", tags="1, 2, 3, 4, 5")]
    pub segment: ::std::option::Option<response::Segment>,
}
pub mod response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Segment {
        #[prost(enumeration="super::Error", tag="1")]
        Error(i32),
        #[prost(message, tag="2")]
        Handshake(super::Handshake),
        #[prost(message, tag="3")]
        Raft(super::RaftResponse),
        #[prost(message, tag="4")]
        Routing(super::RoutingInfo),
        #[prost(message, tag="5")]
        Forwarded(super::ForwardedClientResponse),
    }
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// Components ////////////////////////////////////////////////////////////////////////////////////

/// A description of a client's state for data routing.
///
/// This data is used for ephemeral messaging, RPCs & stream consumer routing. Ephemeral
/// messaging & RPC routing info is ephemeral, and held only in memory. Stream consumer
/// information is propagated up to the app level, but only durable consumer information is
/// persisted on disk via Raft.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ClientInfo {
    #[prost(message, repeated, tag="1")]
    pub messaging: ::std::vec::Vec<MessagingSub>,
    #[prost(message, repeated, tag="2")]
    pub rpc: ::std::vec::Vec<RpcSub>,
    #[prost(message, repeated, tag="3")]
    pub streams: ::std::vec::Vec<StreamSub>,
    #[prost(message, repeated, tag="4")]
    pub pipelines: ::std::vec::Vec<PipelineSub>,
}
/// A node's client routing info.
///
/// The ID of the node to which this info pertains is established during the peer handshake.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RoutingInfo {
    /// A mapping of all clients curently connected to this node, by ID.
    #[prost(map="string, message", tag="2")]
    pub client_info: ::std::collections::HashMap<std::string::String, ClientInfo>,
}
/// Details of a client ephemeral messaging subscription.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MessagingSub {
}
/// Details of a client RPC subscription.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RpcSub {
}
/// Details of a client Stream subscription.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSub {
}
/// Details of a client Pipeline subscription.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineSub {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// Handshake /////////////////////////////////////////////////////////////////////////////////////

/// A handshake frame holding all data needed for a successful handshake between peers.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Handshake {
    /// The ID of the node sending this frame.
    #[prost(uint64, tag="1")]
    pub node_id: u64,
    /// The sending node's client routing info.
    #[prost(message, optional, tag="2")]
    pub routing: ::std::option::Option<RoutingInfo>,
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// Raft Request & Response ///////////////////////////////////////////////////////////////////////

/// A Raft request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RaftRequest {
    #[prost(oneof="raft_request::Payload", tags="1, 2, 3")]
    pub payload: ::std::option::Option<raft_request::Payload>,
}
pub mod raft_request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Payload {
        #[prost(bytes, tag="1")]
        AppendEntries(std::vec::Vec<u8>),
        #[prost(bytes, tag="2")]
        Vote(std::vec::Vec<u8>),
        #[prost(bytes, tag="3")]
        InstallSnapshot(std::vec::Vec<u8>),
    }
}
/// A Raft response.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RaftResponse {
    #[prost(oneof="raft_response::Payload", tags="1, 2, 3")]
    pub payload: ::std::option::Option<raft_response::Payload>,
}
pub mod raft_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Payload {
        #[prost(bytes, tag="1")]
        AppendEntries(std::vec::Vec<u8>),
        #[prost(bytes, tag="2")]
        Vote(std::vec::Vec<u8>),
        #[prost(bytes, tag="3")]
        InstallSnapshot(std::vec::Vec<u8>),
    }
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// Forwarded Client Request & Response ///////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ForwardedClientRequest {
    /// This must be a bincode serialization of an `RgClientPayload`, else will cause an internal error.
    ///
    /// NOTE WELL: though the Raft encapsulating types are unlikely to change, care must be taken to
    /// ensure that all versions of Railgun running in the cluster are able to properly encode/decode
    /// this value. The standard serde backwards compatibility rules apply here.
    #[prost(bytes, tag="1")]
    pub payload: std::vec::Vec<u8>,
}
/// A response to a client payload forwarding request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ForwardedClientResponse {
    #[prost(oneof="forwarded_client_response::Result", tags="1, 2")]
    pub result: ::std::option::Option<forwarded_client_response::Result>,
}
pub mod forwarded_client_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Result {
        /// This must be a bincode serialization of an `AppDataResponse`, else will cause an internal error.
        ///
        /// NOTE WELL: care must be taken to ensure that all versions of Railgun running in the cluster
        /// are able to properly encode/decode this value. The standard serde backwards compatibility
        /// rules apply here.
        #[prost(bytes, tag="1")]
        Data(std::vec::Vec<u8>),
        /// This must be a bincode serialization of an `AppDataError`, else will cause an internal error.
        ///
        /// NOTE WELL: care must be taken to ensure that all versions of Railgun running in the cluster
        /// are able to properly encode/decode this value. The standard serde backwards compatibility
        /// rules apply here.
        #[prost(bytes, tag="2")]
        Error(std::vec::Vec<u8>),
    }
}
/// A frame indicating that the peer connection must disconnect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Disconnect {
    /// The disconnect frame has been sent because the connection is no longer valid..
    ConnectionInvalid = 0,
}
/// A peer error variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Error {
    /// An internal error has taken place. The request should be safe to retry, if related to a request.
    Internal = 0,
    //// The request has hit a timeout.
    Timeout = 1,
    //// The target peer no longer has an active connection.
    TargetPeerDisconnected = 2,
}
