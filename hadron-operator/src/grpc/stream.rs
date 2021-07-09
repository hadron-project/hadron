///////////////////////////////////////////////////////////////////////////////
// Components /////////////////////////////////////////////////////////////////

/// An empty message.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Empty {
}
/// An event record formatted according to the CloudEvents specification.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Event {
    /// The ID of this event, which is always the offset of this event on its stream partition.
    ///
    /// See [`id`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#id).
    #[prost(uint64, tag="1")]
    pub id: u64,
    /// The source of this event, formatted as `/{cluster}/{stream}/{partition}/`.
    ///
    /// See [`source`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#source-1).
    #[prost(string, tag="2")]
    pub source: ::prost::alloc::string::String,
    /// The CloudEvents specification version which the event uses.
    ///
    /// See [`specversion`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#specversion).
    #[prost(string, tag="3")]
    pub specversion: ::prost::alloc::string::String,
    /// The type identifier of this event.
    ///
    /// See [`type`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#type).
    #[prost(string, tag="4")]
    pub r#type: ::prost::alloc::string::String,
    /// The key of this event.
    #[prost(string, tag="5")]
    pub key: ::prost::alloc::string::String,
    /// Any additional optional attributes or extension attributes of this event.
    ///
    /// See [`optional attributes`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#optional-attributes)
    /// and [`extension context attributes`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#extension-context-attributes).
    #[prost(map="string, string", tag="6")]
    pub optattrs: ::std::collections::HashMap<::prost::alloc::string::String, ::prost::alloc::string::String>,
    /// The data payload of this event.
    #[prost(bytes="vec", tag="7")]
    pub data: ::prost::alloc::vec::Vec<u8>,
}
/// A new event record to be published to a stream partition.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NewEvent {
    /// The type identifier of this event.
    ///
    /// See [`type`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#type).
    #[prost(string, tag="1")]
    pub r#type: ::prost::alloc::string::String,
    /// The key of this event.
    #[prost(string, tag="2")]
    pub key: ::prost::alloc::string::String,
    /// Any additional optional attributes or extension attributes of this event.
    ///
    /// See [`optional attributes`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#optional-attributes)
    /// and [`extension context attributes`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#extension-context-attributes).
    #[prost(map="string, string", tag="3")]
    pub optattrs: ::std::collections::HashMap<::prost::alloc::string::String, ::prost::alloc::string::String>,
    /// The data payload of this event.
    #[prost(bytes="vec", tag="4")]
    pub data: ::prost::alloc::vec::Vec<u8>,
}
//////////////////////////////////////////////////////////////////////////////
// Stream Publish ////////////////////////////////////////////////////////////

/// A request to publish data to a stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPublishRequest {
    /// The batch of entries to publish.
    #[prost(message, repeated, tag="1")]
    pub batch: ::prost::alloc::vec::Vec<NewEvent>,
}
/// A response from publishing data to a stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPublishResponse {
    /// The offset of the last entry to be written to the stream.
    #[prost(uint64, tag="1")]
    pub last_offset: u64,
}
//////////////////////////////////////////////////////////////////////////////
// Stream Subscribe //////////////////////////////////////////////////////////

/// A request to subscribe to data on a stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSubscribeRequest {
    #[prost(oneof="stream_subscribe_request::Action", tags="1, 2, 3")]
    pub action: ::core::option::Option<stream_subscribe_request::Action>,
}
/// Nested message and enum types in `StreamSubscribeRequest`.
pub mod stream_subscribe_request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Action {
        /// Setup a stream subscription.
        #[prost(message, tag="1")]
        Setup(super::StreamSubscribeSetup),
        /// All events delivered on the last payload have been processed.
        #[prost(message, tag="2")]
        Ack(super::Empty),
        /// An error has taken place during subscriber processing, and the delivered batch was not
        /// successfully processed.
        ///
        /// The given error message will be recorded by the server for observability.
        #[prost(string, tag="3")]
        Nack(::prost::alloc::string::String),
    }
}
/// A request to setup a stream subscriber channel.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSubscribeSetup {
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
    #[prost(oneof="stream_subscribe_setup::StartingPoint", tags="10, 11, 12")]
    pub starting_point: ::core::option::Option<stream_subscribe_setup::StartingPoint>,
}
/// Nested message and enum types in `StreamSubscribeSetup`.
pub mod stream_subscribe_setup {
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
/// A delivery of data for a stream subscription.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSubscribeResponse {
    /// A batch of records for subscriber processing.
    #[prost(message, repeated, tag="1")]
    pub batch: ::prost::alloc::vec::Vec<Event>,
    /// The last offset included in this batch.
    #[prost(uint64, tag="2")]
    pub last_included_offset: u64,
}
//////////////////////////////////////////////////////////////////////////////
// Pipeline Sub //////////////////////////////////////////////////////////////

/// A request to setup a pipeline stage subscriber channel.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineSubscribeRequest {
    #[prost(oneof="pipeline_subscribe_request::Action", tags="1, 2, 3")]
    pub action: ::core::option::Option<pipeline_subscribe_request::Action>,
}
/// Nested message and enum types in `PipelineSubscribeRequest`.
pub mod pipeline_subscribe_request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Action {
        /// Setup the subscriber channel to process the given stage name.
        #[prost(string, tag="1")]
        StageName(::prost::alloc::string::String),
        /// An acknowledgement of successful processing of this stage and its inputs, along with the
        /// required stage output of the stage.
        #[prost(message, tag="2")]
        Ack(super::PipelineStageOutput),
        /// An error has taken place during subscriber processing, and the delivered data was not
        /// successfully processed.
        ///
        /// The given error message will be recorded by the server for observability.
        #[prost(string, tag="3")]
        Nack(::prost::alloc::string::String),
    }
}
/// A payload of pipeline stage inputs for a particular pipeline stage.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineSubscribeResponse {
    /// The name of the pipeline stage to which this delivery corresponds.
    #[prost(string, tag="1")]
    pub stage: ::prost::alloc::string::String,
    /// The source stream offset corresponding to this pipeline instance.
    #[prost(uint64, tag="2")]
    pub offset: u64,
    /// A mapping of pipeline stage inputs based on the definition of this pipeline stage.
    ///
    /// Every key will be the name of the corresponding pipeline stage output which has been declared
    /// as an input dependency for this stage, or the `root_event` if declared as a dependency for
    /// this stage.
    #[prost(map="string, bytes", tag="3")]
    pub inputs: ::std::collections::HashMap<::prost::alloc::string::String, ::prost::alloc::vec::Vec<u8>>,
}
/// The output of a successful pipeline stage consumption.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineStageOutput {
    /// The base output of the corresponding pipeline stage.
    #[prost(bytes="vec", tag="1")]
    pub output: ::prost::alloc::vec::Vec<u8>,
}
///////////////////////////////////////////////////////////////////////////////
// Metadata ///////////////////////////////////////////////////////////////////

/// A request to open a metadata stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MetadataRequest {
}
/// A response to a metadata subscription setup request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MetadataResponse {
    /// All partitions of the stream.
    #[prost(message, repeated, tag="1")]
    pub partitions: ::prost::alloc::vec::Vec<StreamPartition>,
}
/// Stream partition metadata.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPartition {
    /// The Kubernetes internal address of this partition.
    #[prost(string, tag="1")]
    pub internal: ::prost::alloc::string::String,
    /// The Kubernetes external address of this partition, or empty string if not externally accessible.
    #[prost(string, tag="2")]
    pub external: ::prost::alloc::string::String,
}
