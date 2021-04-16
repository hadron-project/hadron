///////////////////////////////////////////////////////////////////////////////
// Components /////////////////////////////////////////////////////////////////

/// An error object which is returned from the Hadron server under various conditions.
///
/// Clients can match on specific error variants to drive behavior.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Error {
    /// A summary of the error which has taken place.
    #[prost(string, tag="1")]
    pub message: ::prost::alloc::string::String,
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
