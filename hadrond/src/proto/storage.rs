#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SnapshotMetadata {
    /// The snapshot entry's term.
    #[prost(uint64, tag = "1")]
    pub term: u64,
    /// The snapshot entry's index.
    #[prost(uint64, tag = "2")]
    pub index: u64,
    /// The latest membership configuration covered by the snapshot.
    #[prost(message, optional, tag = "3")]
    pub membership: ::std::option::Option<MembershipConfig>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MembershipConfig {
    /// All members of the Raft cluster.
    #[prost(uint64, repeated, tag = "1")]
    pub members: ::std::vec::Vec<u64>,
    /// All members of the Raft cluster after joint consensus is finalized.
    ///
    /// The presence of a value here indicates that the config is in joint consensus.
    #[prost(uint64, repeated, tag = "2")]
    pub members_after_consensus: ::std::vec::Vec<u64>,
}
/// A namespace for grouping resources.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Namespace {
    /// The unique identifier of this namespace.
    #[prost(string, tag = "1")]
    pub name: std::string::String,
    /// A description of this namespace.
    #[prost(string, tag = "2")]
    pub description: std::string::String,
}
/// A durable log of events.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Stream {
    /// The name of this stream.
    #[prost(string, tag = "1")]
    pub name: std::string::String,
    /// The namespace of this stream.
    #[prost(string, tag = "2")]
    pub namespace: std::string::String,
    /// A description of this stream.
    #[prost(string, tag = "3")]
    pub description: std::string::String,
}
/// A multi-stage data workflow.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Pipeline {
    /// The name of this pipeline.
    #[prost(string, tag = "1")]
    pub name: std::string::String,
    /// The namespace of this pipeline.
    #[prost(string, tag = "2")]
    pub namespace: std::string::String,
    /// A description of this pipeline.
    #[prost(string, tag = "3")]
    pub description: std::string::String,
    /// The name of the stream which can trigger this pipeline.
    #[prost(string, tag = "4")]
    pub trigger_stream: std::string::String,
    /// The stages of this pipeline.
    #[prost(message, repeated, tag = "5")]
    pub stages: ::std::vec::Vec<PipelineStage>,
}
/// A single pipeline stage.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineStage {
    /// The name of this pipeline stage, which is unique per pipeline.
    #[prost(string, tag = "1")]
    pub name: std::string::String,
    /// All stages which must complete before this stage may be started.
    #[prost(string, repeated, tag = "2")]
    pub after: ::std::vec::Vec<std::string::String>,
    /// All inputs which this stage depends upon in order to be started.
    #[prost(string, repeated, tag = "3")]
    pub dependencies: ::std::vec::Vec<std::string::String>,
    /// All outputs which this stage must produce in order to complete successfully.
    #[prost(message, repeated, tag = "4")]
    pub outputs: ::std::vec::Vec<PipelineStageOutput>,
}
/// A pipeline stage output definition.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineStageOutput {
    /// The name of this pipeline stage output, which is unique per pipeline stage.
    #[prost(string, tag = "1")]
    pub name: std::string::String,
    /// The name of the stream to which this output event is to be published.
    #[prost(string, tag = "2")]
    pub stream: std::string::String,
    /// The namespace of the output stream.
    #[prost(string, tag = "3")]
    pub namespace: std::string::String,
}
