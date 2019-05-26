/// Metadata related to an API frame.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Meta {
    /// The ID of this request or response frame.
    #[prost(string, tag="1")]
    pub id: std::string::String,
    /// The deadline for this request in milliseconds since the epoch.
    #[prost(int64, tag="2")]
    pub deadline: i64,
}
/// An API frame.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Frame {
    #[prost(message, optional, tag="1")]
    pub meta: ::std::option::Option<Meta>,
    #[prost(oneof="frame::Payload", tags="2, 3, 4")]
    pub payload: ::std::option::Option<frame::Payload>,
}
pub mod frame {
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
/// A request from a peer node.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Request {
    #[prost(oneof="request::Segment", tags="1")]
    pub segment: ::std::option::Option<request::Segment>,
}
pub mod request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Segment {
        #[prost(message, tag="1")]
        Handshake(super::super::handshake::Handshake),
    }
}
/// A response to an earlier sent request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Response {
    #[prost(oneof="response::Segment", tags="1")]
    pub segment: ::std::option::Option<response::Segment>,
}
pub mod response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Segment {
        #[prost(message, tag="1")]
        Handshake(super::super::handshake::Handshake),
    }
}
/// A disconnect variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Disconnect {
    ConnectionInvalid = 0,
}
