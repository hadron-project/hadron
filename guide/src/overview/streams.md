Streams
=======
Streams are append-only, immutable logs of data with absolute ordering per partition.

Streams are deployed as independent StatefulSets within a Kubernetes cluster, based purely on a Stream CRD (YAML) stored in Kubernetes.

Each pod of a Stream's StatefulSet acts as a leader of its own partition. Kubernetes guarantees the identity, stable storage and stable network address of each pod. Replication is based on a deterministic algorithm and does not require consensus due to these powerful identity and stability properties.

### Scaling
Streams can be horizontally scaled based on the Stream's CRD. Scaling a Stream to add new partitions will cause the Hadron Operator to scale the corresponding StatefulSet.

Each pod constitutes an independent container process with its own set of system resources (CPU, RAM, storage), all of which will span different underlying virtual or hardware nodes (OS hoststs) of the Kubernetes cluster. This helps to ensure availability of the Stream.

### High Availability (HA)
In Hadron, the availability of a Stream is reckoned as a whole. If any partition of the Stream is alive and running, then the Stream is available. The temporary loss of an individual partition does not render the Stream overall as being unavailable.

This is a powerful property of Hadron, and is an intentional design decision which the Hadron ecosystem builds upon.

Hadron clients automatically monitor the topology of the connected Hadron cluster, and will establish connections to new partitions as they become available, and will failover to healthy partitions when others become unhealthy. Hashing to specific partitions based on the `subject` of an event or event batch is still the norm, however clients also dynamically take into account the state of connections as part of that procedure.

### Producers
Hadron clients which publish data to a Hadron Stream are considered to be Producers. Stream Producers establish durable long-lived connections to all partitions of a Stream, and will publish data to specific partitions based on hashing the `subject` of an event or event batch.

### Consumers
Hadron clients which consume data from a Stream are considered to be Consumers. Stream Consumers establish durable long-lived connections to all partitions of a Stream, and consume data from all partitions.

Every event consumed includes details on the ID of the event as well as the partition from which it came. This combination establishes uniqueness, and also happens to be a core facet of the CloudEvents ecosystem.

Consuming data from a Stream is transactional in nature, and the lifetime of any transaction is tied to the client's connection to the corresponding Stream partition. If a Consumer's connection is ever lost, then any outstanding deliveries to that Consumer are considered to have failed and will ultimately be delivered to another Consumer for processing.

#### Groups
Stream Consumers may form groups. Groups are used to load-balance work across multiple processes working together as a logical unit.

All consumers must declare their group name when they first establish a connection to the backing Stream partitions. When multiple consumers connect to the Hadron cluster bearing the same group name, then they are treated as members of the same group.

#### Durable or Ephemeral
Stream Consumers may be durable or ephemeral. Durable groups will have their progress recorded on each respective Stream partition. Ephemeral groups only have their progress tracked in memory, which is erased once all group members disconnect per partition.

### Data Lifecycle
Data residing within a Hadron Stream has a configurable retention policy. There are 2 retention policy strategies currently available:

- `"time"`: (default) this strategy will preserve the data on the Stream for a configurable amount of time (defaults to 7 days). After the data has resided on the Stream for longer than the configured period of time, it will be deleted.
- `"retain"`: this strategy will preserve the data on the Stream indefinitely.

These configuration options are controlled via the [Stream CRD](../reference/streams.md).
