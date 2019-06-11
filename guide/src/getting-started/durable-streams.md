Durable Streams
===============
Durable streams are append-only, immutable logs of data.

- Durable stream messages have no concept of “topics”. The stream itself can be named hierarchically, but wildcards have no meaning to consumers. Stream names must conform to the pattern: `[-_A-Za-z0-9.]+`.
- Provides **at least once delivery semantics** by default, or **exactly once delivery semantics** if writing to a stream with [unique id checking](#unique-id-checking) enabled.
- Stream data is replicated accross the entire cluster via the Raft consensus protocol which guarantees strict linearizability.
- Messages must be ack’ed. Messages may be nack’ed to indicate that redelivery is needed. Redelivery may be immediate or may have a delay applied to the message (for the consumer group only). Automatic redelivery takes place when the consumer dies and can not ack or nack amessage. Redelivery delays are tied directly to the lifetime of consumers and their durability. See the [consumers section](#consumers) below for more details.
- Support for optional message ID uniqueness. If a stream is configured with message uniquness, all messages will have their IDs checked against the indexed IDs for that stream. Duplicate IDs will be rejected. This provides a built-in mechanism for deduplication and exactly-once semantics.
- Consumers may consume from streams independently, or as groups, with durable offset tracking or ephemeral offsets. Server will track offset of consumers automatically. See the [consumers section](#consumers) for more details.
- Group consumers will simply have messages load balanced across group members as messages arrive. Group membership is based purely off of a simple ID provided when a consumer creates its subscription.
- Consumers will receive all messages without any message level discrimination. Consumers may specify start points: first, latest, or specific offset. If consumer already has a tracked offset based on its consumer ID, then any specified starting point will be ignored. Railgun keeps track of the location of a consumer by ID and offset.
- Streams should be `ensured` first to ensure that it exists with the expected configuration.

### unique id checking
If a stream is configured to check message ID uniqueness, every message published to the stream must include an ID and it will have its ID checked. The overall protocol is quite simple for unique message ID checking. An index of all message IDs is held in memory for the stream. Before the message is written to the stream, the ID will be checked against the index to ensure it doesn't already exist. If the server detects that the message ID is a duplicate, it will respond to the client with a specific error code describing the issue.

### consumers
Durable streams support ephemeral and durable subscriptions, and both types of subscriptions may form groups. Subscription durability is purely a matter of whether the consumer's stream offsets are tracked.

- Ephemeral individual consumers will not have their offsets tracked.
- Ephemeral group consumers will not have their offsets tracked, but load balancing will be used.
- Durable individual consumers will have their offsets tracked.
- Durable group consumers will have their offsets tracked, and load balancing will be used.

Both ephemeral and durable consumer groups are created when multiple consumers present the same consumer ID for the same target stream. If no consumer ID is presented then the consumer can not be durable, and a random unique consumer ID will be generated for the subscription.

### ack & nack
Messages being consumed from a stream must be ack'ed. When a subscription is created, the maximum number of in-flight unacknowledged messages is configurable (which will apply to the entire group, if part of a group consumer). This provides batch processing or serial processing as needed.

Messages may be nack'ed. By default, they will be immediately redelivered. A delay may be optionally specified which will block only the consumer or consumer group which sent the delay.

#### atomic ack + publish
Railgun clients may transactionally ack a stream message & publish a set of additional messages to other streams all in one atomic operation.

Each message being published along with the ack must specify how the server is to handle unique ID violations. If none of the messages being published are bound for a stream which performs unique ID checking, then this will have no effect and the operation will always succeed. For messages which are bound for a unique ID checking stream, the server can be instructed to abort the entire operation on conflicts, or it can be innstructed to reckon the conflict as a success.
