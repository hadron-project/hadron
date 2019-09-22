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
        #[prost(message, tag="2")]
        Request(super::Request),
        #[prost(message, tag="3")]
        Response(super::Response),
        #[prost(enumeration="super::Disconnect", tag="4")]
        Disconnect(i32),
    }
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// Frame Variants ////////////////////////////////////////////////////////////////////////////////

/// A request from a peer node.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Request {
    #[prost(oneof="request::Segment", tags="1, 2")]
    pub segment: ::std::option::Option<request::Segment>,
}
pub mod request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Segment {
        #[prost(message, tag="1")]
        Handshake(super::Handshake),
        #[prost(message, tag="2")]
        Raft(super::RaftRequest),
    }
}
/// A response to an earlier sent request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Response {
    #[prost(oneof="response::Segment", tags="1, 2")]
    pub segment: ::std::option::Option<response::Segment>,
}
pub mod response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Segment {
        #[prost(message, tag="1")]
        Handshake(super::Handshake),
        #[prost(message, tag="2")]
        Raft(super::RaftResponse),
    }
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// Handshake /////////////////////////////////////////////////////////////////////////////////////

/// A handshake frame holding all data needed for a successful handshake between peers.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Handshake {
    #[prost(uint64, tag="1")]
    pub node_id: u64,
    #[prost(string, tag="2")]
    pub routing_info: std::string::String,
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
/// A disconnect variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Disconnect {
    ConnectionInvalid = 0,
}
