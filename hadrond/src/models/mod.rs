//! Data schema models.

#![allow(dead_code)] // TODO: remove this.

mod impls;
mod traits;

use serde::{Deserialize, Serialize};

pub use traits::Namespaced;

/// A schema update to be applied to the system.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "updateType")]
pub enum SchemaUpdate {
    OneOff(SchemaUpdateOneOff),
    Managed(SchemaUpdateManaged),
}

/// A one-off schema update to be applied to the system.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct SchemaUpdateOneOff {
    /// The set of schema statements of this schema update.
    pub statements: Vec<SchemaStatement>,
}

/// A managed schema update to be applied to the system.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct SchemaUpdateManaged {
    /// The branch name of the schema update.
    pub branch: String,
    /// The timestamp of the schema update.
    pub timestamp: i64,
    /// The set of schema statements of this schema update.
    pub statements: Vec<SchemaStatement>,
}

/// All available schema statements in Hadron.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind")]
pub enum SchemaStatement {
    /// A namespace definition.
    Namespace(Namespace),
    /// A stream definition.
    Stream(Stream),
    /// A pipeline definition.
    Pipeline(Pipeline),
    /// An RPC endpoint definition.
    Endpoint(Endpoint),
}

/// Common metadata found as part of most schema statements.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Metadata {
    /// The name associated with this object.
    pub name: String,
    /// The namespace to which this object applies.
    pub namespace: String,
    /// A description of this object.
    #[serde(default)]
    pub description: String,
}

/// A namespace for grouping resources.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Namespace {
    /// The unique identifier of this namespace.
    pub name: String,
    /// A description of this namespace.
    #[serde(default)]
    pub description: String,
}

/// A durable log of events.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "streamType")]
pub enum Stream {
    Standard(StandardStream),
    OutTable(OutTableStream),
}
/// A standard stream with a configurable number of partitions.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StandardStream {
    /// Object metadata.
    #[serde(flatten)]
    pub metadata: Metadata,
    /// The number of partitions to create for this stream.
    ///
    /// Partitions in Hadron are used solely for increasing write throughput. They have no
    /// impact on stream consumers. That is, even if a stream has a single partition, a
    /// single consumer group with multiple consumers is still able to process the stream in
    /// parallel. Consult the documentation on stream consumers for more details.
    pub partitions: u32,
    /// The replication factor of each partition of this stream.
    pub replication_factor: u8,
    /// An optional TTL duration specifying how long records are to be kept on the stream.
    ///
    /// If not specified, then records will stay on the stream forever.
    pub ttl: Option<String>,
}

/// A single-partition stream optimized for use with a database "out table" CDC pattern.
///
/// Every message written to an OutTable stream must have a unique, orderd ID. This is commonly
/// seen in RDBMS systems as the monotonically increasing ID sequence for table primary keys.
///
/// This stream type guarantees exactly once production of messages to this stream. If a
/// duplicate message — a message bearing the same ID — is published to this stream, it will
/// no-op. This provides a strict server-side guarantee that there will be no duplicates on
/// OutTable streams.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OutTableStream {
    /// Object metadata.
    #[serde(flatten)]
    pub metadata: Metadata,
    /// The replication factor for this stream's single partition.
    pub replication_factor: u8,
    /// An optional TTL duration specifying how long records are to be kept on the stream.
    ///
    /// If not specified, then records will stay on the stream forever.
    pub ttl: Option<String>,
}

/// A multi-stage data workflow.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    /// Object metadata.
    #[serde(flatten)]
    pub metadata: Metadata,
    /// The name of the stream which will trigger this pipeline.
    pub input_stream: String,
    /// Event type matchers which will trigger this pipeline.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// The stages of this pipeline.
    pub stages: Vec<PipelineStage>,
    /// The replication factor for this pipeline's data.
    pub replication_factor: u8,
}

/// A single pipeline stage.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct PipelineStage {
    /// The name of this pipeline stage, which is unique per pipeline.
    pub name: String,
    /// All stages which must complete before this stage may be started.
    pub after: Option<Vec<String>>,
    /// All inputs which this stage depends upon in order to be started.
    pub dependencies: Option<Vec<String>>,
    /// All outputs which this stage must produce in order to complete successfully.
    pub outputs: Option<Vec<PipelineStageOutput>>,
}

/// A pipeline stage output definition.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct PipelineStageOutput {
    /// The name of this pipeline stage output, which is unique per pipeline stage.
    pub name: String,
    /// The name of the stream to which this output event is to be published.
    pub stream: String,
    /// The namespace of the output stream.
    pub namespace: String,
}

/// A RPC endpoint definition.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Endpoint {
    /// Object metadata.
    #[serde(flatten)]
    pub metadata: Metadata,
    /// The input RPC mode.
    #[serde(default)]
    pub input: EndpointMessageFlow,
    /// The output RPC mode.
    #[serde(default)]
    pub output: EndpointMessageFlow,
}

/// A RPC endpoint message mode.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum EndpointMessageFlow {
    Single,
    Stream,
}

impl Default for EndpointMessageFlow {
    fn default() -> Self {
        Self::Single
    }
}

/// A schema branch model.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct SchemaBranch {
    /// The name of the branch.
    pub name: String,
    /// The most recently applied timestamp for this branch.
    pub timestamp: i64,
}
