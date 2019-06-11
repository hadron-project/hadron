//////////////////////////////////////////////////////////////////////////////////////////////////
// ClientRequest & ClientResponse ////////////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ClientRequest {
    #[prost(oneof="client_request::Payload", tags="1")]
    pub payload: ::std::option::Option<client_request::Payload>,
}
pub mod client_request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Payload {
        #[prost(message, tag="1")]
        Connect(super::ConnectRequest),
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ClientResponse {
    #[prost(oneof="client_response::Payload", tags="1")]
    pub payload: ::std::option::Option<client_response::Payload>,
}
pub mod client_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Payload {
        #[prost(message, tag="1")]
        Connect(super::ConnectResponse),
    }
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// ConnectRequest & Connect Response /////////////////////////////////////////////////////////////

/// A request to connect to the cluster.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConnectRequest {
    /// The optional ID of the connection.
    ///
    /// Normally the server will generate connection IDs, but during reconnect scenarios, a
    /// connection ID from a previously lost connection may be supplied.
    #[prost(string, tag="1")]
    pub id: std::string::String,
    /// The rate at which the client would like heartbeats to be sent from the server, in seconds.
    #[prost(uint32, tag="2")]
    pub hb_rate: u32,
    /// The number of heartbeats which can be consecutively missed before being considered dead.
    #[prost(uint32, tag="3")]
    pub hb_max_missed: u32,
}
/// A response to a connection request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConnectResponse {
    /// The ID assigned to this connection by the server.
    #[prost(string, tag="1")]
    pub id: std::string::String,
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// Errors ////////////////////////////////////////////////////////////////////////////////////////

/// An error which has taken place as the result of a client request.
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct ClientError {
    #[prost(string, tag="1")]
    pub message: std::string::String,
    #[prost(enumeration="ErrorCode", tag="2")]
    pub code: i32,
}
/// An enumeration of all error codes which may come from the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum ErrorCode {
    /// An internal error.
    Internal = 0,
}
