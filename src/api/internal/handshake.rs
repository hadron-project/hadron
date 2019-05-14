/// A handshake request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Request {
    #[prost(oneof="request::Frame", tags="1")]
    pub frame: ::std::option::Option<request::Frame>,
}
pub mod request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Frame {
        #[prost(message, tag="1")]
        Handshake(super::Initial),
    }
}
/// A handshake response.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Response {
    #[prost(oneof="response::Frame", tags="1")]
    pub frame: ::std::option::Option<response::Frame>,
}
pub mod response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Frame {
        #[prost(message, tag="1")]
        Handshake(super::Initial),
    }
}
/// The initial frame of a handshake.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Initial {
    #[prost(string, tag="1")]
    pub node_id: std::string::String,
}
/// The confirmation frame of a handshake.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Confirm {
    #[prost(string, tag="1")]
    pub routing_info: std::string::String,
}
