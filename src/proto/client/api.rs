//////////////////////////////////////////////////////////////////////////////////////////////////
// ClientToServer & ClientFromServer /////////////////////////////////////////////////////////////

/// A client request frame.
///
/// Client requests come in a few different forms:
/// - Connnect: a request to establish a client connection.
/// - Disconnect: a request sent to disconnect the client from the server.
/// - PubEphemeral: a request to publish an ephemeral message.
/// - PubRpc: a request to publish an RPC message.
/// - PubStream: a request to publish a message to a durable stream.
/// - SubEphemeral: a request to subscribe to an ephemeral messaging exchange.
/// - SubRpc: a request to subscribe as a handler of an RPC endpoint.
/// - SubStream: a request to subscribe as a stream consumer.
/// - SubPipeline: a request to subscribe to a stage of a pipeline (RPC endpoint or stream).
/// - UnsubStream: unsubscribe from a stream.
/// - UnsubPipeline: unsubscribe from a stage of a pipeline.
/// - EnsureRpcEndpoint: a request to ensure that the specified RPC endpoint exists.
/// - EnsureStream: a request to ensure that the specified stream exists with the given config.
/// - EnsurePipeline: a request to ensure that the specified pipeline exists with the
/// given structure and config.
/// - StreamAck: a request to ack a stream message. Ack'ing a stream message may also be
/// accompanied by a set of messages to be published to other streams. See the guide for more
/// details on how this works in the Durable Streams chapter.
/// - PipelineAck: a request to ack a message from a pipeline stage. This request must include
/// the payload of data which is to be written to downstream stages. Ack'ing the message & writing
/// its output to downstream stages is done transactionally. This request is used even if the stage
/// is an RPC endpoint stage, in which case only the data is written for the downstream stages.
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct ClientFrame {
    #[prost(message, optional, tag="1")]
    pub meta: ::std::option::Option<FrameMeta>,
    #[prost(oneof="client_frame::Payload", tags="2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17")]
    pub payload: ::std::option::Option<client_frame::Payload>,
}
pub mod client_frame {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    #[derive(Serialize, Deserialize)]
    pub enum Payload {
        #[prost(message, tag="2")]
        Connect(super::ConnectRequest),
        #[prost(message, tag="3")]
        Disconnect(super::DisconnectRequest),
        #[prost(message, tag="4")]
        PubEphemeral(super::PubEphemeralRequest),
        #[prost(message, tag="5")]
        PubRpc(super::PubRpcRequest),
        #[prost(message, tag="6")]
        PubStream(super::PubStreamRequest),
        #[prost(message, tag="7")]
        SubEphemeral(super::SubEphemeralRequest),
        #[prost(message, tag="8")]
        SubRpc(super::SubRpcRequest),
        #[prost(message, tag="9")]
        SubStream(super::SubStreamRequest),
        #[prost(message, tag="10")]
        SubPipeline(super::SubPipelineRequest),
        #[prost(message, tag="11")]
        UnsubStream(super::UnsubStreamRequest),
        #[prost(message, tag="12")]
        UnsubPipeline(super::UnsubPipelineRequest),
        #[prost(message, tag="13")]
        EnsureEndpoint(super::EnsureRpcEndpointRequest),
        #[prost(message, tag="14")]
        EnsureStream(super::EnsureStreamRequest),
        #[prost(message, tag="15")]
        EnsurePipeline(super::EnsurePipelineRequest),
        #[prost(message, tag="16")]
        AckStream(super::AckStreamRequest),
        #[prost(message, tag="17")]
        AckPipeline(super::AckPipelineRequest),
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct ServerFrame {
    #[prost(message, optional, tag="1")]
    pub meta: ::std::option::Option<FrameMeta>,
    #[prost(oneof="server_frame::Payload", tags="2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17")]
    pub payload: ::std::option::Option<server_frame::Payload>,
}
pub mod server_frame {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    #[derive(Serialize, Deserialize)]
    pub enum Payload {
        #[prost(message, tag="2")]
        Connect(super::ConnectResponse),
        #[prost(message, tag="3")]
        Disconnect(super::DisconnectResponse),
        #[prost(message, tag="4")]
        PubEphemeral(super::PubEphemeralResponse),
        #[prost(message, tag="5")]
        PubRpc(super::PubRpcResponse),
        #[prost(message, tag="6")]
        PubStream(super::PubStreamResponse),
        #[prost(message, tag="7")]
        SubEphemeral(super::SubEphemeralResponse),
        #[prost(message, tag="8")]
        SubRpc(super::SubRpcResponse),
        #[prost(message, tag="9")]
        SubStream(super::SubStreamResponse),
        #[prost(message, tag="10")]
        SubPipeline(super::SubPipelineResponse),
        #[prost(message, tag="11")]
        UnsubStream(super::UnsubStreamResponse),
        #[prost(message, tag="12")]
        UnsubPipeline(super::UnsubPipelineResponse),
        #[prost(message, tag="13")]
        EnsureEndpoint(super::EnsureRpcEndpointResponse),
        #[prost(message, tag="14")]
        EnsureStream(super::EnsureStreamResponse),
        #[prost(message, tag="15")]
        EnsurePipeline(super::EnsurePipelineResponse),
        #[prost(message, tag="16")]
        AckStream(super::AckStreamResponse),
        #[prost(message, tag="17")]
        AckPipeline(super::AckPipelineResponse),
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct FrameMeta {
    /// The ID of the associated request.
    ///
    /// This is used to establish request/response semantics over the bi-directional stream. These
    /// IDs should have strong uniqueness guarantees. Clients are encouraged to use UUID4s, which is
    /// what the server uses for server side initiated frames sent to clients.
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
//////////////////////////////////////////////////////////////////////////////////////////////////
// Common Components /////////////////////////////////////////////////////////////////////////////

/// The ID of a stream entry.
///
/// This wrapper type is used to allow for leaving the ID field null when there is no assocaited ID.
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct StreamEntryId {
    /// The associated ID. Even an empty string will be treated as an ID.
    #[prost(string, tag="1")]
    pub id: std::string::String,
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// ConnectRequest & ConnectResponse //////////////////////////////////////////////////////////////

/// A request to connect to the cluster.
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct ConnectRequest {
    /// The JWT credentials being used for this connection.
    #[prost(string, tag="1")]
    pub token: std::string::String,
    /// The configured liveness threshold for this client connection.
    ///
    /// The server will treat this as a duration in seconds since the client's last heartbeat, after
    /// which the client connection will be closed by the server.
    #[prost(uint32, tag="2")]
    pub liveness_threshold: u32,
}
/// A response to a connection request.
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct ConnectResponse {
    #[prost(oneof="connect_response::Response", tags="1, 2")]
    pub response: ::std::option::Option<connect_response::Response>,
}
pub mod connect_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    #[derive(Serialize, Deserialize)]
    pub enum Response {
        /// An error associated with this response. If this field is populated, no other fields should be used.
        #[prost(message, tag="1")]
        Error(super::ClientError),
        /// The ID assigned to this connection by the server.
        #[prost(string, tag="2")]
        Id(std::string::String),
    }
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// DisconnectRequest & DisconnectResponse ////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct DisconnectRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct DisconnectResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// PubEphemeralRequest & PubEphemeralResponse ////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct PubEphemeralRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct PubEphemeralResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// PubRpcRequest & PubRpcResponse ////////////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct PubRpcRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct PubRpcResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// PubStreamRequest & PubStreamResponse //////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct PubStreamRequest {
    /// The namespace of the stream to publish to.
    #[prost(string, tag="1")]
    pub namespace: std::string::String,
    /// The name of the stream to publish to.
    #[prost(string, tag="2")]
    pub stream: std::string::String,
    /// The data payload of the entry to publish.
    ///
    /// // The ID of the entry. Leave null if there is no associated ID.
    /// StreamEntryId id = 4;
    #[prost(bytes, tag="3")]
    pub payload: std::vec::Vec<u8>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct PubStreamResponse {
    #[prost(oneof="pub_stream_response::Result", tags="1, 2")]
    pub result: ::std::option::Option<pub_stream_response::Result>,
}
pub mod pub_stream_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    #[derive(Serialize, Deserialize)]
    pub enum Result {
        /// An error associated with this response.
        #[prost(message, tag="1")]
        Error(super::ClientError),
        /// The index of the published stream entry.
        #[prost(uint64, tag="2")]
        Index(u64),
    }
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// SubEphemeralRequest & SubEphemeralResponse ////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct SubEphemeralRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct SubEphemeralResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// SubRpcRequest & SubRpcResponse ////////////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct SubRpcRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct SubRpcResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// SubStreamRequest & SubStreamResponse //////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct SubStreamRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct SubStreamResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// SubPipelineRequest & SubPipelineResponse //////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct SubPipelineRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct SubPipelineResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// UnsubStreamRequest & UnsubStreamResponse //////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct UnsubStreamRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct UnsubStreamResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// UnsubPipelineRequest & UnsubPipelineResponse //////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct UnsubPipelineRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct UnsubPipelineResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// EnsureRpcEndpointRequest & EnsureRpcEndpointResponse //////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct EnsureRpcEndpointRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct EnsureRpcEndpointResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// EnsureStreamRequest & EnsureStreamResponse ////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct EnsureStreamRequest {
    /// The namespace which the ensured stream should exist in.
    #[prost(string, tag="1")]
    pub namespace: std::string::String,
    /// The name of the stream to ensure.
    #[prost(string, tag="2")]
    pub name: std::string::String,
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct EnsureStreamResponse {
    /// An error associated with this response. Will be null if no error has taken place.
    #[prost(message, optional, tag="1")]
    pub error: ::std::option::Option<ClientError>,
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// EnsurePipelineRequest & EnsurePipelineResponse ////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct EnsurePipelineRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct EnsurePipelineResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// AckStreamRequest & AckStreamResponse //////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct AckStreamRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct AckStreamResponse {
}
//////////////////////////////////////////////////////////////////////////////////////////////////
// AckPipelineRequest & AckPipelineResponse //////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct AckPipelineRequest {
}
#[derive(Clone, PartialEq, ::prost::Message)]
#[derive(Serialize, Deserialize)]
pub struct AckPipelineResponse {
}
/// An enumeration of all error codes which may come from the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
#[derive(Serialize, Deserialize)]
pub enum ErrorCode {
    /// An internal error.
    Internal = 0,
    /// The server needs the client to perform the connection handshake before proceeding.
    HandshakeRequired = 1,
    /// The given credentials are invalid.
    Unauthorized = 2,
    /// The token being used by the client does not have sufficient permissions for the requested operation.
    InsufficientPermissions = 3,
    /// The given input is invalid.
    InvalidInput = 4,
    /// The target stream of the request is unknown.
    TargetStreamUnknown = 5,
}
