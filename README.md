railgun
=======
A distributed streaming and messaging platform written in Rust.

## overview | central thesis
Older AMQP style systems were great and have some advantages, but fall short of Kafka style streaming capabilities, specifically message persistence. More recent blends of the technologies, like Nats, offer a nice blend, but lack many of the core features needed from both domains.

Railgun provides a general purpose ephemeral message passing system, with request/response messaging, pub/sub, fanout, queueing, and group consumer load balancing capabilities. Railgun also provides persistent message streaming with configurable durability. Persistent streams are preserved indefinitely by default and may be consumed by individual or group consumers, with consumer offsets and load balancing handled natively by the Railgun system. Persistent streams may also be configured to support unique message ID checking which provides built-in "exactly once" semantics for stream processing, regardless of client misbehavior.

Rust is an excellent language to use for writing a system such as this, arguably the best currently available. This system is built upon the [Actix](https://actix.rs) actor framework, which in turn is built upon the [Tokio](https://tokio.rs/) asynchronous runtime. Leveraging the actor pattern with message passing allows us to completely avoid locking in the Railgun application code, leverage Rust’s ownership system to its maximum potential, and the asynchrounous foundation keeps resource usage minimal along with extremely high throughput.

Railgun also features a very simple wire protocol based on protocol buffers. Protobuf has more and more become a universal serialization language for data interchange. This choice was made to reduce the barrier of entry for building Railgun client libraries in various languages.

## persistent streams
Messages published to a persistent stream are durable. The durability may be configured in various ways. Persistent streams have no topics. They may optionally perform unique message ID checking for server enforced "exactly once" semantics.

- Persistent stream messages have no concept of “topics”. The stream itself can be named hierarchically, but wildcards can not be applied to streams for consumption. Stream names must conform to the pattern: `[-_A-Za-z0-9.]+`.
- Provides **at least once delivery semantics** by default, or **exactly once delivery semantics** if writing to a stream with [unique id checking](#persistent-stream-unique-id-checking) enabled.
- Stream replication can be configured when the stream is created. By default, the stream will be replicated to 3 peer writer nodes, but the number of replicas can be any non-zero unsigned 8-bit integer.
- Messages must be ack’ed. Messages may be nack’ed to indicate that redelivery is needed. Redelivery may be immediate or may have a delay applied to the message (for the consumer group only). Automatic redelivery takes place when consumer dies and can not ack or nack message. Redelivery delays are tied directly to the lifetime of consumers and their durability. See the [consumer section](#consumers) for more details.
- Support for optional message ID uniqueness. If a stream is configured with message uniquness, all messages will have their IDs checked against the indexed IDs for that stream. Duplicate IDs will be rejected. This provides a builtin mechanism for deduplication.
- Consumers may consume from streams independently, or as groups. With persistent offset tracking or ephemeral offsets. Server will track offset of consumers automatically. See the [consumers section](#consumers) for more details.
- Group consumers will simply have messages load balanced across group members as messages arrive. Group membership is based purely off of a simple string ID provided when a consumer first connects. The writer delegate is responsible for load balancing decisions.
- Consumers will receive all messages without any message level discrimination. Consumers may specify start points: start, end, or specific offset. If consumer already has a tracked offset, then start or end starting points will be ignored. Server keeps track of location of consumer by ID and offset.
- Stream should be `ensured` first to ensure that it exists with the expected configuration.

## ephemeral messaging
The entire cluster acts as an AMQP-style exchange. Topics are used for consumer matching. Messages are never preserved, they are either immediately routed to matching consumers, or the message is dropped if there are no matching consumers. Consumer group load balancing decisions are made by the source node which received the message needing to be load balanced. Consumer group information is synchronously replicated to all writer nodes (described below) when the consumer group is formed and as members join and leave the group, but this information is only held in memory.

Request/Response capabilities are implemented using the same message routing infrastrucuture, but use different semantics for how request and response messages are sent through the system. This is enforced by the wire protocol. Only the publisher of the request can receive the response.

- Provides **at most once delivery semantics.**
- Ephemeral messages may have “topics”, similar to AMQP-style topics. Defaults to an empty string. No ack or nack used for ephemeral messages.
- Consumers may specify a “topic” matcher. Defaulting to an empty string which would only match messages published with an empty string as the topic. Wildcard topic matchers are supported, similar to AMQP-style wildcards. If no consumer matches the topic of the message, it will be dropped.
- Consumers may form groups, where messages will be load balanced across healthy group members.
- Messages will be delivered once to each consuming entity by default. Entities are individual consumers or groups.

### topics
Railgun enforces that message topics adhere to the following pattern `[-_A-Za-z0-9.]*`. In English, this could be read as "all alpha-numeric characters, hyphen, underscore and period". Topics are case-sensitive and can not contain whitespace.

##### topic hierarchies
TODO: update the content of the next four headings.

The `.` character is used to create a subject hierarchy. For example, a world clock application might define the following to logically group related subjects:

```
time.us
time.us.east
time.us.east.atlanta
time.eu.east
time.us.east.warsaw
```

### wildcard matchers
Subscribers may use two wildcards for matching message topics. Subscribers can use these wildcards to listen to multiple topics with a single subscription but Publishers will always use a fully specified subject, without the wildcard (as the wildcard characters are not valid topic characters).

##### single token matching
The first wildcard is `*` which will match a single hierarchy token. For example, if an application wanted to listen for eastern time zones, they could subscribe to `time.*.east`, which would match `time.us.east` and `time.eu.east`.

##### multiple token matching
The second wildcard is `>` which will match one or more hierachy tokens, and can only appear at the end of the topic. For example, `time.us.>` will match `time.us.east` and `time.us.east.atlanta`, while `time.us.*` would only match `time.us.east` since it can’t match more than one hierarchy token.

--------------------------------------------------------------------------------------------------

## cap
In respect to CAP theorem, this system prioritizes *Availability* and *Partition Tolerance*. This works perfectly for this type of system as persistent streams are immutable. So consistency is purely a matter whether the replica node has the most recent additions to the stream. There is no MVCC to be concerned about, no atomic updating concerns or the like as transactions are not currently on the roadmap and don't really apply to a system like this, especially considering the [persistent stream unique id checking feature](#persistent-stream-unique-id-checking).

## clustering
- Clustering is natively supported. Cluster roles are dynamic.
- Raft is used for cluster consensus.
- Nodes may function in a few different roles: `master`, `writer`, `reader`. Nodes are master eligible (`writer`) by default, but can be configured to be read-only (`reader`) so that they can not become the cluster master.
- Dead cluster members may be pruned after some period of time based on cluster configuration.

### discovery
When a node first comes online, the discovery system will be booted to monitor for peers. The raft protocol is used to establish a cluster master. Once a master has been elected, all other nodes will treat it as the source of truth for selecting stream writer delegates.

### node lifecycle
Nodes may be in a few differnt states during their lifecycle.
- `syncing`: a node has come online for the first time or after some period of time, and it needs to sync with the most recent state of the system.
- `active`: a node is live and active. Replicating data, potentially operating as a writer delegate and potentially operating as the master.

##### syncing
A node always initially comes online in the syncing state. When a node is syncing, it will still participate in the cluster consensus protocol. Overview of the syncing lifecycle:
- The node will request cluster metadata from the master to determine if it is considered new to the cluster.
- If the node is new to the cluster, the master will determine the streams which it should replicate, and will then propose this as part of the cluster state. Once the new node receives this information, it will begin requesting snapshots and the like from the various stream writer delegates.
- Once the new node has come up to speed on all of the streams which it needs to replicate, it will notify the master that it is ready to enter the active state. Once it receives confirmation of the state change, it will update itself internally.

##### active
Once a node is active, it will participate in all standard lifecycle events of the cluster, and if the node is a writer node, it will be eligible for master election.

### writing data
This applies primarily to persistent streams. This section introduces the term `writer delegate`. A writer delegate is a node, which must at least be a writer, which is responsible for handling all write operations on a stream.
- When a stream is first created, the cluster master will elect a writer node to act as the writer delegate for the new stream.
- The cluster master will ensure that there is always a live writer delegate for every stream. A single node may function as the writer delegate for multiple streams.
- The master will broadcast a writer delegate removal to all nodes (and wait for all writers to respond) before broadcasting the new writer delegate for a stream. This should typically be faster than a standard cluster election process if raft were being used instead of writer delegates.
- When a new stream is created, it will get an initial ID starting point for the stream. As data replication events are broadcast, the payload will also include the ID used for the message so that IDs will always be monotinically increasing, which guarantees strict ordering within the database. If a writer delegate goes down and a new writer delegate is elected, it will be able to begin generating new IDs for the target stream seamlessly.

##### data organization
- Within the DB, each stream receives a top-level keyspace based on the name. Stream names must be simple, matching the character set `[-_A-Za-z0-9.]+`.
- The stream's concrete data is stored under the keyspace as follows `/streams/{streamName}/data/{id}`. This allows for easily watching a keyspace for consumer operations.
- The stream's consumer offsets are stored under the keyspace as follows `/streams/{streamName}/offsets/{consumerId}`. This allows for easily updating the offsets of consumers by consumer ID.
- This system does not use oplogs as there is only one type of operation which can be applied to persistent streams. Replication is synchronous to all other writers nodes, so writer delegate changes will normally not require the master sync.
- The cluster's membership, node state, and writer delegates are stored in the database under `/cluster/metadata/`. Each change gets an ID just as a standard persistent stream. This data is synchronously replicated to all writer nodes. Reader nodes receive the data at the same time, but the operation will be considered complete after writer nodes are synced.

### data replication
Data replication for a persistent stream is influenced by the creation of the stream. When a stream is created, it may be created with 3 node replication (which is the default), but may be configured to have a different number of replicas. The number of replicas specified must be a valid non-zero unsigned 8-bit integer. If zero is specified, 1 the system will default to 1 replica. Clients should not validate the value, but should allow the server to perform validation.

The cluster master is responsible for the placement of stream replicas and nominating which nodes will replicate which streams. The algorith which will be used for making this decsion will quite likely just be based on the message count of the streams. Streams with larger numbers of messages will be spread out evenly as a simple heuristic for placement.

Read only servers will replicate all data of all streams, but typically replication confirmation from these nodes will not be required during write operations.

#### stream sharding
For now, configurable stream replicas seems to be sufficient. Let's stay tentative on this and see what sort of usecases emerge.

### discovery
Discovery is what allows members to automatically join a cluster by way of network communication. This is often referred to as, cluster formation, peer discovery, auto clustering. A Railgun cluster can be configured to check credentials of peers during cluster formation.

Currently only DNS based peer discovery is implemented in this system. However, Railgun has been designed so that new discovery backends can be easily added in the future as needed.

### networking
- Railgun client <-> server communication takes place over WebSockets, which allows for multiplexed communication channels by default.
- Clients may use a single pipe for consumption as well as publication.
- Server <-> server clustering communication takes place over WebSockets as well.
- Will probably use protobuf as the framing protocol, maybe capnproto. This is still TBD. TODO: get this nailed down.
- Nodes within a cluster may forward commands between each other as needed.

### ack & nack
Messages being consumed from a stream must be ack'ed. At this point, no batch processing patterns are planned, just scale out the number of consumers in the group via concurrency model for horizontal scaling.

Messages may be nack'ed. By default, they will be immediately redelivered. A delay may be optionally specified which will block only the consumer or consumer group which sent the delay.

--------------------------------------------------------------------------------------------------

## feature details
### persistent stream unique id checking
If a stream is configured to check message ID uniqueness, every message published to the stream must include an ID and it will have its ID checked. The write operation will be replicated to all nodes responsible for replicating the stream before a response is returned by the server.

The overall protocol is quite simple for unique message ID checking. An index of all message IDs is held in memory for the stream. Before the message is written to the stream, the ID will be checked against the index to ensure it doesn't already exist. The message will then be committed to persistent storage then to the index.

If the server detects that the message ID is a duplicate, it will respond to the client with a specific error code describing the issue.

--------------------------------------------------------------------------------------------------

### consumers
#### ephemeral message consumption
Ephemeral messaging supports two types of consumers: individual and group consumers.
- Individual consumers will operate in a standard pub/sub fashion where every subscriber will receive all messages which its subscription pattern matches.
- Group consumers will operate in a load balancing fashion, where only one member of the group will receive any specific message. Consumers outside of the group may still receive the message.

When a client connects to the cluster to begin consuming messages, its topic matching pattern along with other metadata is broadcast to all nodes in the cluster.
- If the information is not ack'd by all peers, retries will be made.
- An error will be returned to the client if the data can not successfully be sent to all peers.
- Each node maintains info on all connected clients throughout the cluster and will push messages to matching connected clients. This data is held in memory only.
- When a node receives a publication, it will pump the message out to any peers which have an active consumer which matches the message's topic.
- Ephemeral message consumers may form groups by presenting an ID. When groups are formed, group membership metadata is broadcast to all nodes just like normal consumer connections.
- When a node receives a publication which would match a consumer group, the node must randomly choose a single client to send the message to for load-balancing.
- When a request message is published, a unique response topic will be generated which clients can not publish to normally (a response message must be used) and the publishing client will be automatically subscribed to the generated topic. The message payload will be broadcast in a unique payload which also includes the response topic client subscription data. This allows for efficiently setting up routing for the response, as well as accomplishing delivery of the message to any consumers.
- When a response message is published, it will be sent to the node which the original requesting client is connected to. All other nodes will receive a background broadcast to remove the request topic/consumer matcher data.

#### persistent stream consumption
Persistent streams support ephemeral and durable subscriptions, and both types of subscriptions may form groups. Subscription durability is purely a matter of whether the consumer's stream offsets are tracked.
- Ephemeral individual consumers will not have their offsets tracked.
- Ephemeral group consumers will not have their offsets tracked, but load balancing will be used.
- Durable individual consumers will have their offsets tracked.
- Durable group consumers will have their offsets tracked, and load balancing will be used.

Both ephemeral and durable consumer groups are created when multiple consumers present the same consumer ID for the same target stream. If no consumer ID is presented then the consumer can not be durable, and a random unique consumer ID will be generated for the subscription.

###### connectin
When a client connects to the cluster to begin consuming from a persistent stream, its target stream and other metadata is broadcast to all writer nodes in the cluster which are responsible for replicating the stream.
- If the information is not ack'd by all writer peers responsible for replicating the target stream, retries will be made.
- An error will be returned to the client if the data can not successfully be sent to the target peers.
- As writes to a persistent stream must go through the writer delegate node, all message delivery is handled by the writer delegate, including load balancing decisions.

###### consumption
TODO: work with sled folks on determining what the LOE would be for async keyspace watching, as this is likely to be a big bottleneck for consumer processes ... unless newly written messages are pushed to open cursors as part of the data writing process.

- The consumption process is handled by the writer delegate for the purpose of tracking offsets. The system will use cursors over the stream's keyspace so that batches of messages may be read from disk, but the offsets will only be committed as messages are ack'd by the target consumer.
- The target client/node combination to which a message from the cursor must be routed to is determined by the the router. The message is sent to the router along with metadata on the stream it came from. The router will then make load balancing decisions and other message routing decisions.
- The offsets of consumers is broadcast to all writer nodes responsible for the specific streams replication. Durable consumer offsets are written to disk, while ephemeral consumer offsets are only held in memory.
- Messages on a cursor which are nack'd by a consumer will be held in memory for immediate reference and decision making, this info is broadcast to all writing peers responsible for the target stream. A timeout will be spawned on the writer for when it should be redelivered. A writer which is selected to be a new writer delegate must spawn timeouts for any currently nack'd messages on cursors.
- Consuming from an ID-checked stream behaves the same way as other persistent streams.

--------------------------------------------------------------------------------------------------

### primitives
##### node
A node is an individual process instance. Usually running in a container, VM or the like.

##### node configuration
The static configuration for a node when it is started. The exact pattern is still a bit up in the air now. Probably env based and file based with env as overwrites.

##### cluster
A grouping of nodes working together. Clustering is natively supported by this system, with DNS based discovery and cluster already available, and a few other discovery backends planned.

##### message
A message is a structured blob of data inbound to or outbound from a node in the system.

##### consumer
A consumer is a process which is connected to the cluster and is configured to receive messages from some set of streams and or topics in the cluster. Consumers may form groups.

##### dlq
Dead letter queue. This is a longstanding paradigm which represents a resting place for messages which simply can not be successfully processed for some reason. Users may create DLQ streams for particularly troublesom messages. No Railgun specific paradigm here. Use your own DLQ pattern based on message redelivery count and message staleness tracking wich are parts of every message delivered to clients. Redelivery information is only present on messages which have been redelivered.

----

## LICENSE
Unless otherwise noted, the Railgun source files are distributed under the Apache Version 2.0 license found in the LICENSE file.
