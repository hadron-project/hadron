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
- Provides **at least once delivery semantics** by default, or **exactly once delivery semantics** if using ID time boxes or properly functioning clients.
- Does not specify number of partitions. Ordering is determined by time of receipt. Streams are replicated heuristically to guard against data loss.
- Messages have no concept of “topics” on a PS.
- Messages must be ack’ed. Messages may be nack’ed to indicate that redelivery is needed. Redelivery may be immediate or may have a delay applied. Automatic redelivery takes place when consumer dies and can not ack or nack message.
- Support for optional time boxed unique message IDs. A message presented with an ID will have its ID checked against the time boxed IDs pool for that stream. If the ID was used within the time box, the message will be rejected. This provides a builtin mechanism for deduplication.
- Consumers may consume from streams independently, or as groups. Server will track offset of consumers automatically. Automatic DLQ on messages according to stream config when successful processing of the message falls a specific distance behind most recently consumed message.
- Consumer groups can be setup to be exclusive. This means that only one consumer from the group will receive messages at a time. This supports consumption ordering based on the time a message hit the stream.
- Non-exclusive consumer groups will simply have messages load balanced across group members as messages arrive.
- Consumers will receive all messages without any message level discrimination. May specify start points: start, end, or specific offset. If consumer already has a tracked offset, then start or end starting points will be ignored. Server keeps track of location of consumer by ID and offset. Groups will share this consumer ID to coordinate location in stream.

### es streams
- Provides **at most once delivery semantics.**
- Messages on an ephemeral stream may have “topics” (AMQP style). Defaulting to empty string. No ack or nack on ES streams.
- Consumers may specify a “topic” matcher. Defaulting to a match all wildcard. AMQP style matchers are supported. If no consumer matches the topic of the message, it will be dropped.
- Consumers may form groups, where messages will be load balanced across healthy group members.
- Messages will be delivered once to each consuming entity by default. Entities are individual consumers or groups.

### rs streams
- Messages published to an ES stream may include a “response” field, in which case a RS stream will be created matching the response field. Will error if already in use.
- Can not be subscribed to explicitly. The publisher will be automatically subscribed to the RS stream, and can be the only subscriber.
- A timeout may be optionally supplied when the message needing a response is initially published, else the default RS stream timeout config will be used. Clients do not need to manually timeout. The server will respond with the timeout error.

----

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

The cluster may span multiple geographic regions, and streams may be tagged to correspond with these geographic regions. L3-L4 coordination should be employed for clients to target the regions that they are interested in to maximize throughput and reduce latency.

Members of the cluster will forward requests to other nodes as needed for reads on streams which the receiving node does not replicate. For write operations, the request will be forwarded to the master for the target stream's region.

##### message
A message is a structured blob of data inbound to or outbound from a stream.

##### consumer
A consumer is a process which is connected to the cluster and is configured to receive messages from some set of streams in the cluster. Consumers may form groups.

##### dlq
Dead letter queue. This is a longstanding paradigm which represents resting place for messages which simply can not be successfully processed for some reason. In this system, persistent streams may be configured to automatically create a DLQ and have messages sent their if they fail to be processed successfully according to some configuration parameters.

----

### clustering
- Clustering is natively supported. Cluster roles are dynamic. Nodes are not configured for one role or another.
- Using Raft for this.
- Dead cluster members may be pruned after some period of time based on cluster configuration.

#### discovery
Will support a few discovery protocols. Allows members to automatically join as long as they can present needed credentials. AKA, cluster formation, peer discovery, auto clustering.

- `crate discovery_dns`: DNS based discovery.
- `crate discovery_consul`: Consul based discovery.
- `crate discovery_etcd`: Etcd based discovery.

### admin api
- Used to “ensure” PS or ES streams. If ensured configuration is different, this can be detected and updated. Maybe options to overwrite config & another to warn if config is different. No startup config specific to streams. Only general maintenance config &c.

### networking
- Railgun client <-> server communication takes place over multiplexed TCP keepalive connections (similar to AMQP style systems).
- Clients may use a single pipe for consumption as well as publication.
- Server <-> server clustering communication takes place over persistent TCP connections as well.
- Will probably use protobuf as the framing protocol.

### ack & nack
Messages being consumed from a stream must be ack'ed. At this point, no batch processing, just scale out the number of consumers in the group via concurrency model or horizontal scaling.

Messages may be nack'ed. By default, they will be immediately redelivered. A delay may be optionally specified which will block only the consumer or consumer group which sent the delay.

### cap
As far as CAP theorem, this system prioritizes *Availability* and *Partition Tolerance*. This works perfectly for this type of system, because for streams which are persistent, they are immutable. So consistency is purely a matter whether the replica node has the most recent additions to the stream. There is no MVCC to be concerned about, no atomic updating concerns or the like.

----

----

----

### id pools vs transactions

transactions pros:
- allows for atomically comitting messages to multiple streams simultaneously.
- works well when processing only involves reading from and writing to other streams within the cluster.

transactions cons:
- does not work well with systems outside of the cluster. Eg, writing to MongoDB or some other store.

id pool pros:
- can be used to accomplish the same thing as transactions within a cluster.
- works well with systems outside of the cluster. Eg, unique message IDs can be used to guarantee uniqueness across system boundaries. Consider the following:
    - a consumer process may need to receive a message, write information to MongoDB and then write a result to another stream.
    - transactions will not help us here as the MongoDB operations may fail in various ways. Or may succeed and thent he write to the final stream may fail.
    - id pools will work well in this situation. Original message will have an ID. MongoDB can use this with a unique index on an array field. Will use `$nin` on the query to ensure the ID hasn't already been used in a document for an update. Then will use a `$push` to add the ID to the unique array field atomically.
    - when dealing with retries, if the ID has already been used, you can safely skip it, and move on to the next phase of the workflow.

id pool cons:
- error handling patterns are needed to ensure idempotent transaction-like semantics. Eg, specific error variant will be returned when ID has already been used for the specific stream.
- clients must be using a correct implementation of UUID4 when ID pools are needed.

discussion:
- transactions can be used to simply populate multiple streams where each stream is used by one of the downstream (external to the cluster) systems. This reduces the complexity by making in operation a read-write-ack operation.
- the ack could still fail after the downstream system has been populated (duplicates).
- ordering can not be guaranteed with this pattern. The ordering in which downstream systems read and populate their stores could be out of sync (a consumer could be down) and this could cause invariants of the system to be violated.
