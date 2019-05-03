railgun
=======
A distributed streaming and messaging platform written in Rust.

The name is not final. Might go with Lepton, as leptons (which include electrons) are necessary for the formation of hadrons, and thus all normal matter. Events from an event stream are the source of data for materialized views.

## central thesis
Older AMQP style systems were great and have some advantages, but fall short of Kafka style streaming capabilities, specifically message persistence. More recent blends of the technologies, like Nats, offer a nice blend, but lack many of the core features needed from both domains.

Rust is an excellent language to use for writing a system such as this. We will build upon the tokio runtime for networking. We will use Rust's async/await system for readability and approachability of the system (hopefully will have more contributors this way). We will leverage message passing patterns within the system to avoid locking & leverage Rust’s ownership system to its maximum potential.

## three types of streams
Everything is a stream.

- **Persistent (PS):** Messages are persistent. No topics. Optional ID time box for deduplication and transaction like semantics.
- **Ephemeral (ES):** AMQP exchange style stream. Topics are used for consumer matching.
- **Response (RS):** Similar to an ephemeral stream, except it is immediately removed after first message is published to it.













## clustering
- Clustering is natively supported. Cluster roles are dynamic.
- Raft is used for cluster consensus.
- Nodes may function in a few different roles: `master`, `writer`, `reader`. Nodes are master eligible (`writer`) by default, but can be configured to be read-only (`reader`) so that they can not become the cluster master.
- Dead cluster members may be pruned after some period of time based on cluster configuration.

### discovery
When a node first comes online, the discovery system will be booted to monitor for peers. The raft protocol is used to establish a cluster master. Once a master has been elected, all other nodes will treat it as the source of truth for writer delegates of streams.

### node lifecycle
- Nodes may be in a few differnt states during their lifecycle.
- `syncing`: a node has come online for the first time or after some period of time, and it needs to sync with the most recent state of the system.

### writing data
This applies primarily to persistent streams. This section introduces the term `writer delegate`. A writer delegate is a node, which must at least be a writer, which is responsible for handle all write operations on a stream.
- When a stream is first created, the cluster master will elect a writer node to act as the writer delegate for the new stream.
- The cluster master will ensure that there is always a live writer delegate for every stream. A single node may function as the writer delegate for multiple streams.
- The master will broadcast a writer delegate removal to all nodes (and wait for all writers to respond) before broadcasting the new writer delegate for a stream. This should typically be faster than a standard cluster election process if raft were being used instead of writer delegates.
- When a new stream is created, it will get an initial ID starting point for the stream. As data replication events are broadcast, the payload will also include the ID used for the message so that IDs will always be monotinically increasing, which guarantees strict ordering within the database. If a writer delegate goes down and a new writer delegate is elected, it will be able to begin generating new IDs for the target stream seamlessly.

**Data organization:**
- Within the DB, each stream receives a top-level keyspace based on the name. Stream names must be simple, matching the character set `[-_A-Za-z0-9.]+`.
- The streams concrete data is stored under the keyspace as follows `/streams/{streamName}/{id}`. This allows for easily watching a keyspace for consumer operations.
- This system does not use oplogs as there is only one type of operation which can be applied to persistent streams. Replication is synchronous to all other writers nodes, so writer delegate changes will normally not require the master sync.
- The cluster's membership, node state, and writer delegates are stored in the database under `/cluster/metadata/`. Each change gets an ID just as a standard persistent stream. This data is synchronously replicated to all writer nodes. Reader nodes receive the data at the same time, but the operation will be considered complete after writer nodes are synced.

### data replication


#### discovery
Will support a few discovery protocols. Allows members to automatically join as long as they can present needed credentials. AKA, cluster formation, peer discovery, auto clustering.

- `crate discovery_dns`: DNS based discovery.
- `crate discovery_consul`: Consul based discovery.
- `crate discovery_etcd`: Etcd based discovery.

