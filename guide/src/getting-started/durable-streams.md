Durable Streams
===============
Durable streams are append-only, immutable logs of data.

- Stream names may be 1-100 characters long, containing only `[-_.a-zA-Z0-9]`. The `.` can be used to form hierarchies for authorization matching wildcards. Consumers do not use wildcards for streams.
- Provides **at least once delivery semantics** by default, or **exactly once delivery semantics** if writing to a stream with [unique id checking](#unique-id-checking) enabled.
- Stream data is replicated accross the entire cluster via the Raft consensus protocol which guarantees strict linearizability.
- Messages must be ack’ed. Messages may be nack’ed to indicate that redelivery is needed. Redelivery may be immediate or may have a delay applied to the message (for the consumer group only). Automatic redelivery takes place when the consumer dies and can not ack or nack amessage. Redelivery delays are tied directly to the lifetime of consumers and their durability. See the [consumers section](#consumers) below for more details.
- Support for optional message ID uniqueness. If a stream is configured with message uniquness, all messages will have their IDs checked against the indexed IDs for that stream. Duplicate IDs will be rejected. This provides a built-in mechanism for deduplication and exactly-once semantics.
- Consumers may consume from streams independently, or as groups, with durable offset tracking or ephemeral offsets. Server will track offset of consumers automatically. See the [consumers section](#consumers) for more details.
- Group consumers will simply have messages load balanced across group members as messages arrive. Group membership is based purely off of a simple ID provided when a consumer creates its subscription.
- Consumers will receive all messages without any message level discrimination. Consumers may specify start points: first, latest, or specific offset. If consumer already has a tracked offset based on its consumer ID, then any specified starting point will be ignored. Railgun keeps track of the location of a consumer by ID and offset.
- Streams should be `ensured` first to ensure that it exists with the expected configuration.

### unique id checking
If a stream is configured to check message ID uniqueness, every message published to the stream must include an ID and it will have its ID checked. The overall protocol is quite simple for unique message ID checking. An index of all message IDs is held in memory for the stream. Before the message is written to the stream, the ID will be checked against the index to ensure it doesn't already exist. When the stream is created with ID checking enabled, it must specify a behavior for dealing with conflicts: `fail` or `succeed`. For the `fail` setting, the server will return an error. For the `succeed` setting, the server will return a success response without actually mutating the stream (essentially a no-op).

### stream creation
Streams are created in code via the client `EnsureStream` request. Publisher and subscriber clients should both ensure that a stream exists before attempting to use it. During the lifetime of a stream, some of its configuration parameters may need to be changed. To ensure that conflicts do not arise, a version number must be provided in the `EnsureStream` call. If a call is received which has an older version number, the call will succeed without any changes being made to the stream. If a newer version is received, the requested changes will be applied to the stream.

- Changing a stream from a normal stream to a unique ID checking stream, or vice-versa, is considered and error. Such attempts will return errors.
- A stream's durability settings may be changed any time.

### stream deletion
Streams can only be deleted via the `rgctl` CLI.

### consumers
Streams support ephemeral and durable subscriptions, and both types of subscriptions may form groups. All stream consumers MUST specify a queue group name, and groups are formed when consumers share the same queue group name. Durable subscriptions will have their offsets persisted to disk and will be able to resume from their last recorded offset within a stream even if all consumers have gone offline. Subscriptions are made durable via a boolean value in the subscription request, and once a subscription is durable, it is always durable.

Durable subscriptions which are no longer being used can be deleted via the `rgctl` CLI.

### ack & nack
Messages being consumed from a stream must be ack'ed. When a subscription is created, the maximum number of in-flight unacknowledged messages is configurable (which will apply to the entire group, if part of a group consumer). This provides batch processing or serial processing as needed.

Messages may be nack'ed. By default, they will be immediately redelivered. A delay may be optionally specified which will block only the consumer or consumer group which sent the delay.

#### atomic ack + publish
Railgun clients may transactionally ack a stream message & publish a set of additional messages to other streams all in one atomic operation.

Each message being published along with the ack must specify how the server is to handle unique ID violations. If none of the messages being published are bound for a stream which performs unique ID checking, then this will have no effect and the operation will always succeed. For messages which are bound for a unique ID checking stream, the server can be instructed to abort the entire operation on conflicts, or it can be innstructed to reckon the conflict as a success.
