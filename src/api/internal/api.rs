/// Metadata describing a request or response message.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Meta {
}
/// A request from a peer node.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Request {
    #[prost(message, optional, tag="1")]
    pub meta: ::std::option::Option<Meta>,
    #[prost(oneof="request::Frame", tags="2")]
    pub frame: ::std::option::Option<request::Frame>,
}
pub mod request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Frame {
        #[prost(message, tag="2")]
        Handshake(super::super::handshake::Request),
    }
}
/// A response to an earlier sent request from a peer node.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Response {
    #[prost(message, optional, tag="1")]
    pub meta: ::std::option::Option<Meta>,
    #[prost(oneof="response::Frame", tags="2")]
    pub frame: ::std::option::Option<response::Frame>,
}
pub mod response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Frame {
        #[prost(message, tag="2")]
        Handshake(super::super::handshake::Response),
    }
}
