/// A storage struct for a Raft log's metadata.
///
/// This system uses a multiraft setup. All streams have their own Raft instance.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RaftState {
    /// The voting term.
    #[prost(uint64, required, tag="1")]
    pub term: u64,
    /// The node ID which was voted for during `term`.
    #[prost(uint64, required, tag="2")]
    pub vote: u64,
    /// The last commit index.
    #[prost(uint64, required, tag="3")]
    pub commit: u64,
    /// All known voting members of the cluster.
    #[prost(uint64, repeated, tag="4")]
    pub nodes: ::std::vec::Vec<u64>,
    /// All known learner members of the cluster.
    #[prost(uint64, repeated, tag="5")]
    pub learners: ::std::vec::Vec<u64>,
}
/// A storage struct for the Railgun cluster's metadata.
///
/// A base-line Raft metadata struct is embedded here for the cluster's Raft instance, and
/// additional fields are used for the cluster's specific metadata.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ClusterState {
    /// The cluster's Raft metadata.
    #[prost(message, required, tag="1")]
    pub raft: RaftState,
    /// All known streams by name.
    ///
    /// To ensure that there are no duplicates, a map is used here. The value is meaningless.
    #[prost(map="string, uint32", tag="2")]
    pub streams: ::std::collections::HashMap<std::string::String, u32>,
}
