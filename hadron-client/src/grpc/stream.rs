///////////////////////////////////////////////////////////////////////////////
// Components /////////////////////////////////////////////////////////////////

/// An empty message.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Empty {}
/// An event record formatted according to the CloudEvents specification.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Event {
    /// The application defined ID of this event.
    ///
    /// See [`id`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#id).
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// The application defined source of this event.
    ///
    /// See [`source`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#source-1).
    #[prost(string, tag = "2")]
    pub source: ::prost::alloc::string::String,
    /// The CloudEvents specification version which the event uses.
    ///
    /// See [`specversion`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#specversion).
    #[prost(string, tag = "3")]
    pub specversion: ::prost::alloc::string::String,
    /// The type identifier of this event.
    ///
    /// See [`type`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#type).
    #[prost(string, tag = "4")]
    pub r#type: ::prost::alloc::string::String,
    /// Any additional optional attributes or extension attributes of this event.
    ///
    /// See [`optional attributes`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#optional-attributes)
    /// and [`extension context attributes`](https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#extension-context-attributes).
    #[prost(map = "string, string", tag = "5")]
    pub optattrs: ::std::collections::HashMap<::prost::alloc::string::String, ::prost::alloc::string::String>,
    /// The data payload of this event.
    #[prost(bytes = "vec", tag = "6")]
    pub data: ::prost::alloc::vec::Vec<u8>,
}
//////////////////////////////////////////////////////////////////////////////
// Stream Publish ////////////////////////////////////////////////////////////

/// A request to publish data to a stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPublishRequest {
    /// The batch of entries to publish.
    #[prost(message, repeated, tag = "1")]
    pub batch: ::prost::alloc::vec::Vec<Event>,
    /// Fsync after writing batch.
    #[prost(bool, tag = "2")]
    pub fsync: bool,
    /// The replication acknowledgement mode for the batch.
    #[prost(enumeration = "WriteAck", tag = "3")]
    pub ack: i32,
}
/// A response from publishing data to a stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPublishResponse {
    /// The offset of the last entry to be written to the stream.
    #[prost(uint64, tag = "1")]
    pub offset: u64,
}
//////////////////////////////////////////////////////////////////////////////
// Stream Subscribe //////////////////////////////////////////////////////////

/// A request to subscribe to data on a stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSubscribeRequest {
    #[prost(oneof = "stream_subscribe_request::Action", tags = "1, 2, 3")]
    pub action: ::core::option::Option<stream_subscribe_request::Action>,
}
/// Nested message and enum types in `StreamSubscribeRequest`.
pub mod stream_subscribe_request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Action {
        /// Setup a stream subscription.
        #[prost(message, tag = "1")]
        Setup(super::StreamSubscribeSetup),
        /// All events delivered on the last payload have been processed.
        #[prost(message, tag = "2")]
        Ack(super::Empty),
        /// An error has taken place during subscriber processing, and the delivered batch was not
        /// successfully processed.
        ///
        /// The given error message will be recorded by the server for observability.
        #[prost(string, tag = "3")]
        Nack(::prost::alloc::string::String),
    }
}
/// A request to setup a stream subscriber channel.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSubscribeSetup {
    /// The name of the subscriber.
    #[prost(string, tag = "1")]
    pub group_name: ::prost::alloc::string::String,
    /// A bool indicating if this subscription should be considered durable; if `false`, then its
    /// offsets will be held in memory only.
    #[prost(bool, tag = "2")]
    pub durable: bool,
    /// The maximum batch size for this subscriber.
    #[prost(uint32, tag = "3")]
    pub max_batch_size: u32,
    /// The starting point from which to begin the subscription, if the subscription has no
    /// previously recorded offsets.
    #[prost(oneof = "stream_subscribe_setup::StartingPoint", tags = "10, 11, 12")]
    pub starting_point: ::core::option::Option<stream_subscribe_setup::StartingPoint>,
}
/// Nested message and enum types in `StreamSubscribeSetup`.
pub mod stream_subscribe_setup {
    /// The starting point from which to begin the subscription, if the subscription has no
    /// previously recorded offsets.
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum StartingPoint {
        #[prost(message, tag = "10")]
        Beginning(super::Empty),
        #[prost(message, tag = "11")]
        Latest(super::Empty),
        #[prost(uint64, tag = "12")]
        Offset(u64),
    }
}
/// A delivery of data for a stream subscription.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamSubscribeResponse {
    /// A batch of records for subscriber processing.
    #[prost(message, repeated, tag = "1")]
    pub batch: ::prost::alloc::vec::Vec<Event>,
    /// The last offset included in this batch.
    #[prost(uint64, tag = "2")]
    pub last_included_offset: u64,
}
//////////////////////////////////////////////////////////////////////////////
// Pipeline Sub //////////////////////////////////////////////////////////////

