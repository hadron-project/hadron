Clustering
==========
Clustering is natively supported in Railgun via the Raft protocol. All nodes participate in the cluster's Raft. This guarantees consistency and safety for the cluster's data. Dead cluster members will be pruned after some period of time based on cluster configuration, as long as the cluster leader determines that it is safe to do so. This is also known as cluster auto-healing.

### cap
In respect to CAP theorem, Railgun is a CP system. That is, Railgun prioritizes *Consistency* and *Partition Tolerance*. This works perfectly for this type of system as persistent streams are immutable and strict linearizability is required. As the ephemeral messaging & RPC systems do not store their data on disk, they are not interrupted during availability failures.
