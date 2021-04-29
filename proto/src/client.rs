///////////////////////////////////////////////////////////////////////////////
// Components /////////////////////////////////////////////////////////////////

/// An empty message.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Empty {
}
/// An error object which is returned from the Hadron server under various conditions.
///
/// Clients can match on specific error variants to drive behavior.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Error {
    /// A summary of the error which has taken place.
    #[prost(string, tag="1")]
    pub message: ::prost::alloc::string::String,
}
/// A stream record with its associated offset and data.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Record {
    /// The offset of this record.
    #[prost(uint64, tag="1")]
    pub offset: u64,
    /// The data payload of this record.
    #[prost(bytes="vec", tag="2")]
    pub data: ::prost::alloc::vec::Vec<u8>,
}
/// Details on a Hadron replica set.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReplicaSet {
    /// The name of the replica set.
    ///
    /// This is immutable and is always used to identify partition assignment.
    #[prost(string, tag="1")]
    pub name: ::prost::alloc::string::String,
}
///////////////////////////////////////////////////////////////////////////////
// Metadata ///////////////////////////////////////////////////////////////////

/// All known Hadron metadata.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MetadataResponse {
    /// The name of the cluster which was queried.
    #[prost(string, tag="1")]
    pub cluster_name: ::prost::alloc::string::String,
    /// Details on the replica set which was queried.
    #[prost(string, tag="2")]
    pub replica_set: ::prost::alloc::string::String,
    /// All known replica sets in the cluster.
    #[prost(message, repeated, tag="3")]
    pub all_replica_sets: ::prost::alloc::vec::Vec<ReplicaSet>,
}
///////////////////////////////////////////////////////////////////////////////
// Schema /////////////////////////////////////////////////////////////////////

/// A request to update the schema of the Hadron cluster.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SchemaUpdateRequest {
    #[prost(oneof="schema_update_request::Type", tags="1, 2")]
    pub r#type: ::core::option::Option<schema_update_request::Type>,
}
/// Nested message and enum types in `SchemaUpdateRequest`.
pub mod schema_update_request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Type {
        /// A managed schema update request.
        #[prost(message, tag="1")]
        Managed(super::SchemaUpdateManaged),
        /// A one-off schema update request.
        #[prost(message, tag="2")]
        Oneoff(super::SchemaUpdateOneOff),
    }
}
/// A response from an earlier `SchemaUpdateRequest`.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SchemaUpdateResponse {
    /// A bool indicating if the request was a no-op, which would only apply to
    /// managed schema updates.
    #[prost(bool, tag="1")]
    pub was_noop: bool,
}
/// A managed schema update request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SchemaUpdateManaged {
    /// A set of Hadron schema documents to apply to the system.
    #[prost(string, tag="1")]
    pub schema: ::prost::alloc::string::String,
    /// The branch name of this set of schema updates.
    #[prost(string, tag="2")]
    pub branch: ::prost::alloc::string::String,
    /// The timestamp of this set of schema updates.
    ///
    /// This should be an epoch timestamp with millisecond precision.
    #[prost(int64, tag="3")]
    pub timestamp: i64,
}
/// A one-off schema update request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SchemaUpdateOneOff {
    /// A set of Hadron schema documents to apply to the system.
    #[prost(string, tag="1")]
    pub schema: ::prost::alloc::string::String,
}
//////////////////////////////////////////////////////////////////////////////
// Stream Pub ////////////////////////////////////////////////////////////////

