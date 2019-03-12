railgun
=======
A distributed streaming and messaging platform written in Rust.

The name is not final.

### Three Types of Streams
Everything is a stream.

- **Persistent (PS):** Messages are persistent. No topics. Optional ID time box for deduplication.
- **Ephemeral (ES):** AMQP exchange style stream. Topics are used for consumer matching.
- **Response (RS):** Similar to an ephemeral stream, except it is immediately removed after first message is published to it.

### PS Streams
- Provides **at least once delivery semantics** by default, or **exactly once delivery semantics** if using ID time boxes or properly functioning clients.
- Does not specify number of partitions. Ordering is determined by time of receipt. Streams are replicated heuristically to guard against data loss.
- Messages have no concept of “topics” on a PS.
- Messages must be ack’ed. Messages may be Nack’ed to indicate that redelivery is needed. Redelivery is immediate to some live consumer from same group. Automatic redelivery takes place when consumer dies and can not ack or nack message. 
- Support for optional time boxed unique message IDs. A message presented with an ID will have its ID checked against the time boxed IDs pool for that stream. If the ID was used within the time box, the message will be rejected. This provides a builtin mechanism for deduplication.
- Consumers may consume from streams independently, or as groups. Server will track offset of consumers automatically. Automatic DLQ on messages according to stream config when successful processing of the message falls a specific distance behind most recently consumed message.
- Consumer groups can be setup to be exclusive. This means that only one consumer from the group will receive messages at a time. This supports consumption ordering based on time message hit stream.
- Non-exclusive consumer groups will simply have messages load balanced across group members as messages arrive.

### ES Streams
- Provides at most once delivery semantics.
- Messages on an ephemeral stream may have “topics” (AMQP style). Defaulting to empty string. No ack or nack on ES streams.
- Consumers may specify a “topic” matcher. Defaulting to a match all wildcard. AMQP style matchers are supported. If no consumer matches the topic of the message, it will be dropped.
- Consumers may form groups, where messages will be load balanced across healthy group members. 
- Messages will be delivered once to each consuming entity by default. Entities are individual consumers or groups.
- Persistent stream consumers will receive all messages without any message level discrimination. May specify start points. Server keeps track of location of consumer by ID. Groups will share this consumer ID to coordination location in stream.

### RS Streams
- Messages published to an ES stream may include a “response” field, in which case a RS stream will be created matching the response field. Will error if already in use.
- Can not be subscribed to explicitly. The publisher will be automatically subscribed to the RS stream, and can be the only subscriber.
- A timeout may be optionally supplied when the message needing a response is initially published, else the default RS stream timeout config will be used. Clients do not need to manually timeout. The server will respond with the timeout error.

### Clustering
- Clustering is natively supported. Nodes internally maintain distinction and leadership elections. Configurations are the same no matter the node.
- Planning on using Raft for this.
- Will support modern clustering protocols. DNS srv record peer discovery. Allows members to automatically join as long as they can present needed credentials.

### Admin API
- Used to “ensure” PS or ES streams. If ensured configuration is different, this can be detected and updated. Maybe options to overwrite config & another to warn if config is different. No startup config specific to streams. Only general maintenance config &c.

### Networking
- Railgun client <-> server communication takes place over multiplexed TCP keepalive connections (similar to AMQP style systems).
- Clients may use a single pipe for consumption as well as publication.
- Server <-> server clustering communication takes place over persistent TCP connections as well.
- Will probably use protobuf as the framing protocol.

### Central Thesis
Older AMQP style systems were great and have some advantages, but fall short of Kafka style streaming capabilities, specifically message persistence. More recent blends of the technologies, like Nats, offer a nice blend, but lack many of the core features needed from both domains.

Rust is an excellent language to use for writing a system such as this. We will build upon the tokio runtime for networking. Will use Rusts async/await system for readability and approachability of the system (hopefully will have more contributors this way). Will leverage message passing patterns within the system to completely avoid locks & leverage Rust’s ownership system to its maximum potential.
