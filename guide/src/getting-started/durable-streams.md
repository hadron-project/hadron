Durable Streams
===============
Durable streams are append-only, immutable logs of data.

- Stream names may be 1-100 characters long, containing only `[-_.a-zA-Z0-9]`. The `.` can be used to form hierarchies for authorization matching wildcards.
- Streams provide base-line **at least once delivery semantics** by default. **Exactly once delivery semantics** will always depend upon clients operating properly; however, the [atomic ack + publish](#atomic-ack-+-publish) feature provides consumers a greater idempotence guarantee.
- Stream data is replicated accross the entire cluster via the Raft consensus protocol which guarantees strict linearizability.
- Messages must be ack’ed. Once a message is ack'd, it will not be re-delivered again to the same consumer group.
- Messages may be nack’ed to indicate that redelivery is needed. Automatic redelivery takes place when the consumer dies and can neither ack nor nack a message. See the [subscriptions section](#subscriptions) below for more details.
- Consumers may consume from streams independently, or as groups, with durable offset tracking. Railgun will track offsets of consumers automatically. See the [subscriptions section](#subscriptions) for more details.
- Group consumers will simply have messages load balanced across group members as messages arrive. Group membership is based purely on the `name` specified when a subscription is created.
- Consumers will receive all messages without any message level discrimination. When subscriptions are created, they may specify start points: first, latest, or specific offset. The starting point only effects the creation of the consumer group when a subscription is created.
- Streams should be `ensured` first to ensure that it exists with the expected configuration.

### stream creation
Streams are created in code via the client `EnsureStream` request. Publisher and subscriber clients should both ensure that a stream exists before attempting to use it.

### stream deletion
Streams can only be deleted via the `rgctl` CLI.

### subscriptions
Subscriptions represent a client's interest to consume data from a stream. Subscriptions are created when a client submits a subscription request. Subscription requests must specify the target stream to subscribe to and a name to use for the subscription. When multiple clients subscribe to the same stream using the same subscription name, this will dynamically form a consumer group and messages will be load-balanced across all members of the group.

Subscriptions are durable in the sense that the groups progress through consuming the data of the target stream goes through Raft and is stored on disk. Clients may disconnect at any time, and they will be automatically removed from the consumer group, and new consumers may be added to the group just as easily.

Subscriptions may be deleted using the `rgctl` CLI.

### ack & nack
Messages being consumed from a stream must be ack'ed. When a subscription is created, the maximum number of in-flight unacknowledged messages is configurable (which will apply to the entire subscriptions consumer group).

Messages may be nack'ed, which will cause immediate redelivery by default. A redelivery timeout may be specified, which will cause a timeout to be applied before redelivery of the message to a consumer. Redelivery timeouts are not durable, and are held in-memory in the cluster leader only. If a leader goes offline, when the new leader comes to power, it will immediately begin delivering messages to connected consumers without knowledge of any timeouts. Leader failure is rare for properly configured clusters.

If a client disconnects while it was processing unacknowledged messages, Railgun will redeliver those messages to other live consumers of the same subscription consumer group. Automatic redelivery only takes place based on Nacks and client disconnects.

#### atomic ack + publish
Clients may transactionally ack a stream message & publish a set of additional messages to other streams all in one atomic operation.

### exactly-once semantics
While Railgun does guarantee that ack'ed messages are never redelivered, it is important to understand that exactly-once stream processing is not technically possible to guarantee 100% of the time given our current understanding of physics (non-quantum physics expert, circa 2019 CE). Instead, idempotent logic must be used by the consumer to provide *exactly-once semantics* for stream processing. A simple example of this difficulty is that the consumer process might lose power just before ack'ing the message it was processing. Check out the chapter in this guide on the [WAFL Protocol](../wafl-protocol.md) for a way to deal with this and related issues.

Railgun provides powerful guarantees for building systems with exactly-once semantics. Consumer processes must still be idempotent, but can use the atomic ack + publish feature to guarantee that data written to output streams is not duplicated under retry scenarios, as the output of the stream processing function can be delivered along with the ack of the message, and once a message is successfully ack'ed by a consumer, Railgun guarantees that the message will not be redelivered.
