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
    /// See \[`id`\](<https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#id>).
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    /// The application defined source of this event.
    ///
    /// See \[`source`\](<https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#source-1>).
    #[prost(string, tag = "2")]
    pub source: ::prost::alloc::string::String,
    /// The CloudEvents specification version which the event uses.
    ///
    /// See \[`specversion`\](<https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#specversion>).
    #[prost(string, tag = "3")]
    pub specversion: ::prost::alloc::string::String,
    /// The type identifier of this event.
    ///
    /// See \[`type`\](<https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#type>).
    #[prost(string, tag = "4")]
    pub r#type: ::prost::alloc::string::String,
    /// Any additional optional attributes or extension attributes of this event.
    ///
    /// See [`optional attributes`](<https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#optional-attributes>)
    /// and [`extension context attributes`](<https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#extension-context-attributes>).
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
#[doc = r" Generated server implementations."]
pub mod stream_controller_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    #[doc = "Generated trait containing gRPC methods that should be implemented for use with StreamControllerServer."]
    #[async_trait]
    pub trait StreamController: Send + Sync + 'static {
        #[doc = "Server streaming response type for the Metadata method."]
        type MetadataStream: futures_core::Stream<Item = Result<super::MetadataResponse, tonic::Status>> + Send + 'static;
        #[doc = " Open a metadata stream."]
        async fn metadata(&self, request: tonic::Request<super::MetadataRequest>) -> Result<tonic::Response<Self::MetadataStream>, tonic::Status>;
        #[doc = " Open a stream publisher channel."]
        async fn stream_publish(&self, request: tonic::Request<super::StreamPublishRequest>) -> Result<tonic::Response<super::StreamPublishResponse>, tonic::Status>;
        #[doc = "Server streaming response type for the StreamSubscribe method."]
        type StreamSubscribeStream: futures_core::Stream<Item = Result<super::StreamSubscribeResponse, tonic::Status>> + Send + 'static;
        #[doc = " Open a stream subscriber channel."]
        async fn stream_subscribe(&self, request: tonic::Request<tonic::Streaming<super::StreamSubscribeRequest>>) -> Result<tonic::Response<Self::StreamSubscribeStream>, tonic::Status>;
        #[doc = "Server streaming response type for the PipelineSubscribe method."]
        type PipelineSubscribeStream: futures_core::Stream<Item = Result<super::PipelineSubscribeResponse, tonic::Status>> + Send + 'static;
        #[doc = " Open a pipeline subscriber channel."]
        async fn pipeline_subscribe(&self, request: tonic::Request<tonic::Streaming<super::PipelineSubscribeRequest>>) -> Result<tonic::Response<Self::PipelineSubscribeStream>, tonic::Status>;
    }
    #[doc = " The Hadron stream controller interface."]
    #[derive(Debug)]
    pub struct StreamControllerServer<T: StreamController> {
        inner: _Inner<T>,
        accept_compression_encodings: (),
        send_compression_encodings: (),
    }
    struct _Inner<T>(Arc<T>);
    impl<T: StreamController> StreamControllerServer<T> {
        pub fn new(inner: T) -> Self {
            let inner = Arc::new(inner);
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
            }
        }
        pub fn with_interceptor<F>(inner: T, interceptor: F) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for StreamControllerServer<T>
    where
        T: StreamController,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = Never;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/stream.StreamController/Metadata" => {
                    #[allow(non_camel_case_types)]
                    struct MetadataSvc<T: StreamController>(pub Arc<T>);
                    impl<T: StreamController> tonic::server::ServerStreamingService<super::MetadataRequest> for MetadataSvc<T> {
                        type Response = super::MetadataResponse;
                        type ResponseStream = T::MetadataStream;
                        type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<super::MetadataRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).metadata(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = MetadataSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(accept_compression_encodings, send_compression_encodings);
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/stream.StreamController/StreamPublish" => {
                    #[allow(non_camel_case_types)]
                    struct StreamPublishSvc<T: StreamController>(pub Arc<T>);
                    impl<T: StreamController> tonic::server::UnaryService<super::StreamPublishRequest> for StreamPublishSvc<T> {
                        type Response = super::StreamPublishResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<super::StreamPublishRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).stream_publish(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = StreamPublishSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(accept_compression_encodings, send_compression_encodings);
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/stream.StreamController/StreamSubscribe" => {
                    #[allow(non_camel_case_types)]
                    struct StreamSubscribeSvc<T: StreamController>(pub Arc<T>);
                    impl<T: StreamController> tonic::server::StreamingService<super::StreamSubscribeRequest> for StreamSubscribeSvc<T> {
                        type Response = super::StreamSubscribeResponse;
                        type ResponseStream = T::StreamSubscribeStream;
                        type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<tonic::Streaming<super::StreamSubscribeRequest>>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).stream_subscribe(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = StreamSubscribeSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(accept_compression_encodings, send_compression_encodings);
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/stream.StreamController/PipelineSubscribe" => {
                    #[allow(non_camel_case_types)]
                    struct PipelineSubscribeSvc<T: StreamController>(pub Arc<T>);
                    impl<T: StreamController> tonic::server::StreamingService<super::PipelineSubscribeRequest> for PipelineSubscribeSvc<T> {
                        type Response = super::PipelineSubscribeResponse;
                        type ResponseStream = T::PipelineSubscribeStream;
                        type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<tonic::Streaming<super::PipelineSubscribeRequest>>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).pipeline_subscribe(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = PipelineSubscribeSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(accept_compression_encodings, send_compression_encodings);
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => Box::pin(async move {
                    Ok(http::Response::builder()
                        .status(200)
                        .header("grpc-status", "12")
                        .header("content-type", "application/grpc")
                        .body(empty_body())
                        .unwrap())
                }),
            }
        }
    }
    impl<T: StreamController> Clone for StreamControllerServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
            }
        }
    }
    impl<T: StreamController> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: StreamController> tonic::transport::NamedService for StreamControllerServer<T> {
        const NAME: &'static str = "stream.StreamController";
    }
}
