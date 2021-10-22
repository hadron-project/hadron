What is Transactional Processing
================================

<small><i>This use case assumes some familiarity with Hadron. It is recommended to have at least read the [Quick Start](../overview/quick-start.md) chapter before continuing here. Reading the previous chapter [Use Case: Service Provisioning](./service-provisioning.md) is a great primer as well.</i></small>

Transactional processing is an algorithmic pattern used to ensure that event processing is [idempotent](https://en.wikipedia.org/wiki/Idempotence).

In the greater context of system design and operations, errors take place all the time. This can lead to race conditions, duplicate events, invalid states, you name it. How do we deal with these issues?

In some cases, none of these issues matter. Maybe we don't care about the data, maybe we don't retry in the face of errors. However, when attempting to build a consistent system (either strong or eventual), then we need to care about these issues. Deeply.

**Transactional processing is the way to deal with these issues.** Before we delve more deeply into implementing a transactional processing pattern, we need to understand identity.

### Identity
An event must be uniquely identifiable.

In Hadron, this is easy, as we use the CloudEvents model, and each event bears the `id` and `source` fields which together uniquely identify an event. Producers provide the `id` and `source` fields when publishing events, the only trick is in determining what the values for these fields should be.

#### Establishing Identity
Depending on the requirements of the system being built, there are a few different ways to establish the identity of an event before publishing it to Hadron.

**OutTableIdentity:** this approach uses a transactional database system to generate events as part of the database transaction in which the event transpired, writing those events first to a database table, called the out-table, and then using a separate transaction to copy or move those events from the out-table into Hadron.
- Most transactional databases provide [ACID guarantees](https://en.wikipedia.org/wiki/ACID) around this process (it is just a regular transaction).
- This means that the generation of the event is "all or nothing". If the transaction is rolled-back before it is committed, then it is as though it never happened.
- The event should have a unique ID enforced by the database as it is written to the out-table and which will map directly to the Hadron CloudEvents `id` field.
- The event should also have a well established value for `source`, which will often correspond to the application's name, the out-table's name, or some other value which uniquely identifies the entity which generated the event.
- The process of moving events from the out-table into Hadron could still result in duplicate events, however the ID of the event from the out-table will be preserved and firmly establishes its identity.

**OccurrenceIdentity:** this approach establishes an identity the moment the event occurs, typically using something like a UUID. The generated ID is embedded within the event when it is published to Hadron, and even though the publication could be duplicated due to retries, the generated ID firmly establishes its identity.
- The generated ID will directly map to the Hadron CloudEvents `id` field of the event when published.
- The event should also have a well established value for `source`, which will often correspond to the application's name or some other value which uniquely identifies the entity which generated the event.
- This approach works well for systems which need to optimize for rapid ingest, and as such typically treat the occurrence itself as authoritative and will often not subject the event to dynamic stateful validation (which is what out-tables are good for).

These two patterns for establishing the identity of an event generalize quite well, and can be applied to practically any use case depending on its requirements.

In all of these cases, the `id` and `source` fields of the CloudEvents model together establish an absolute identity for the event. This identity is key in implementing a transactional processing model which is impervious to race conditions, duplicate events and the like.

### Hadron is Transactional
Hadron itself is a transactional system.

For Streams, transactions apply only to `ack`'ing that an event or event batch was processed. For Pipeline stages, `ack`'ing an event requires an output event which is transactionally recorded as part of the `ack`.

For both Stream and Pipeline consumers, once an event or event batch has been `ack`'ed, those events will never be processed again by the same consumer group. However, it is important to note that Hadron does not use the aforementioned CloudEvents identity for its transactions. Hadron Stream partitions use a different mechanism to track processing, and users of Hadron should never have to worry about that.

For users of Hadron, processing should only ever take into account the `id` and `source` fields of an event to establish identity.