/// A request to setup a stream publisher channel.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPubSetupRequest {
    /// The name of the publisher.
    #[prost(string, tag="1")]
    pub name: ::prost::alloc::string::String,
}
/// A response to a stream publisher setup request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPubSetupResponse {
    #[prost(oneof="stream_pub_setup_response::Result", tags="1, 2")]
    pub result: ::core::option::Option<stream_pub_setup_response::Result>,
}
/// Nested message and enum types in `StreamPubSetupResponse`.
pub mod stream_pub_setup_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Result {
        #[prost(message, tag="1")]
        Ok(super::Empty),
        #[prost(message, tag="2")]
        Err(super::Error),
    }
}
/// A request to publish data to a stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPubRequest {
    /// The batch of entries to publish.
    #[prost(bytes="vec", repeated, tag="1")]
    pub batch: ::prost::alloc::vec::Vec<::prost::alloc::vec::Vec<u8>>,
}
/// A response from publishing data to a stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPubResponse {
    #[prost(oneof="stream_pub_response::Result", tags="1, 2")]
    pub result: ::core::option::Option<stream_pub_response::Result>,
}
/// Nested message and enum types in `StreamPubResponse`.
pub mod stream_pub_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Result {
        #[prost(message, tag="1")]
        Ok(super::StreamPubResponseOk),
        #[prost(message, tag="2")]
        Err(super::Error),
    }
}
/// An ok response from publishing data to a stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPubResponseOk {
    /// The offset of the last entry to be written to the stream.
    #[prost(uint64, tag="1")]
    pub last_offset: u64,
}
//////////////////////////////////////////////////////////////////////////////
// Stream Sub ////////////////////////////////////////////////////////////////

/// A request to setup a stream subscriber channel.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSubSetupRequest {
    /// The name of the subscriber.
    #[prost(string, tag="1")]
    pub group_name: ::prost::alloc::string::String,
    /// A bool indicating if this subscription should be considered durable; if `false`, then its
    /// offsets will be held in memory only.
    #[prost(bool, tag="2")]
    pub durable: bool,
    /// The maximum batch size for this subscriber.
    #[prost(uint32, tag="3")]
    pub max_batch_size: u32,
    /// The starting point from which to begin the subscription, if the subscription has no
    /// previously recorded offsets.
    #[prost(oneof="stream_sub_setup_request::StartingPoint", tags="10, 11, 12")]
    pub starting_point: ::core::option::Option<stream_sub_setup_request::StartingPoint>,
}
/// Nested message and enum types in `StreamSubSetupRequest`.
pub mod stream_sub_setup_request {
    /// The starting point from which to begin the subscription, if the subscription has no
    /// previously recorded offsets.
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum StartingPoint {
        #[prost(message, tag="10")]
        Beginning(super::Empty),
        #[prost(message, tag="11")]
        Latest(super::Empty),
        #[prost(uint64, tag="12")]
        Offset(u64),
    }
}
/// A response to a stream subscriber setup request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSubSetupResponse {
    #[prost(oneof="stream_sub_setup_response::Result", tags="1, 2")]
    pub result: ::core::option::Option<stream_sub_setup_response::Result>,
}
/// Nested message and enum types in `StreamSubSetupResponse`.
pub mod stream_sub_setup_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Result {
        #[prost(message, tag="1")]
        Ok(super::Empty),
        #[prost(message, tag="2")]
        Err(super::Error),
    }
}
/// A payload of stream entries delivered to a subscriber by the server.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSubDelivery {
    /// A batch of records for subscriber processing.
    #[prost(message, repeated, tag="1")]
    pub batch: ::prost::alloc::vec::Vec<Record>,
    /// The last offset included in this batch.
    #[prost(uint64, tag="2")]
    pub last_included_offset: u64,
}
/// A subscriber response to a subscription delivery, either `ack`ing or `nack`ing the delivery.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSubDeliveryResponse {
    #[prost(oneof="stream_sub_delivery_response::Result", tags="1, 2")]
    pub result: ::core::option::Option<stream_sub_delivery_response::Result>,
}
/// Nested message and enum types in `StreamSubDeliveryResponse`.
pub mod stream_sub_delivery_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Result {
        /// All records delivered on the last payload have been processed.
        #[prost(message, tag="1")]
        Ack(super::Empty),
        /// An error has taken place during subscriber processing, and the delivered batch was not
        /// successfully processed.
        ///
        /// The given error message will be recorded by the server for observability.
        #[prost(message, tag="2")]
        Nack(super::Error),
    }
}
