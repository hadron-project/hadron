//! Data Definition Language (DDL) models.

#![allow(dead_code)] // TODO: remove this.

mod validation;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// All Data Definition Language statements availabe in Hadron.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind")]
pub enum DDLStatement {
    /// A changeset statement.
    ChangeSet(Object<ChangeSet>),
    /// A namespace definition.
    Namespace(Namespace),
    /// A stream definition.
    Stream(Object<Stream>),
    /// A pipeline definition.
    Pipeline(Object<Pipeline>),
}

/// A DDL schema update to be applied to the system.
#[derive(Debug, Eq, PartialEq)]
pub struct DDLSchemaUpdate {
    /// The changeset declaration.
    pub changeset: ChangeSet,
    /// The set of DDL statements composing this changeset.
    pub statements: Vec<DDL>,
}

/// All Data Definition Language variants availabe in Hadron.
#[derive(Debug, Eq, PartialEq)]
pub enum DDL {
    Namespace(Namespace),
    Stream(Object<Stream>),
    Pipeline(Object<Pipeline>),
}

/// Common metadata found as part of most DDL objects.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Metadata {
    /// The name associated with the object.
    pub name: String,
    /// The namespace to which the object applies.
    pub namespace: String,
    /// A description of the object.
    #[serde(default)]
    pub description: String,
}

/// A wrapper type for objects with metadata and a spec.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Object<T: Serialize + DeserializeOwned + Eq + PartialEq> {
    pub metadata: Metadata,
    #[serde(bound = "T: Serialize + DeserializeOwned + Eq + PartialEq")]
    pub spec: T,
}

/// A DDL changeset.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind")]
pub enum ChangeSet {
    OneOff,
    Branch {
        /// The name of the DDL branch to which this changeset belongs.
        name: String,
    },
}

/// A namespace for grouping resources.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Namespace {
    /// The unique identifier of this namespace.
    pub name: String,
    /// A description of this namespace.
    #[serde(default)]
    pub description: String,
}

/// A durable log of events.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "streamType")]
pub enum Stream {
    /// A standard stream with a configurable number of partitions.
    Standard {
        /// The number of partitions to create for this stream.
        ///
        /// Partitions in Hadron are used solely for increasing write throughput. They have no
        /// impact on stream consumers. That is, even if a stream has a single partition, a
        /// single consumer group with multiple consumers is still able to process the stream in
        /// parallel. Consult the documentation on stream consumers for more details.
        partitions: u8,
        /// The replication factor of each partition of this stream.
        #[serde(rename = "replicationFactor")]
        replication_factor: u8,
        /// An optional TTL duration specifying how long records are to be kept on the stream.
        ///
        /// If not specified, then records will stay on the stream forever.
        ttl: Option<String>,
    },
    /// A single-partition stream optimized for use with a database "out table" CDC pattern.
    ///
    /// Every message written to an OutTable stream must have a unique, orderd ID. This is commonly
    /// seen in RDBMS systems as the monotonically increasing ID sequence for table primary keys.
    ///
    /// This stream type guarantees exactly once production of messages to this stream. If a
    /// duplicate message — a message bearing the same ID — is published to this stream, it will
    /// no-op. This provides a strict server-side guarantee that there will be no duplicates on
    /// OutTable streams.
    OutTable {
        /// The replication factor for this stream's single partition.
        #[serde(rename = "replicationFactor")]
        replication_factor: u8,
        /// An optional TTL duration specifying how long records are to be kept on the stream.
        ///
        /// If not specified, then records will stay on the stream forever.
        ttl: Option<String>,
    },
}

/// A multi-stage data workflow.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Pipeline {
    /// The name of the stream which can trigger this pipeline.
    #[serde(rename = "triggerStream")]
    pub trigger_stream: String,
    /// The stages of this pipeline.
    pub stages: Vec<PipelineStage>,
}

/// A single pipeline stage.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
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
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct PipelineStageOutput {
    /// The name of this pipeline stage output, which is unique per pipeline stage.
    pub name: String,
    /// The name of the stream to which this output event is to be published.
    pub stream: String,
    /// The namespace of the output stream.
    pub namespace: String,
}
