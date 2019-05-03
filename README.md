railgun
=======
A distributed streaming and messaging platform written in Rust.

The name is not final. Might go with Lepton, as leptons (which include electrons) are necessary for the formation of hadrons, and thus all normal matter. Events from an event stream are typically the source of data for materialized views.

### central thesis
Older AMQP style systems were great and have some advantages, but fall short of Kafka style streaming capabilities, specifically message persistence. More recent blends of the technologies, like Nats, offer a nice blend, but lack many of the core features needed from both domains.

Rust is an excellent language to use for writing a system such as this. We will build upon the tokio runtime for networking. Will use Rust's async/await system for readability and approachability of the system (hopefully will have more contributors this way). Will leverage message passing patterns within the system to completely avoid locks & leverage Rust’s ownership system to its maximum potential.

### three types of streams
Everything is a stream.

- **Persistent (PS):** Messages are persistent. No topics. Optional ID time box for deduplication and transaction like semantics.
- **Ephemeral (ES):** AMQP exchange style stream. Topics are used for consumer matching.
- **Response (RS):** Similar to an ephemeral stream, except it is immediately removed after first message is published to it.

### ps streams
- Provides **at least once delivery semantics** by default, or **exactly once delivery semantics** if outputing to a stream with message ID uniqueness enabled and the consumer client is properly acking messages.
- Does not specify number of partitions. Ordering is determined by time of receipt and is based on CRDTs. Streams are replicated to all members of a region for redundancy and HA, but due to nature of CRDTs replication can very often by asynchronous.
- Messages have no concept of “topics” on a PS.
- Messages must be ack’ed. Messages may be nack’ed to indicate that redelivery is needed. Redelivery may be immediate or may have a delay applied to the message (for the consumer group only). Automatic redelivery takes place when consumer dies and can not ack or nack message.
- Support for optional message ID uniqueness. If a stream is configured with message uniquness, all messages will have their IDs checked against the indexed IDs for that stream. Duplicate IDs will be rejected. This provides a builtin mechanism for deduplication. This type of stream requires full synchronization between participating nodes. **See [ps unique id option](#ps-unique-id-option).**
- Consumers may consume from streams independently, or as groups. With persistent offset tracking or ephemeral offsets. Server will track offset of consumers automatically. See [ps offset tracking](#ps-offset-tracking).
- Group consumers will simply have messages load balanced across group members as messages arrive. Group membership is based purely off of a simple string ID provided when a consumer first connects.
- Consumers will receive all messages without any message level discrimination. Consumers may specify start points: start, end, or specific offset. If consumer already has a tracked offset, then start or end starting points will be ignored. Server keeps track of location of consumer by ID and offset.
- Stream should be `ensured` first to ensure that it exists with the expected configuration.

### es streams
- Provides **at most once delivery semantics.**
- Messages on an ephemeral stream may have “topics” (AMQP style). Defaulting to empty string. No ack or nack on ES streams.
- Consumers may specify a “topic” matcher. Defaulting to a match all wildcard. AMQP style matchers are supported. If no consumer matches the topic of the message, it will be dropped.
- Consumers may form groups, where messages will be load balanced across healthy group members.
- Messages will be delivered once to each consuming entity by default. Entities are individual consumers or groups.
- Stream should be `ensured` first to ensure that it exists with the expected configuration.

### rs streams
- Messages published to an ephemeral stream may include a “response” field which must be a string ID. The ID must be unique, and must be generated by the client in order to accurately match the response to the request call.
- Can not be subscribed to explicitly. The publisher will be automatically subscribed to the response stream, and can be the only subscriber.
- A timeout may be optionally supplied when the request message is initially published, else a default timeout config will be used. Clients do not need to manually timeout. The server will respond with the timeout error as needed.


--------------------------------------------------------------------------------------------------


### primitives

##### node
A node is an individual process instance. Usually running in a container, VM or the like.

##### node configuration
The static configuration for a node when it is started. Configuration is broken up into a few individual sections. The decision has been made (for now) not to support environment variable based config, or some sort of inheritence like cascading config. Only a config file will be used.

- `cluster`: Configuration for the cluster overall. Once the cluster has started successfully, certain pieces of the cluster configuration can not be changed by way of static configuration updates, but only via the Admin API.
- `node`: Configuration specifically for the parent node. This will hold things like the regions which this node is part of. These config options can be updated freely using the node's static config.

##### region
A region is simply a tag used to form replication groups among nodes.

- Streams are always associated with one, and only one, region.
- When streams are created, a region may be specified (defaulting to `global` if not specified).
- A default region `global` will always be present, and all nodes entering the cluster will be eligible to participate in the `global` replication group, unless configured otherwise.
- Nodes may be configured as being eligible for becoming master or just replication for specific regions. This supports the pattern where some regions may want to replicate global streams for reads in a region, but may not want a node in that region to become a master.
- If a region is specified for a stream, but there are no nodes in the cluster which are configured to replicate that region, then the command will result in an error.
- Regions do not act as a namespace for streams. Stream names must be unique throughout the cluster.

##### stream
Streams are the central most concept in this system. All data exists in streams. There are three types of streams. Streams belong to exactly one region, which will default to the `global` region. Stream names must be unique throughout the cluster.

##### cluster
A grouping of nodes working together. Clustering is natively supported by this system. Clustering is dynamic and there are a few different options available for automatic cluster formation.

##### message
A message is a structured blob of data inbound to or outbound from a stream.

##### consumer
A consumer is a process which is connected to the cluster and is configured to receive messages from some set of streams in the cluster. Consumers may form groups.

##### dlq
Dead letter queue. This is a longstanding paradigm which represents a resting place for messages which simply can not be successfully processed for some reason. In this system, persistent streams may be configured to automatically create a DLQ and have messages sent their if they fail to be processed successfully according to some configuration parameters.


--------------------------------------------------------------------------------------------------


## clustering
- Clustering is natively supported. Cluster roles are dynamic. Nodes are master eligible by default, but can be configured to be read-only.
- Using Raft for consensus.
- Dead cluster members may be pruned after some period of time based on cluster configuration.

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


--------------------------------------------------------------------------------------------------


### persistent streams with unique ID checking
Writing to an ID checked stream will go through a selected master for the region. This will keep throughput high and won’t require as much coordination. When such a persistent stream is first `ensured` by a client, the node, which the client is connected to, will create the needed data on disk first, will broadcast the data to all regional peers, and will then emit this data to the master for node selection.

Once the regional delegate has been selected for handling the new stream, it will broadcast this data to all members of that region.

unique ID pros:
- works well with systems outside of the cluster as unique message IDs can be used to guarantee uniqueness across system boundaries. Consider a consumer process which needs to receive a message with a unique ID from a stream, write information to a traditional database and then write a result to another stream.
    - the DB operation may fail in various ways, and if the DB operation does succeed, the process of writing to the output stream may fail for some transient reason.
    - the DB operation can be made to be idempotent, as the database can create a unique index on a field corresponding to the ID of the message.
    - the operation of writing to the output stream can be idempotent as well as long as the output stream uses unique ID checking and the ID of the original message is used when writing to the output stream.

unique IDs cons:
- error handling is needed to check for ID violation errors when publishing, in addition to checking for standard network errors while publishing.
- unique IDs must be used by the client. This system does not supply the ID as that would defeat the purpose of having unique IDs checked.


--------------------------------------------------------------------------------------------------


### ephemeral stream consumption
- Data is not stored for these types of streams. When a node receives a publication, it will pump the event out to all peers involved in the region. Each node maintains info on all connected clients and will push messages to matching connected clients. This is held in memory only.
- Ephemeral stream consumers may form groups by presenting an ID. When groups are formed, group membership metadata is broadcast to all nodes in participating region synchronously. The consumption request will only succeed after all participating members in the stream's region have acknowledged the broadcast. The node/groupID/client combination data will be held in memory on each regional node.
- When a node receives a publication which would match a consumer group, the node must randomly choose a single client to send the message to for load-balancing. The load-balancing decision — which includes the node/groupID/client data — will be broadcast along with the message to all regional nodes.
- When messages are published to an ephemeral stream, a region must be specified. Leaving the region null represents the cluster-wide default region. All nodes participate in this region.
- When a request message is published, the response stream will be created in the region corresponding to the region which the request message was published to. Before the payload of the request is published, the ID of the response stream will be broadcast to all regional members synchronously. After all regional members have acknowledged the broadcast, the payload may be published. This ensures that the response stream is created before the payload is sent to mitigate potential race conditions. Response stream IDs are held in memory by regional members. This ensures that duplicate response stream IDs will not occur.

### persistent stream consumption
**The central difficulty** for group consumers of persistent streams, as persistent streams store their data in a master-less fashion using CRDTs, is that the read process must come from a single entity at any point it time. Some amount of coordination is required. The cluster master will be leveraged for selecting a regional node for handling the responsibility of reading a stream from a particular offset and emitting the events out to the various members of the consumer group.
- the master will not need to keep track of the offset which has been read for the consumer group.
- the offset of the consumer group will be broadcast from the elected handler node to all regional members. The offset read, the corresponding event and the selected node/client combination will be emitted to all regional members to bundle the replication & load-balancing process. The offset, event and node/client decision payload is referred to as `group read event`.
- when a group read event is received by a node, it will first persist the offset; either in memory if the group is ephemeral, or on disk if it is a durable group. Then the payload will be routed to the connected client if applicable.


- Consumer IDs can be used to form groups. If no consumer ID is presented, then the consumer is ephemeral.
- Ephemeral consumer offsets are held in memory only by the source node the consumer is connected to.
- Ephemeral group consumers behave identically to group consumers of ephemeral streams, except that the offset data is broadcast along with payload for regional members to hold in memory.
- Durable group consumers behave identically to their ephemeral counterparts, except that their offsets are stored on disk.
- Coordination is required when dealing with persistent stream group consumers. The cluster master will be used to select a single regional node to act as the consumer group coordinator. The selected node will then begin the process of load balancing messages across regional members while updating in-memory or disk data for offset tracking.
- Consuming from an ID-checked stream behaves the same way as other persistent streams.


--------------------------------------------------------------------------------------------------


### regional streams | pros cons
##### abstract
The main idea with regions is that nodes can be configured to belong to particular regions. Regions are simply string values, and don't have to correspond to actual geographic regions.
- nodes may participate in multiple regions.
- all nodes participate in the default region, which is always present, and is not specified in config.
- regional participation must be explicit (other than the default region).

###### pros
- This would allow for complex cluster configurations where a single cluster may be composed of multiple regional sub-clusters.
- As streams are associated with regions, regional sub-clusters may be able to replicate work more efficiently due to networking proximity.
- Stream reading/writing will be more efficient for clients connecting to closer regions as their operations will not need to be sent or replicated accross the entire cluster.

###### cons
- Is this really needed? In the world of k8s/nomad/consule and the like, being able to deploy independent railgun clusters in specific regions and treating them as independent services would achieve roughly the same thing. Application level coordination between regions would still need to be developed by end-users in either case.

**Consensus:** it would seem that the regional abstraction is unnecessary for actual geographical distribution of the service. Instead, let's focus on a much more simple raft model with a single master. Non-master nodes will still proxy over to the master for writes. Reads can still be directed to nodes which are geographically closer based on k8s tags or the like.

Perhaps the only fancy abstraction which we will really want is the concept of read-only nodes. Which, by definition, could not become master nodes.
