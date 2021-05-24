/// Common metadata found as part of most schema statements.
#[derive(::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Metadata {
    /// The name associated with this object.
    #[prost(string, required, tag="1")]
    pub name: ::prost::alloc::string::String,
    /// The namespace to which this object applies.
    #[prost(string, required, tag="2")]
    pub namespace: ::prost::alloc::string::String,
    /// A description of this object.
    #[prost(string, required, tag="3")]
    #[serde(default)]
    pub description: ::prost::alloc::string::String,
}
/// A namespace for grouping resources.
#[derive(::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Namespace {
    /// The unique ID of this object.
    ///
    /// When this object is declared via YAML as part of the schema management system, the ID field
    /// is ignored and defaults to `0`, as IDs are only used internally and are not exposed to the
    /// end users.
    #[prost(uint64, required, tag="1")]
    #[serde(default, skip)]
    pub id: u64,
    /// The unique identifier of this namespace.
    #[prost(string, required, tag="2")]
    pub name: ::prost::alloc::string::String,
    /// A description of this namespace.
    #[prost(string, required, tag="3")]
    #[serde(default)]
    pub description: ::prost::alloc::string::String,
}
/// A durable log of events with a configurable number of partitions.
///
/// Partitions in Hadron are used to increasing write throughput by horizontally scaling the
/// workload of a stream across multiple Hadron nodes.
///
/// In Hadron, the partitions of a stream are explicitly assigned to different replica sets of
/// the cluster as part of the metadata system. A stream may only have one partition on a
/// replica set, though the stream may _indefinitely_ scale across any number of different
/// replica sets.
///
/// The replication factor of a stream's data is governed by the replication factor of each
/// replica set running a partition of the stream, and may differ per replica set.
#[derive(::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Stream {
    /// The unique ID of this object.
    ///
    /// When this object is declared via YAML as part of the schema management system, the ID field
    /// is ignored and defaults to `0`, as IDs are only used internally and are not exposed to the
    /// end users.
    #[prost(uint64, required, tag="1")]
    #[serde(default, skip)]
    pub id: u64,
    /// Object metadata.
    #[prost(message, required, tag="2")]
    #[serde(flatten)]
    pub metadata: Metadata,
    /// The partition assignments of this stream.
    ///
    /// Adding new partitions to a stream is allowed, however removing partitions of a stream is
    /// not allowed as it would result in the loss of the data of the removed partition.
    #[prost(string, repeated, tag="3")]
    pub partitions: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// An optional TTL duration specifying how long records are to be kept on the stream.
    ///
    /// If not specified, then records will stay on the stream forever.
    #[prost(string, optional, tag="4")]
    pub ttl: ::core::option::Option<::prost::alloc::string::String>,
}
/// A multi-stage data workflow.
#[derive(::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Pipeline {
    /// The unique ID of this object.
    ///
    /// When this object is declared via YAML as part of the schema management system, the ID field
    /// is ignored and defaults to `0`, as IDs are only used internally and are not exposed to the
    /// end users.
    #[prost(uint64, required, tag="1")]
    #[serde(default, skip)]
    pub id: u64,
    /// Object metadata.
    #[prost(message, required, tag="2")]
    #[serde(flatten)]
    pub metadata: Metadata,
    /// The name of the stream which will trigger this pipeline.
    #[prost(string, required, tag="3")]
    pub input_stream: ::prost::alloc::string::String,
    /// Event type matchers which will trigger this pipeline.
    #[prost(string, repeated, tag="4")]
    #[serde(default)]
    pub triggers: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// The stages of this pipeline.
    #[prost(message, repeated, tag="5")]
    pub stages: ::prost::alloc::vec::Vec<PipelineStage>,
    /// The maximum number of pipeline instances which may be executed in parallel per partition.
    #[prost(uint32, required, tag="6")]
    #[serde(default = "super::pipeline_max_parallel_default")]
    pub max_parallel: u32,
}
/// A single pipeline stage.
#[derive(::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineStage {
    /// The name of this pipeline stage, which is unique per pipeline.
    #[prost(string, required, tag="1")]
    pub name: ::prost::alloc::string::String,
    /// All stages which must complete before this stage may be started.
    #[prost(string, repeated, tag="2")]
    #[serde(default)]
    pub after: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// All inputs which this stage depends upon in order to be started.
    #[prost(string, repeated, tag="3")]
    #[serde(default)]
    pub dependencies: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
}
/// A RPC endpoint definition.
#[derive(::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Endpoint {
    /// The unique ID of this object.
    ///
    /// When this object is declared via YAML as part of the schema management system, the ID field
    /// is ignored and defaults to `0`, as IDs are only used internally and are not exposed to the
    /// end users.
    #[prost(uint64, required, tag="1")]
    #[serde(default, skip)]
    pub id: u64,
    /// Object metadata.
    #[prost(message, required, tag="2")]
    #[serde(flatten)]
    pub metadata: Metadata,
    /// The replica set on which this endpoint is to run.
    #[prost(string, required, tag="3")]
    pub replica_set: ::prost::alloc::string::String,
    /// The input RPC mode.
    #[prost(enumeration="EndpointMessageFlow", required, tag="4")]
    #[serde(default)]
    pub input: i32,
    /// The output RPC mode.
    #[prost(enumeration="EndpointMessageFlow", required, tag="5")]
    #[serde(default)]
    pub output: i32,
}
/// A schema branch model.
#[derive(::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SchemaBranch {
    /// The name of the branch.
    #[prost(string, required, tag="1")]
    pub name: ::prost::alloc::string::String,
    /// The most recently applied timestamp for this branch.
    #[prost(int64, required, tag="2")]
    pub timestamp: i64,
}
/// A RPC endpoint message mode.
#[derive(::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum EndpointMessageFlow {
    Oneshot = 0,
    Streaming = 1,
}
