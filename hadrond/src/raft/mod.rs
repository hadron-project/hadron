mod network;
mod storage;

use async_raft::Raft;

/// A Raft type which is backed by the Tonic network provider and Sled storage.
pub type HadronRaft<Req, Res, Bck> = Raft<Req, Res, TonicNetworkProvider<Req>, RaftStorageSled<Bck>>;

pub use network::TonicNetworkProvider;
pub use storage::{RaftStorageSled, SledStorageProvider};
