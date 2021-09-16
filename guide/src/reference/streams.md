These docs are currently under construction.
<!-- Streams
=======
Streams are append-only, immutable logs of data.

## Schema
Streams are declared in YAML as part of the [Schema Management system](./schema.md). The schema for the `Stream` object is as follows:

```yaml
## The kind of object being defined. In this case, a stream.
kind: Stream
## The namespace in which this stream is to be created.
namespace: required string
## The name of the stream. Each stream must have a unique name per namespace.
name: required string

## The cluster replica sets which are to be used as partitions for this stream.
partitions: required string

## An optional TTL duration specifying how long records are to be kept on
## the stream.
##
## If not specified, then records will stay on the stream forever.
ttl: optional duration (default none)
```

Streams can be updated the same way all other Hadron DDL objects can be updated. See the [Schema Management chapter](./schema.md) for more details.

### Details
- Stream names may be 1-100 characters long, containing only `[-_.a-zA-Z0-9]`. The `.` can be used to form hierarchies for authorization matching wildcards.
- Streams may have one or more partitions, which may be increased as needed. Horizontally scaling the Hadron cluster and adding more partitions to a stream will allow the write throughput of the stream to horizontally scale.
- Stream data is replicated across each partition's replica set.
- Streams must first be created via the Hadron schema system. See the [Schema Management](./schema.md) chapter for more details.

## Subscriptions
Subscriptions represent a client's interest to consume data from a stream. Subscriptions are created when a client submits a subscription request. Subscription requests must specify the target stream to subscribe to and a name to use for the subscription.

**Consumers & groups:** clients which process records from a stream as part of a subscription are known as consumers. When multiple clients subscribe to the same stream using the same subscription name, this will dynamically form a consumer group and messages will be load-balanced across all members of the group. Consumers will receive all messages without any message level discrimination.

**Durable:** subscriptions are durable in the sense that the group's progress through the stream is stored on disk. Clients may disconnect at any time, and they will be automatically removed from the consumer group, and new consumers may be added to the group just as easily.

**Starting point:** when subscriptions are created, they may specify a starting point: first or latest. The starting point only affects the creation of the consumer group when a subscription is created.

**Subscription Config:** when creating a subscription, there are various configuration options which my be specified in order to control behavior.
- `maxParallelConsumers` (default none): the maximum number of consumers allowed to process messages in parallel. If set to 1, then only one consumer is allowed to process messages at a time for the consumer group.

**Consumer Config:** each individual consumer within a group has its own isolated configuration options.
- `batchSize` (default 100): the maximum number of messages which will be delivered to the consumer per batch.
- `batchWaitMillis` (default 500): the amount of time in milliseconds which the consumer should delay for its batch to fill. This only applies when the consumer's batch has been partially filled.

Subscriptions may be deleted using the `StreamUnsub` client API.

### Ack & Nack
Messages being consumed from a stream must be ack'ed. Once a message is ack'd, it will not be delivered again to the same consumer group.

Messages may be nack'ed, which will cause immediate redelivery by default. A redelivery timeout may be specified, which will cause a timeout to be applied before redelivery of the message to a consumer. Redelivery timeouts are not durable, and are held in-memory by Hadron.

If a client disconnects while it was processing unacknowledged messages, Hadron will redeliver those messages to other live consumers of the same subscription consumer group. -->