### networking
- Railgun client <-> server communication takes place over multiplexed TCP keepalive connections (similar to AMQP style systems).
- Clients may use a single pipe for consumption as well as publication.
- Server <-> server clustering communication takes place over persistent TCP connections as well.
- Will probably use protobuf as the framing protocol, maybe capnproto
- Nodes within a cluster may forward commands between each other as needed.

### ack & nack
Messages being consumed from a stream must be ack'ed. At this point, no batch processing patterns are planned, just scale out the number of consumers in the group via concurrency model or horizontal scaling.

Messages may be nack'ed. By default, they will be immediately redelivered. A delay may be optionally specified which will block only the consumer or consumer group which sent the delay.

### cap
As far as CAP theorem, this system prioritizes *Availability* and *Partition Tolerance*. This works perfectly for this type of system as persistent streams are immutable. So consistency is purely a matter whether the replica node has the most recent additions to the stream. There is no MVCC to be concerned about, no atomic updating concerns or the like as transactions are not currently on the roadmap.

However, for a few of the features of this system, full synchronization is used to ensure write/mutation consistency.


--------------------------------------------------------------------------------------------------


#### ps unique id option
If a stream is configured to check message ID uniqueness, every message published to the stream must include an ID and it will have its ID checked. This will require the stream to have fully synchronous replication between all members participating in region.

When a node receives a write request targeting a unique ID ps stream, it will first check its local storage to ensure the ID is not a duplicate.
- if message ID is a dupe, it will respond with an error immediately.
- else continue.

Then it will send out a non-locking transaction request to all other members of the region. When received by the other nodes, they must place the requested ID in a pending write pool for the stream and then check to ensure that it is not a duplicate ID according to their current state. They must also check to ensure that the ID doesn't exist in their own pending write pool.
- if the message ID is a dupe in their current stream state, or in their pending write pool, they will respond with an error to the source node. If the source node receives even a single rejection from a peer, it will send a cencelation to all other peers and respond with an error to the client.
- if a transient error is received from a peer, the same request will be sent to the peer until a proper response is received. If a peer is removed from the region while awaiting a response, the peer will be removed from this process. A timeout will be hit if a peer never responds, and then the whole operation will be cancelled.
- if peers respond affirmatively, then we continue.

Once all members respond with an affirmative, the source node will commit its change, and then send a commit message to all peers. This is noteably different than when dealing with raft-based consensus for data management. This is due to the nature of CRDTs and the fact that this has taken place during a full peer sync. Peer responses to the commit message do not impact the success of the operation. If a transient error has taken place or the like, the source node will still be able to respond to the client, and the failed message to the peer will be retried on a backoff with some pre-defined retry limit.

#### replication protocol
This system is a masterless system in respect to data management. A client writing data to the node it is connected to may choose to wait for various replication levels, `initial`, `majority` & `all`. `initial` meaning the node will respond after the data is first written to the source node; `majority` meaning the client will respond after a majority of replicating members in the cluster have acknowledged the write; `all` meaning the client will respond after all replicating members in the cluster have acknowledged the write.

Nodes will forward client connections to their peers if they are not participating in the target region of a stream. Connections will also be forwarded to peers if the source node is far behind in the replication process.

#### gossip protocol
The gossip protocol is used for distributed consensus of regional data only. The raft protocol is used for overall cluster membership. The purpose of the gossip protocol is to ensure that data replication and consensus is maintained as quickly and efficiently as possible while still allowing for transient errors to take place in the replication protocol without system disruption.

It works as follows:

#### stream sharding
Do we want this?

#### master-less data | masterful clustering
Railgun uses a master-less model for data management, enabled by its use of CRDTs and the type of data being stored. For cluster membership, however, the raft protocol is used, and a single master is used for cluster membership consensus as well as some decision making within the cluster.

The cluster master keeps track of all cluster membership data, including node IDs, node region participation and the like. The master replicates this data with all nodes which present themselves as being master-eligible.

The master will hold in-memory data on regional stream consumer delegates (nodes which have been elected as being responsible for reading/consuming a persistent stream for consumers).
