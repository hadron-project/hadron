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