/// A request to setup a pipeline stage subscriber channel.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineSubscribeRequest {
    /// The name of the pipeline to process.
    #[prost(string, tag = "1")]
    pub pipeline: ::prost::alloc::string::String,
    #[prost(oneof = "pipeline_subscribe_request::Action", tags = "10, 11, 12")]
    pub action: ::core::option::Option<pipeline_subscribe_request::Action>,
}
/// Nested message and enum types in `PipelineSubscribeRequest`.
pub mod pipeline_subscribe_request {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Action {
        /// Setup the subscriber channel to process the given stage name.
        #[prost(string, tag = "10")]
        StageName(::prost::alloc::string::String),
        /// An acknowledgement of successful processing of this stage and its inputs, along with the
        /// required stage output of the stage.
        #[prost(message, tag = "11")]
        Ack(super::PipelineStageOutput),
        /// An error has taken place during subscriber processing, and the delivered data was not
        /// successfully processed.
        ///
        /// The given error message will be recorded by the server for observability.
        #[prost(string, tag = "12")]
        Nack(::prost::alloc::string::String),
    }
}
/// A payload of pipeline stage inputs for a particular pipeline stage.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineSubscribeResponse {
    /// The name of the pipeline stage to which this delivery corresponds.
    #[prost(string, tag = "1")]
    pub stage: ::prost::alloc::string::String,
    /// The root event which triggered this pipeline instance.
    #[prost(message, optional, tag = "2")]
    pub root_event: ::core::option::Option<Event>,
    /// A mapping of pipeline stage inputs based on the definition of this pipeline stage.
    ///
    /// Every key will be the name of the corresponding pipeline stage output which has been declared
    /// as an input dependency for this stage.
    #[prost(map = "string, message", tag = "3")]
    pub inputs: ::std::collections::HashMap<::prost::alloc::string::String, Event>,
}
/// The output of a successful pipeline stage consumption.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineStageOutput {
    /// The base output of the corresponding pipeline stage.
    #[prost(message, optional, tag = "1")]
    pub output: ::core::option::Option<Event>,
}
///////////////////////////////////////////////////////////////////////////////
// Metadata ///////////////////////////////////////////////////////////////////

/// A request to open a metadata stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MetadataRequest {}
/// A response to a metadata subscription setup request.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MetadataResponse {
    /// All partitions of the stream.
    #[prost(message, repeated, tag = "1")]
    pub partitions: ::prost::alloc::vec::Vec<StreamPartition>,
}
/// Stream partition metadata.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct StreamPartition {
    /// The partition number of this partition.
    #[prost(uint32, tag = "1")]
    pub partition: u32,
    /// The Kubernetes internal address of this partition.
    #[prost(string, tag = "2")]
    pub internal: ::prost::alloc::string::String,
    /// The Kubernetes external address of this partition, or empty string if not externally accessible.
    #[prost(string, tag = "3")]
    pub external: ::prost::alloc::string::String,
}
/// The replication acknowledgement mode to use for a write batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum WriteAck {
    /// Wait until all in-sync replicas have acknowledged the write.
    All = 0,
    /// Wait until a majority of in-sync replicas have acknowledged the write.
    Majority = 1,
    /// Do not wait for replica write acknowledgement.
    None = 2,
}
#[doc = r" Generated client implementations."]
pub mod stream_controller_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    #[doc = " The Hadron stream controller interface."]
    #[derive(Debug, Clone)]
    pub struct StreamControllerClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl StreamControllerClient<tonic::transport::Channel> {
        #[doc = r" Attempt to create a new client by connecting to a given endpoint."]
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: std::convert::TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> StreamControllerClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::ResponseBody: Body + Send + Sync + 'static,
        T::Error: Into<StdError>,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_interceptor<F>(inner: T, interceptor: F) -> StreamControllerClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T: tonic::codegen::Service<http::Request<tonic::body::BoxBody>, Response = http::Response<<T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody>>,
            <T as tonic::codegen::Service<http::Request<tonic::body::BoxBody>>>::Error: Into<StdError> + Send + Sync,
        {
            StreamControllerClient::new(InterceptedService::new(inner, interceptor))
        }
        #[doc = r" Compress requests with `gzip`."]
        #[doc = r""]
        #[doc = r" This requires the server to support it otherwise it might respond with an"]
        #[doc = r" error."]
        pub fn send_gzip(mut self) -> Self {
            self.inner = self.inner.send_gzip();
            self
        }
        #[doc = r" Enable decompressing responses with `gzip`."]
        pub fn accept_gzip(mut self) -> Self {
            self.inner = self.inner.accept_gzip();
            self
        }
        #[doc = " Open a metadata stream."]
        pub async fn metadata(&mut self, request: impl tonic::IntoRequest<super::MetadataRequest>) -> Result<tonic::Response<tonic::codec::Streaming<super::MetadataResponse>>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/stream.StreamController/Metadata");
            self.inner.server_streaming(request.into_request(), path, codec).await
        }
        #[doc = " Open a stream publisher channel."]
        pub async fn stream_publish(&mut self, request: impl tonic::IntoRequest<super::StreamPublishRequest>) -> Result<tonic::Response<super::StreamPublishResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/stream.StreamController/StreamPublish");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " Open a stream subscriber channel."]
        pub async fn stream_subscribe(
            &mut self, request: impl tonic::IntoStreamingRequest<Message = super::StreamSubscribeRequest>,
        ) -> Result<tonic::Response<tonic::codec::Streaming<super::StreamSubscribeResponse>>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/stream.StreamController/StreamSubscribe");
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        #[doc = " Open a pipeline subscriber channel."]
        pub async fn pipeline_subscribe(
            &mut self, request: impl tonic::IntoStreamingRequest<Message = super::PipelineSubscribeRequest>,
        ) -> Result<tonic::Response<tonic::codec::Streaming<super::PipelineSubscribeResponse>>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/stream.StreamController/PipelineSubscribe");
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
    }
}
