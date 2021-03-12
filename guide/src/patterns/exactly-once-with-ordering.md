Exactly Once Events with Ordering
=================================
Unicorn: we want "exactly once" event production and consumption along with strict ordering.

There are a lot of technical issues with such a request. Misunderstandings of the concepts and how systems interact. However, as it turns out, we can get pretty damn close to this when needed.

---

TODO: finish this up. Lots of raw thoughts here.

- PG could have missing seq IDs for the event table. This could be natural and the missing IDs may never be backfilled as they were from rolledback transactions or the like. No problem with that, as such is standard practice for PG.
- However, there may also be cases where small gaps or even individual IDs in the sequence are simply taking a longer time to generate before commit, and other events with later IDs may be committed first and sent to the Hadron OutTable stream, which results in out of order delivery.

This is a probelm for:
- replication: as we can no longer use the ID of the given event as the true replication LSN. For standard streams the current offset can always be used as the LSN and the absolute ID and strongly-ordered sequence number of each entry on a stream per partition.
- consumers: as we have no way of delivering old entries to consumers if they have already moved beyond such an out of order point in the stream.

----

Probably update the entry record to be just a payload of bytes with a key and an offset. Offset is never provided by consumer, it is only assigned by the server and is a u64. Key can be any string value which is used for compaction within a stream partition.

**IT WOULD SEEM** that there is no value in attempting to add an "OutTable" type stream, as out-of-order delivery of messages can still take place, the uniqueness of a key withing a stream partition is already guarantied by using a single partition stream where the key is the value of the out table's ID seq column (which is basically exactly what an OutTable stream is). So we should simplify and remove the "OutTable" concept.

The only way to truely have EXACTLY ordered event processing is by:
- having only one event producer.
- having only one event consumer.

This is true regardless of the underlying technology. As such, there is no value in having a dedicated "OutTable" stream. The producer must behave correctly, and the consumer must behave correct and in an ACID/transactional manner.

This problem can be "scaled" by simply partitioning/sharding the ordering problem down into smaller pieces. Per object ID in the subject domain, by associating a mutation or event sequence per object. Then consumers can detect if they've received an out of order event per object by checking the transactional system.
