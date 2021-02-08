//! Stream Partition Controller.
//!
//! This controller is responsible for a partition of a stream. All reads & writes for any specific
//! partition will be handled by the leader of a SPC group.
//!
//! SPCs form replication groups based on the nomination of the Cluster Raft Controller (CRC). The
//! leader of the replication group handles all reads and writes, and replicates data to the
//! followers of the partition. See the
//! [CRC Control Group Leader Nomination Algorithm](../crc/index.html) for more details.
//!
//! SPC control groups use a commit index to track replication of data per stream partition.
