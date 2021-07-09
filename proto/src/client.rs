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
    /// The subject of the event in the context of the event producer.
    ///
    /// This is used by Hadron in much the same way that Kafka uses the event `key`.
    ///
    /// See [`subject`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#subject).
    #[prost(string, tag="5")]
    pub subject: ::prost::alloc::string::String,
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
    /// The source of this event.
    ///
    /// See [`source`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#source-1).
    #[prost(string, tag="2")]
    pub source: ::prost::alloc::string::String,
    /// The subject of the event in the context of the event producer.
    ///
    /// This is used by Hadron in much the same way that Kafka uses the event `key`.
    ///
    /// See [`subject`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#subject).
    #[prost(string, tag="3")]
    pub subject: ::prost::alloc::string::String,
    /// Any additional optional attributes or extension attributes of this event.
    ///
    /// See [`optional attributes`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#optional-attributes)
    /// and [`extension context attributes`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#extension-context-attributes).
    #[prost(map="string, string", tag="4")]
    pub optattrs: ::std::collections::HashMap<::prost::alloc::string::String, ::prost::alloc::string::String>,
    /// The data payload of this event.
    #[prost(bytes="vec", tag="5")]
    pub data: ::prost::alloc::vec::Vec<u8>,
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
    #[prost(message, repeated, tag="1")]
    pub batch: ::prost::alloc::vec::Vec<NewEvent>,
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
    pub batch: ::prost::alloc::vec::Vec<Event>,
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
        /// All events delivered on the last payload have been processed.
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
//////////////////////////////////////////////////////////////////////////////
// Pipeline Sub //////////////////////////////////////////////////////////////

/// A request to setup a pipeline stage subscriber channel.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineSubSetupRequest {
    /// The name of the pipeline stage to consume.
    #[prost(string, tag="1")]
    pub stage_name: ::prost::alloc::string::String,
}
/// A response to a pipeline stage subscriber setup request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineSubSetupResponse {
    #[prost(oneof="pipeline_sub_setup_response::Result", tags="1, 2")]
    pub result: ::core::option::Option<pipeline_sub_setup_response::Result>,
}
/// Nested message and enum types in `PipelineSubSetupResponse`.
pub mod pipeline_sub_setup_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Result {
        #[prost(message, tag="1")]
        Ok(super::Empty),
        #[prost(message, tag="2")]
        Err(super::Error),
    }
}
/// A payload of pipeline stage inputs for a particular pipeline stage.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineSubDelivery {
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
/// A subscriber response to a subscription delivery, either `ack`ing or `nack`ing the delivery.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineSubDeliveryResponse {
    #[prost(oneof="pipeline_sub_delivery_response::Result", tags="1, 2")]
    pub result: ::core::option::Option<pipeline_sub_delivery_response::Result>,
}
/// Nested message and enum types in `PipelineSubDeliveryResponse`.
pub mod pipeline_sub_delivery_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Result {
        /// An acknowledgement of successful processing of this stage and its inputs, along with the
        /// require stage output of this stage.
        #[prost(message, tag="1")]
        Ack(super::PipelineStageOutput),
        /// An error has taken place during subscriber processing, and the delivered batch was not
        /// successfully processed.
        ///
        /// The given error message will be recorded by the server for observability.
        #[prost(message, tag="2")]
        Nack(super::Error),
    }
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

/// A response to a metadata subscription setup request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MetadataSubSetupResponse {
    #[prost(oneof="metadata_sub_setup_response::Result", tags="1, 2")]
    pub result: ::core::option::Option<metadata_sub_setup_response::Result>,
}
/// Nested message and enum types in `MetadataSubSetupResponse`.
pub mod metadata_sub_setup_response {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Result {
        #[prost(message, tag="1")]
        Ok(super::ClusterMetadata),
        #[prost(message, tag="2")]
        Err(super::Error),
    }
}
/// All known metadata of the target cluster.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ClusterMetadata {
    /// The name of the cluster which was queried.
    #[prost(string, tag="1")]
    pub cluster_name: ::prost::alloc::string::String,
    /// All known pods of the Hadron cluster.
    #[prost(string, repeated, tag="2")]
    pub pods: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// A mapping of all known streams, with metadata data on all partitions,
    /// their leaders and their replicas.
    #[prost(map="string, message", tag="3")]
    pub streams: ::std::collections::HashMap<::prost::alloc::string::String, StreamMetadata>,
    /// A mapping of all known pipelines, with metadata data on all partitions,
    /// their leaders and their replicas.
    #[prost(map="string, message", tag="4")]
    pub pipelines: ::std::collections::HashMap<::prost::alloc::string::String, PipelineMetadata>,
}
/// Stream metadata.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamMetadata {
    /// The name of the stream.
    #[prost(string, tag="1")]
    pub name: ::prost::alloc::string::String,
    /// The partitions of this stream.
    #[prost(message, repeated, tag="2")]
    pub partitions: ::prost::alloc::vec::Vec<PartitionMetadata>,
}
/// Pipeline metadata.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineMetadata {
    /// The name of the pipeline.
    #[prost(string, tag="1")]
    pub name: ::prost::alloc::string::String,
    /// The source stream of this pipeline.
    ///
    /// This is used by clients to establish pipeline topology, as pipelines mirror their source
    /// stream's topology.
    #[prost(string, tag="2")]
    pub source_stream: ::prost::alloc::string::String,
}
/// Metadata of a stream partition.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PartitionMetadata {
    /// The offset of this partition (actually an unsigned 8-bit integer).
    #[prost(uint32, tag="1")]
    pub offset: u32,
    /// The pod name of the partition leader.
    #[prost(string, tag="2")]
    pub leader: ::prost::alloc::string::String,
    /// The pod names of all replicas of this partition.
    #[prost(string, repeated, tag="3")]
    pub replicas: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    /// The schedule state of the partition.
    #[prost(string, tag="4")]
    pub schedule_state: ::prost::alloc::string::String,
    /// The runtime state of the partition.
    #[prost(string, tag="5")]
    pub runtime_state: ::prost::alloc::string::String,
}
/// A metadata change.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MetadataChange {
    /// The type of metadata change observed.
    #[prost(oneof="metadata_change::Change", tags="1, 2, 3, 4, 5, 6, 7")]
    pub change: ::core::option::Option<metadata_change::Change>,
}
/// Nested message and enum types in `MetadataChange`.
pub mod metadata_change {
    /// The type of metadata change observed.
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Change {
        /// Metadata streams were reset on the connected pod, so a new full payload has been sent.
        #[prost(message, tag="1")]
        Reset(super::ClusterMetadata),
        /// A pod has been added.
        ///
        /// This corresponds to StatefulSet scaling events. Not temprorary pod state changes.
        #[prost(string, tag="2")]
        PodAdded(::prost::alloc::string::String),
        /// A pod has been removed.
        ///
        /// This corresponds to StatefulSet scaling events. Not temprorary pod state changes.
        #[prost(string, tag="3")]
        PodRemoved(::prost::alloc::string::String),
        /// A stream has been updated or added.
        #[prost(message, tag="4")]
        StreamUpdated(super::StreamMetadata),
        /// A stream has been removed.
        #[prost(string, tag="5")]
        StreamRemoved(::prost::alloc::string::String),
        /// A pipeline has been updated or added.
        #[prost(message, tag="6")]
        PipelineUpdated(super::PipelineMetadata),
        /// A pipeline has been removed.
        #[prost(string, tag="7")]
        PipelineRemoved(::prost::alloc::string::String),
    }
}
