//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct TransactionClient {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct TransactionServer {}
//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct EphemeralPubRequest {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct EphemeralPubResponse {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct EphemeralSubClient {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct EphemeralSubServer {}
//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct RpcPubRequest {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct RpcPubResponse {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct RpcSubClient {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct RpcSubServer {}
//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct StreamPubRequest {
    /// The namespace of the stream to which this event will be published.
    #[prost(string, tag = "1")]
    pub namespace: std::string::String,
    /// The stream to which this event will be published.
    #[prost(string, tag = "2")]
    pub stream: std::string::String,
    /// The payload of this event.
    #[prost(bytes, tag = "3")]
    pub payload: std::vec::Vec<u8>,
}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct StreamPubResponse {
    /// The ID of the newly created event.
    #[prost(uint64, tag = "1")]
    pub id: u64,
}
/// TODO: add a field `overwrite` which will cause the config presented in this subscription
/// request to overwrite the subscription's current config. Consumer apps can be deployed in such
/// a way that rollbacks and version changes will not confict and cause unexpected sub config.
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct StreamSubClient {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct StreamSubServer {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct StreamUnsubRequest {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct StreamUnsubResponse {}
//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct PipelineStageSubClient {}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct PipelineStageSubServer {}
//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct UpdateSchemaRequest {
    #[prost(oneof = "update_schema_request::Update", tags = "1, 2")]
    pub update: ::std::option::Option<update_schema_request::Update>,
}
pub mod update_schema_request {
    #[derive(Clone, PartialEq, ::prost::Oneof, serde::Serialize, serde::Deserialize)]
    pub enum Update {
        #[prost(message, tag = "1")]
        Oneoff(super::UpdateSchemaOneOff),
        #[prost(message, tag = "2")]
        Managed(super::UpdateSchemaManaged),
    }
}
/// A one-off schema update, which is not guaranteed to be idempotent.
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct UpdateSchemaOneOff {
    /// A set of Hadron schema documents to apply to the system.
    #[prost(string, tag = "1")]
    pub schema: std::string::String,
}
/// An idempotent managed schema update.
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct UpdateSchemaManaged {
    /// The branch name of this set of schema updates.
    #[prost(string, tag = "1")]
    pub branch: std::string::String,
    /// The timestamp of this set of schema updates.
    ///
    /// This should be an epoch timestamp with millisecond precision.
    #[prost(int64, tag = "2")]
    pub timestamp: i64,
    /// A set of Hadron schema documents to apply to the system.
    #[prost(string, tag = "3")]
    pub schema: std::string::String,
}
#[derive(Clone, PartialEq, ::prost::Message, serde::Serialize, serde::Deserialize)]
pub struct UpdateSchemaResponse {
    /// A bool indicating if the request completed as a no-op.
    #[prost(bool, tag = "1")]
    pub was_noop: bool,
}
#[doc = r" Generated client implementations."]
pub mod client_client {
    #![allow(unused_variables, dead_code, missing_docs)]
    use tonic::codegen::*;
    pub struct ClientClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl ClientClient<tonic::transport::Channel> {
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
    impl<T> ClientClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::ResponseBody: Body + HttpBody + Send + 'static,
        T::Error: Into<StdError>,
        <T::ResponseBody as HttpBody>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_interceptor(inner: T, interceptor: impl Into<tonic::Interceptor>) -> Self {
            let inner = tonic::client::Grpc::with_interceptor(inner, interceptor);
            Self { inner }
        }
        #[doc = " Open a new transaction."]
        #[doc = ""]
        #[doc = " This gRPC endpoint uses a bi-direction stream. The client sends `TransactionClient` messages and"]
        #[doc = " the server sends `TransactionServer` messages. The client will send an initial message to initialize"]
        #[doc = " the stream and the server will send an initial message to confirm initialization."]
        #[doc = ""]
        #[doc = " The initial response will return a transaction ID. The transaction ID may be included in"]
        #[doc = " `EphemeralPub` or `StreamPub` requests. Such requests will be recorded in the transaction, but"]
        #[doc = " will not be applied until the transaction commits. Once the transaction commits, all commands"]
        #[doc = " registered as part of the transaction will atomically be applied. If an error is encountered,"]
        #[doc = " then none of the commands will be applied."]
        #[doc = ""]
        #[doc = " Once all work in the transaction has been finished, the client should send a second `TransactionClient`"]
        #[doc = " message which will commit the transaction. The server will respond with a second `TransactionServer`"]
        #[doc = " indicating if the transaction succeeded for failed."]
        #[doc = ""]
        #[doc = " Transactions are scoped to the namespace."]
        pub async fn transaction(
            &mut self, request: impl tonic::IntoStreamingRequest<Message = super::TransactionClient>,
        ) -> Result<tonic::Response<tonic::codec::Streaming<super::TransactionServer>>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/client.Client/Transaction");
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        #[doc = " Publish an ephemeral message."]
        pub async fn ephemeral_pub(
            &mut self, request: impl tonic::IntoRequest<super::EphemeralPubRequest>,
        ) -> Result<tonic::Response<super::EphemeralPubResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/client.Client/EphemeralPub");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " Subscribe to an ephemeral messaging exchange using a given routing key."]
        #[doc = ""]
        #[doc = " This gRPC endpoint uses a uni-direction stream. The client sends an initial `EphemeralSubClient`"]
        #[doc = " message to setup the subscription stream, and the server will respond with an initial"]
        #[doc = " `EphemeralSubServer` message to confirm initialization."]
        #[doc = ""]
        #[doc = " From there, the server will send `EphemeralSubServer` messages containing published ephemeral"]
        #[doc = " messages matching this subscription's exchange and routing key."]
        pub async fn ephemeral_sub(
            &mut self, request: impl tonic::IntoRequest<super::EphemeralSubClient>,
        ) -> Result<tonic::Response<tonic::codec::Streaming<super::EphemeralSubServer>>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/client.Client/EphemeralSub");
            self.inner.server_streaming(request.into_request(), path, codec).await
        }
        #[doc = " Publish an RPC request and await its response."]
        pub async fn rpc_pub(
            &mut self, request: impl tonic::IntoRequest<super::RpcPubRequest>,
        ) -> Result<tonic::Response<super::RpcPubResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/client.Client/RpcPub");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " Subscribe as an RPC handler."]
        #[doc = ""]
        #[doc = " This gRPC endpoint uses a bi-direction stream. The client sends `RpcSubClient` messages and"]
        #[doc = " the server sends `RpcSubServer` messages. The client will send an initial message to initialize"]
        #[doc = " the stream and the server will send an initial message to confirm initialization."]
        #[doc = ""]
        #[doc = " From there, the server will send a `RpcSubServer` message any time an RPC has been published"]
        #[doc = " and this client connection has been chosen to handle the RPC. The client is then expected to"]
        #[doc = " handle the RPC and respond with a `RpcSubClient` message."]
        pub async fn rpc_sub(
            &mut self, request: impl tonic::IntoStreamingRequest<Message = super::RpcSubClient>,
        ) -> Result<tonic::Response<tonic::codec::Streaming<super::RpcSubServer>>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/client.Client/RpcSub");
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        #[doc = " Publish an event to a stream."]
        pub async fn stream_pub(
            &mut self, request: impl tonic::IntoRequest<super::StreamPubRequest>,
        ) -> Result<tonic::Response<super::StreamPubResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/client.Client/StreamPub");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " Subscribe as an event handler for a specific stream."]
        #[doc = ""]
        #[doc = " This gRPC endpoint uses a bi-direction stream. The client sends `StreamSubClient` messages and"]
        #[doc = " the server sends `StreamSubServer` messages. The client will send an initial message to initialize"]
        #[doc = " the stream and the server will send an initial message to confirm initialization."]
        #[doc = ""]
        #[doc = " From there, the server will send a `StreamSubServer` message any time an event has been published"]
        #[doc = " to the corresponding stream and this client connection has been chosen to handle event. The client"]
        #[doc = " is then expected to handle the event and respond with a `StreamSubClient` message, which will"]
        #[doc = " either acknowledge (`ack`) the event indicating that it was successfully processed, or will"]
        #[doc = " negatively acknowledge (`nack`) the event indicating that the event was not processed successfully."]
        pub async fn stream_sub(
            &mut self, request: impl tonic::IntoStreamingRequest<Message = super::StreamSubClient>,
        ) -> Result<tonic::Response<tonic::codec::Streaming<super::StreamSubServer>>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/client.Client/StreamSub");
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        #[doc = " Unsubscribes a stream consumer group, deleting the consumer group's offsets for the associated stream."]
        pub async fn stream_unsub(
            &mut self, request: impl tonic::IntoRequest<super::StreamUnsubRequest>,
        ) -> Result<tonic::Response<super::StreamUnsubResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/client.Client/StreamUnsub");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " Subscribe as an event handler for a specific pipeline stage."]
        #[doc = ""]
        #[doc = " This gRPC endpoint uses a bi-direction stream. The client sends `PipelineStageSubClient` messages and"]
        #[doc = " the server sends `PipelineStageSubServer` messages. The client will send an initial message to initialize"]
        #[doc = " the stream and the server will send an initial message to confirm initialization."]
        #[doc = ""]
        #[doc = " From there, the server will send a `PipelineStageSubServer` message to trigger a specific pipeline"]
        #[doc = " stage providing the stage's required input. The client is then expected to handle the input"]
        #[doc = " event for the specific stage and respond with a `PipelineStageSubClient` message, which will either"]
        #[doc = " acknowledge (`ack`) the stage providing any required outputs for the stage, or will negatively"]
        #[doc = " acknowledge (`nack`) the stage indicating that the stage was not processed successfully."]
        pub async fn pipeline_stage_sub(
            &mut self, request: impl tonic::IntoStreamingRequest<Message = super::PipelineStageSubClient>,
        ) -> Result<tonic::Response<tonic::codec::Streaming<super::PipelineStageSubServer>>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/client.Client/PipelineStageSub");
            self.inner.streaming(request.into_streaming_request(), path, codec).await
        }
        #[doc = " Update the schema of the Hadron cluster."]
        pub async fn update_schema(
            &mut self, request: impl tonic::IntoRequest<super::UpdateSchemaRequest>,
        ) -> Result<tonic::Response<super::UpdateSchemaResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into())))?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/client.Client/UpdateSchema");
            self.inner.unary(request.into_request(), path, codec).await
        }
    }
    impl<T: Clone> Clone for ClientClient<T> {
        fn clone(&self) -> Self {
            Self { inner: self.inner.clone() }
        }
    }
    impl<T> std::fmt::Debug for ClientClient<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "ClientClient {{ ... }}")
        }
    }
}
#[doc = r" Generated server implementations."]
pub mod client_server {
    #![allow(unused_variables, dead_code, missing_docs)]
    use tonic::codegen::*;
    #[doc = "Generated trait containing gRPC methods that should be implemented for use with ClientServer."]
    #[async_trait]
    pub trait Client: Send + Sync + 'static {
        #[doc = "Server streaming response type for the Transaction method."]
        type TransactionStream: Stream<Item = Result<super::TransactionServer, tonic::Status>> + Send + Sync + 'static;
        #[doc = " Open a new transaction."]
        #[doc = ""]
        #[doc = " This gRPC endpoint uses a bi-direction stream. The client sends `TransactionClient` messages and"]
        #[doc = " the server sends `TransactionServer` messages. The client will send an initial message to initialize"]
        #[doc = " the stream and the server will send an initial message to confirm initialization."]
        #[doc = ""]
        #[doc = " The initial response will return a transaction ID. The transaction ID may be included in"]
        #[doc = " `EphemeralPub` or `StreamPub` requests. Such requests will be recorded in the transaction, but"]
        #[doc = " will not be applied until the transaction commits. Once the transaction commits, all commands"]
        #[doc = " registered as part of the transaction will atomically be applied. If an error is encountered,"]
        #[doc = " then none of the commands will be applied."]
        #[doc = ""]
        #[doc = " Once all work in the transaction has been finished, the client should send a second `TransactionClient`"]
        #[doc = " message which will commit the transaction. The server will respond with a second `TransactionServer`"]
        #[doc = " indicating if the transaction succeeded for failed."]
        #[doc = ""]
        #[doc = " Transactions are scoped to the namespace."]
        async fn transaction(
            &self, request: tonic::Request<tonic::Streaming<super::TransactionClient>>,
        ) -> Result<tonic::Response<Self::TransactionStream>, tonic::Status>;
        #[doc = " Publish an ephemeral message."]
        async fn ephemeral_pub(
            &self, request: tonic::Request<super::EphemeralPubRequest>,
        ) -> Result<tonic::Response<super::EphemeralPubResponse>, tonic::Status>;
        #[doc = "Server streaming response type for the EphemeralSub method."]
        type EphemeralSubStream: Stream<Item = Result<super::EphemeralSubServer, tonic::Status>> + Send + Sync + 'static;
        #[doc = " Subscribe to an ephemeral messaging exchange using a given routing key."]
        #[doc = ""]
        #[doc = " This gRPC endpoint uses a uni-direction stream. The client sends an initial `EphemeralSubClient`"]
        #[doc = " message to setup the subscription stream, and the server will respond with an initial"]
        #[doc = " `EphemeralSubServer` message to confirm initialization."]
        #[doc = ""]
        #[doc = " From there, the server will send `EphemeralSubServer` messages containing published ephemeral"]
        #[doc = " messages matching this subscription's exchange and routing key."]
        async fn ephemeral_sub(
            &self, request: tonic::Request<super::EphemeralSubClient>,
        ) -> Result<tonic::Response<Self::EphemeralSubStream>, tonic::Status>;
        #[doc = " Publish an RPC request and await its response."]
        async fn rpc_pub(&self, request: tonic::Request<super::RpcPubRequest>) -> Result<tonic::Response<super::RpcPubResponse>, tonic::Status>;
        #[doc = "Server streaming response type for the RpcSub method."]
        type RpcSubStream: Stream<Item = Result<super::RpcSubServer, tonic::Status>> + Send + Sync + 'static;
        #[doc = " Subscribe as an RPC handler."]
        #[doc = ""]
        #[doc = " This gRPC endpoint uses a bi-direction stream. The client sends `RpcSubClient` messages and"]
        #[doc = " the server sends `RpcSubServer` messages. The client will send an initial message to initialize"]
        #[doc = " the stream and the server will send an initial message to confirm initialization."]
        #[doc = ""]
        #[doc = " From there, the server will send a `RpcSubServer` message any time an RPC has been published"]
        #[doc = " and this client connection has been chosen to handle the RPC. The client is then expected to"]
        #[doc = " handle the RPC and respond with a `RpcSubClient` message."]
        async fn rpc_sub(
            &self, request: tonic::Request<tonic::Streaming<super::RpcSubClient>>,
        ) -> Result<tonic::Response<Self::RpcSubStream>, tonic::Status>;
        #[doc = " Publish an event to a stream."]
        async fn stream_pub(
            &self, request: tonic::Request<super::StreamPubRequest>,
        ) -> Result<tonic::Response<super::StreamPubResponse>, tonic::Status>;
        #[doc = "Server streaming response type for the StreamSub method."]
        type StreamSubStream: Stream<Item = Result<super::StreamSubServer, tonic::Status>> + Send + Sync + 'static;
        #[doc = " Subscribe as an event handler for a specific stream."]
        #[doc = ""]
        #[doc = " This gRPC endpoint uses a bi-direction stream. The client sends `StreamSubClient` messages and"]
        #[doc = " the server sends `StreamSubServer` messages. The client will send an initial message to initialize"]
        #[doc = " the stream and the server will send an initial message to confirm initialization."]
        #[doc = ""]
        #[doc = " From there, the server will send a `StreamSubServer` message any time an event has been published"]
        #[doc = " to the corresponding stream and this client connection has been chosen to handle event. The client"]
        #[doc = " is then expected to handle the event and respond with a `StreamSubClient` message, which will"]
        #[doc = " either acknowledge (`ack`) the event indicating that it was successfully processed, or will"]
        #[doc = " negatively acknowledge (`nack`) the event indicating that the event was not processed successfully."]
        async fn stream_sub(
            &self, request: tonic::Request<tonic::Streaming<super::StreamSubClient>>,
        ) -> Result<tonic::Response<Self::StreamSubStream>, tonic::Status>;
        #[doc = " Unsubscribes a stream consumer group, deleting the consumer group's offsets for the associated stream."]
        async fn stream_unsub(
            &self, request: tonic::Request<super::StreamUnsubRequest>,
        ) -> Result<tonic::Response<super::StreamUnsubResponse>, tonic::Status>;
        #[doc = "Server streaming response type for the PipelineStageSub method."]
        type PipelineStageSubStream: Stream<Item = Result<super::PipelineStageSubServer, tonic::Status>> + Send + Sync + 'static;
        #[doc = " Subscribe as an event handler for a specific pipeline stage."]
        #[doc = ""]
        #[doc = " This gRPC endpoint uses a bi-direction stream. The client sends `PipelineStageSubClient` messages and"]
        #[doc = " the server sends `PipelineStageSubServer` messages. The client will send an initial message to initialize"]
        #[doc = " the stream and the server will send an initial message to confirm initialization."]
        #[doc = ""]
        #[doc = " From there, the server will send a `PipelineStageSubServer` message to trigger a specific pipeline"]
        #[doc = " stage providing the stage's required input. The client is then expected to handle the input"]
        #[doc = " event for the specific stage and respond with a `PipelineStageSubClient` message, which will either"]
        #[doc = " acknowledge (`ack`) the stage providing any required outputs for the stage, or will negatively"]
        #[doc = " acknowledge (`nack`) the stage indicating that the stage was not processed successfully."]
        async fn pipeline_stage_sub(
            &self, request: tonic::Request<tonic::Streaming<super::PipelineStageSubClient>>,
        ) -> Result<tonic::Response<Self::PipelineStageSubStream>, tonic::Status>;
        #[doc = " Update the schema of the Hadron cluster."]
        async fn update_schema(
            &self, request: tonic::Request<super::UpdateSchemaRequest>,
        ) -> Result<tonic::Response<super::UpdateSchemaResponse>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct ClientServer<T: Client> {
        inner: _Inner<T>,
    }
    struct _Inner<T>(Arc<T>, Option<tonic::Interceptor>);
    impl<T: Client> ClientServer<T> {
        pub fn new(inner: T) -> Self {
            let inner = Arc::new(inner);
            let inner = _Inner(inner, None);
            Self { inner }
        }
        pub fn with_interceptor(inner: T, interceptor: impl Into<tonic::Interceptor>) -> Self {
            let inner = Arc::new(inner);
            let inner = _Inner(inner, Some(interceptor.into()));
            Self { inner }
        }
    }
    impl<T, B> Service<http::Request<B>> for ClientServer<T>
    where
        T: Client,
        B: HttpBody + Send + Sync + 'static,
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
                "/client.Client/Transaction" => {
                    #[allow(non_camel_case_types)]
                    struct TransactionSvc<T: Client>(pub Arc<T>);
                    impl<T: Client> tonic::server::StreamingService<super::TransactionClient> for TransactionSvc<T> {
                        type Response = super::TransactionServer;
                        type ResponseStream = T::TransactionStream;
                        type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<tonic::Streaming<super::TransactionClient>>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).transaction(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1;
                        let inner = inner.0;
                        let method = TransactionSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = if let Some(interceptor) = interceptor {
                            tonic::server::Grpc::with_interceptor(codec, interceptor)
                        } else {
                            tonic::server::Grpc::new(codec)
                        };
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/client.Client/EphemeralPub" => {
                    #[allow(non_camel_case_types)]
                    struct EphemeralPubSvc<T: Client>(pub Arc<T>);
                    impl<T: Client> tonic::server::UnaryService<super::EphemeralPubRequest> for EphemeralPubSvc<T> {
                        type Response = super::EphemeralPubResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<super::EphemeralPubRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).ephemeral_pub(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1.clone();
                        let inner = inner.0;
                        let method = EphemeralPubSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = if let Some(interceptor) = interceptor {
                            tonic::server::Grpc::with_interceptor(codec, interceptor)
                        } else {
                            tonic::server::Grpc::new(codec)
                        };
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/client.Client/EphemeralSub" => {
                    #[allow(non_camel_case_types)]
                    struct EphemeralSubSvc<T: Client>(pub Arc<T>);
                    impl<T: Client> tonic::server::ServerStreamingService<super::EphemeralSubClient> for EphemeralSubSvc<T> {
                        type Response = super::EphemeralSubServer;
                        type ResponseStream = T::EphemeralSubStream;
                        type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<super::EphemeralSubClient>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).ephemeral_sub(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1;
                        let inner = inner.0;
                        let method = EphemeralSubSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = if let Some(interceptor) = interceptor {
                            tonic::server::Grpc::with_interceptor(codec, interceptor)
                        } else {
                            tonic::server::Grpc::new(codec)
                        };
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/client.Client/RpcPub" => {
                    #[allow(non_camel_case_types)]
                    struct RpcPubSvc<T: Client>(pub Arc<T>);
                    impl<T: Client> tonic::server::UnaryService<super::RpcPubRequest> for RpcPubSvc<T> {
                        type Response = super::RpcPubResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<super::RpcPubRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).rpc_pub(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1.clone();
                        let inner = inner.0;
                        let method = RpcPubSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = if let Some(interceptor) = interceptor {
                            tonic::server::Grpc::with_interceptor(codec, interceptor)
                        } else {
                            tonic::server::Grpc::new(codec)
                        };
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/client.Client/RpcSub" => {
                    #[allow(non_camel_case_types)]
                    struct RpcSubSvc<T: Client>(pub Arc<T>);
                    impl<T: Client> tonic::server::StreamingService<super::RpcSubClient> for RpcSubSvc<T> {
                        type Response = super::RpcSubServer;
                        type ResponseStream = T::RpcSubStream;
                        type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<tonic::Streaming<super::RpcSubClient>>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).rpc_sub(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1;
                        let inner = inner.0;
                        let method = RpcSubSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = if let Some(interceptor) = interceptor {
                            tonic::server::Grpc::with_interceptor(codec, interceptor)
                        } else {
                            tonic::server::Grpc::new(codec)
                        };
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/client.Client/StreamPub" => {
                    #[allow(non_camel_case_types)]
                    struct StreamPubSvc<T: Client>(pub Arc<T>);
                    impl<T: Client> tonic::server::UnaryService<super::StreamPubRequest> for StreamPubSvc<T> {
                        type Response = super::StreamPubResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<super::StreamPubRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).stream_pub(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1.clone();
                        let inner = inner.0;
                        let method = StreamPubSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = if let Some(interceptor) = interceptor {
                            tonic::server::Grpc::with_interceptor(codec, interceptor)
                        } else {
                            tonic::server::Grpc::new(codec)
                        };
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/client.Client/StreamSub" => {
                    #[allow(non_camel_case_types)]
                    struct StreamSubSvc<T: Client>(pub Arc<T>);
                    impl<T: Client> tonic::server::StreamingService<super::StreamSubClient> for StreamSubSvc<T> {
                        type Response = super::StreamSubServer;
                        type ResponseStream = T::StreamSubStream;
                        type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<tonic::Streaming<super::StreamSubClient>>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).stream_sub(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1;
                        let inner = inner.0;
                        let method = StreamSubSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = if let Some(interceptor) = interceptor {
                            tonic::server::Grpc::with_interceptor(codec, interceptor)
                        } else {
                            tonic::server::Grpc::new(codec)
                        };
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/client.Client/StreamUnsub" => {
                    #[allow(non_camel_case_types)]
                    struct StreamUnsubSvc<T: Client>(pub Arc<T>);
                    impl<T: Client> tonic::server::UnaryService<super::StreamUnsubRequest> for StreamUnsubSvc<T> {
                        type Response = super::StreamUnsubResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<super::StreamUnsubRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).stream_unsub(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1.clone();
                        let inner = inner.0;
                        let method = StreamUnsubSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = if let Some(interceptor) = interceptor {
                            tonic::server::Grpc::with_interceptor(codec, interceptor)
                        } else {
                            tonic::server::Grpc::new(codec)
                        };
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/client.Client/PipelineStageSub" => {
                    #[allow(non_camel_case_types)]
                    struct PipelineStageSubSvc<T: Client>(pub Arc<T>);
                    impl<T: Client> tonic::server::StreamingService<super::PipelineStageSubClient> for PipelineStageSubSvc<T> {
                        type Response = super::PipelineStageSubServer;
                        type ResponseStream = T::PipelineStageSubStream;
                        type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<tonic::Streaming<super::PipelineStageSubClient>>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).pipeline_stage_sub(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1;
                        let inner = inner.0;
                        let method = PipelineStageSubSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = if let Some(interceptor) = interceptor {
                            tonic::server::Grpc::with_interceptor(codec, interceptor)
                        } else {
                            tonic::server::Grpc::new(codec)
                        };
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/client.Client/UpdateSchema" => {
                    #[allow(non_camel_case_types)]
                    struct UpdateSchemaSvc<T: Client>(pub Arc<T>);
                    impl<T: Client> tonic::server::UnaryService<super::UpdateSchemaRequest> for UpdateSchemaSvc<T> {
                        type Response = super::UpdateSchemaResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<super::UpdateSchemaRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).update_schema(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1.clone();
                        let inner = inner.0;
                        let method = UpdateSchemaSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = if let Some(interceptor) = interceptor {
                            tonic::server::Grpc::with_interceptor(codec, interceptor)
                        } else {
                            tonic::server::Grpc::new(codec)
                        };
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => Box::pin(async move {
                    Ok(http::Response::builder()
                        .status(200)
                        .header("grpc-status", "12")
                        .body(tonic::body::BoxBody::empty())
                        .unwrap())
                }),
            }
        }
    }
    impl<T: Client> Clone for ClientServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self { inner }
        }
    }
    impl<T: Client> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone(), self.1.clone())
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: Client> tonic::transport::NamedService for ClientServer<T> {
        const NAME: &'static str = "client.Client";
    }
}
