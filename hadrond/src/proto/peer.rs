//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

/// A handshake frame holding all data needed for a successful handshake between peers.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HandshakeMsg {
    /// The ID of the node sending this frame.
    #[prost(uint64, tag = "1")]
    pub node_id: u64,
}
//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

/// A Raft AppendEntries RPC.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RaftAppendEntriesMsg {
    #[prost(string, tag = "1")]
    pub cluster: std::string::String,
    #[prost(bytes, tag = "2")]
    pub payload: std::vec::Vec<u8>,
}
/// A Raft Vote RPC.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RaftVoteMsg {
    #[prost(string, tag = "1")]
    pub cluster: std::string::String,
    #[prost(bytes, tag = "2")]
    pub payload: std::vec::Vec<u8>,
}
/// A Raft InstallSnapshot RPC.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RaftInstallSnapshotMsg {
    #[prost(string, tag = "1")]
    pub cluster: std::string::String,
    #[prost(bytes, tag = "2")]
    pub payload: std::vec::Vec<u8>,
}
#[doc = r" Generated client implementations."]
pub mod peer_client {
    #![allow(unused_variables, dead_code, missing_docs)]
    use tonic::codegen::*;
    pub struct PeerClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl PeerClient<tonic::transport::Channel> {
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
    impl<T> PeerClient<T>
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
        #[doc = " A handshake request, part of the initial peer connection setup."]
        pub async fn handshake(
            &mut self,
            request: impl tonic::IntoRequest<super::HandshakeMsg>,
        ) -> Result<tonic::Response<super::HandshakeMsg>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/peer.Peer/Handshake");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " A Raft AppendEntries RPC."]
        pub async fn raft_append_entries(
            &mut self,
            request: impl tonic::IntoRequest<super::RaftAppendEntriesMsg>,
        ) -> Result<tonic::Response<super::RaftAppendEntriesMsg>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/peer.Peer/RaftAppendEntries");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " A Raft Vote RPC."]
        pub async fn raft_vote(
            &mut self,
            request: impl tonic::IntoRequest<super::RaftVoteMsg>,
        ) -> Result<tonic::Response<super::RaftVoteMsg>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/peer.Peer/RaftVote");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " A Raft InstallSnapshot RPC."]
        pub async fn raft_install_snapshot(
            &mut self,
            request: impl tonic::IntoRequest<super::RaftInstallSnapshotMsg>,
        ) -> Result<tonic::Response<super::RaftInstallSnapshotMsg>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/peer.Peer/RaftInstallSnapshot");
            self.inner.unary(request.into_request(), path, codec).await
        }
    }
    impl<T: Clone> Clone for PeerClient<T> {
        fn clone(&self) -> Self {
            Self {
                inner: self.inner.clone(),
            }
        }
    }
    impl<T> std::fmt::Debug for PeerClient<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "PeerClient {{ ... }}")
        }
    }
}
#[doc = r" Generated server implementations."]
pub mod peer_server {
    #![allow(unused_variables, dead_code, missing_docs)]
    use tonic::codegen::*;
    #[doc = "Generated trait containing gRPC methods that should be implemented for use with PeerServer."]
    #[async_trait]
    pub trait Peer: Send + Sync + 'static {
        #[doc = " A handshake request, part of the initial peer connection setup."]
        async fn handshake(
            &self,
            request: tonic::Request<super::HandshakeMsg>,
        ) -> Result<tonic::Response<super::HandshakeMsg>, tonic::Status>;
        #[doc = " A Raft AppendEntries RPC."]
        async fn raft_append_entries(
            &self,
            request: tonic::Request<super::RaftAppendEntriesMsg>,
        ) -> Result<tonic::Response<super::RaftAppendEntriesMsg>, tonic::Status>;
        #[doc = " A Raft Vote RPC."]
        async fn raft_vote(
            &self,
            request: tonic::Request<super::RaftVoteMsg>,
        ) -> Result<tonic::Response<super::RaftVoteMsg>, tonic::Status>;
        #[doc = " A Raft InstallSnapshot RPC."]
        async fn raft_install_snapshot(
            &self,
            request: tonic::Request<super::RaftInstallSnapshotMsg>,
        ) -> Result<tonic::Response<super::RaftInstallSnapshotMsg>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct PeerServer<T: Peer> {
        inner: _Inner<T>,
    }
    struct _Inner<T>(Arc<T>, Option<tonic::Interceptor>);
    impl<T: Peer> PeerServer<T> {
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
    impl<T, B> Service<http::Request<B>> for PeerServer<T>
    where
        T: Peer,
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
                "/peer.Peer/Handshake" => {
                    #[allow(non_camel_case_types)]
                    struct HandshakeSvc<T: Peer>(pub Arc<T>);
                    impl<T: Peer> tonic::server::UnaryService<super::HandshakeMsg> for HandshakeSvc<T> {
                        type Response = super::HandshakeMsg;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::HandshakeMsg>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).handshake(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1.clone();
                        let inner = inner.0;
                        let method = HandshakeSvc(inner);
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
                "/peer.Peer/RaftAppendEntries" => {
                    #[allow(non_camel_case_types)]
                    struct RaftAppendEntriesSvc<T: Peer>(pub Arc<T>);
                    impl<T: Peer> tonic::server::UnaryService<super::RaftAppendEntriesMsg> for RaftAppendEntriesSvc<T> {
                        type Response = super::RaftAppendEntriesMsg;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RaftAppendEntriesMsg>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).raft_append_entries(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1.clone();
                        let inner = inner.0;
                        let method = RaftAppendEntriesSvc(inner);
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
                "/peer.Peer/RaftVote" => {
                    #[allow(non_camel_case_types)]
                    struct RaftVoteSvc<T: Peer>(pub Arc<T>);
                    impl<T: Peer> tonic::server::UnaryService<super::RaftVoteMsg> for RaftVoteSvc<T> {
                        type Response = super::RaftVoteMsg;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RaftVoteMsg>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).raft_vote(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1.clone();
                        let inner = inner.0;
                        let method = RaftVoteSvc(inner);
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
                "/peer.Peer/RaftInstallSnapshot" => {
                    #[allow(non_camel_case_types)]
                    struct RaftInstallSnapshotSvc<T: Peer>(pub Arc<T>);
                    impl<T: Peer> tonic::server::UnaryService<super::RaftInstallSnapshotMsg>
                        for RaftInstallSnapshotSvc<T>
                    {
                        type Response = super::RaftInstallSnapshotMsg;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RaftInstallSnapshotMsg>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).raft_install_snapshot(request).await };
                            Box::pin(fut)
                        }
                    }
                    let inner = self.inner.clone();
                    let fut = async move {
                        let interceptor = inner.1.clone();
                        let inner = inner.0;
                        let method = RaftInstallSnapshotSvc(inner);
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
    impl<T: Peer> Clone for PeerServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self { inner }
        }
    }
    impl<T: Peer> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone(), self.1.clone())
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: Peer> tonic::transport::NamedService for PeerServer<T> {
        const NAME: &'static str = "peer.Peer";
    }
}
