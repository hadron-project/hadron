Streams
=======
Streams are append-only, immutable logs of data with absolute ordering per partition.

Streams are deployed as independent StatefulSets within a Kubernetes cluster, based purely on a Stream CRD (YAML config) stored in Kubernetes. Each pod of the StatefulSet acts as the leader of a partition of its Stream. Kubernetes guarantees the identity, stable storage and stable network address of each pod. When replication is enabled for a Stream, replication is based on a deterministic algorithm and does not require leadership or consensus given these powerful identity properties from the StatefulSet building block.

### Scaling
Streams can be horizontally scaled based on the Stream's CRD. Scaling a Stream to add new partitions will cause the Hadron Operator to scale the corresponding StatefulSet, which ultimately adds a matching number of pods.

Each pod constitutes an independent container process with its own set of system resources (CPU, RAM, storage), all of which will span different underlying virtual or hardware nodes (OS hoststs) of the underlying Kubernetes cluster. This helps to ensure availability of the Stream.

### High Availability (HA)
In Hadron, the availability of a Stream is reckoned as a whole. If any partition of the Stream is alive and running, then the Stream is available. The temporary loss of an individual partition does not render the Stream overall as being unavailable.

This is a powerful property of Hadron, and is an intentional design decision which the Hadron ecosystem builds upon. Hadron clients automatically monitor the topology of the connected Hadron cluster, and will establish connections to new partitions as they become available, and will failover to healthy partitions when others become unhealthy. Hashing to specific partitions based on the `subject` of an event or event batch is still the norm, however clients also dynamically take into account the state of connections as part of that procedure.

### Producers
Hadron clients which produce and publish data to a Hadron Stream (or other component) are considered to be Producers. Stream Producers establish durable long-lived connections to all partitions of a Stream, and will publish data to specific partitions based on hashing the `subject` of an event or event batch.

### Consumers
Hadron clients which consume data from a Stream (or other component) are considered to be Consumers. Stream Consumers establish durable long-lived connections to all partitions of a Stream, and consume data from all partitions. Every event consumed includes details on the ID of the event as well as the partition from which it came. This combination establishes uniqueness, and also happens to be a core facet of the CloudEvents ecosystem.

Consuming data from a Stream is transactional in nature, and the lifetime of any transaction is tied to the client's connection to the corresponding Stream partition. If a Consumer's connection is ever lost, then any outstanding deliveries to that Consumer are considered to have failed and will ultimately be delivered to another Consumer for processing.

#### Groups
Stream Consumers may form groups. Groups are used to load-balance work across multiple processes working together as a logical Consumer.

All consumers must declare a group when they first establish a connection to the backing Stream partitions. When multiple consumers claim to be participating in a group bearing the same name, then they are implicitly members of the same group.

#### Durable or Ephemeral
Stream Consumers may be durable or ephemeral. Durable groups will have their progress recorded on each respective Stream partition. Ephemeral groups only have their progress tracked in memory, which is erased once all group members disconnect per partition.
