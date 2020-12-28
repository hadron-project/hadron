todo
====
Kafka and others help with building EDA apps, Hadron helps more. Pipelines provide a native mechainism which greatly simplifies the building of complex applications.

## Controllers
Build remaining controllers:
- [ ] stream partition controller (SPC): participates with a group of other SPCs responsible for a single partition of a stream. One leader, >= 0 replicas.
- [ ] stream consumer controller (SCC):
- [ ] pipeline consumer controller (PCC):
- [ ] transaction controller (TXC):

### Cluster Raft Controller (CRC)
- all cluster-wide changes go through this controller and are based on Raft.
    - cluster membership
    - leadership designations for other controllers
    - schema management
    - users & tokens
- controller leadership designation is based on a monotonically increasing term per control group, which is disjoint from Raft's leadership terms.
- conflicts between leadership designation is easily and safely resolved based on designated leadership terms.


- [ ] given that storage initialization may take some time, pass a signal emitter down to the storage engine so that it can tell the rest of the app when initialization has actually finished.
    - [ ] the network layer should refuse to perform peer handshakes and refuse client connections until the system is ready.
- [ ] build dynamic membership system, most everything is in place.
- [ ] ensure delays are set on raft requests when a peer channel has been disconnected.
- [ ] finish up tests on DDL.
    - [ ] add namespace DDL (the "default" namespace is always present and can not be removed)
    - [ ] validate namespace names
    - [ ] perform cycle tests to ensure stage `after` & `dependencies` constraints do not form cycles in the graph

---

- [ ] open issue for creating initial streams for
    - CRUD on objects in the system
    - stream for metrics
- [ ] open an issue on future integration with Vault as a token provider.
- [ ] open issue for having admin UI setup with OAuth handler so that orgs can grant viewer permissions to anyone in their org.
- [x] combine all internal error types to a single type for more uniform handling.
<!--







### stream storage
Writing data to streams through Raft is a bad idea. Instead, use Raft to nominate stable leader of a stream, and that node will handle all writes to the stream.
- partitions are basically the only way to scale out write throughput.
- this also has the added advantage of the data being partitioned to spread disk load across the cluster.
- partitions can only be increased, and removal == data loss.
- stream partitions & replication factor will be defined in the DDL.
    - should have a global config for both of these (default partitions & replication factor).

**partition leadership**
- whenever a node stops or crashes, leadership for that nodes's partitions transfers to other nodes. When the node is restarted it will only be a follower for all its partitions, meaning it will not be used for client reads and writes.
    - when a node comes online, before it will open streams for writing it will check with the master to read the latest config for its stream. If it does not have the latest config, it will wait until it has replicated such data from the master before resuming work.
    - when transitioning partition leadership, this will typically only take place because a node is dead or is being drained; as such, only a majority of the members of a partition replica set need to ack the config change.
    - the replication protocol / communication ensures that stale leaders will be discovered quickly & will not cause invalid writes.
- optimize write path to go directly to a partition leader to make writing as fast as possible.
    - every node should be able to make partition load balancing decisions.
    - cluster raft leader propagates decision info by way of state machine updates, which are replicated to all nodes in the cluster.
- availability zone / rack placement will be a dynamic configuration property of nodes, given that the location of a node may change. If the cluster master detects that all replicas of a partition are in the same AZ, then a new replica will be selected and data will be moved to the new replica.
    - This influences acks.
    - Acks should be:
        - `Write`: the message has been written by the leader.
        - `Majority`: a majority of replicas have acked the write (default).
        - `All`: all replicas have acked the write. If acks is set to `All`, then any replica being down will block successful writing.
    - When a node is being drained (removed from the cluster), or self-healing has nominated to cordon a node, then any replicas the node was responsible for will be moved over to other nodes.

- cluster self-healing will have to ensure that a dead node to be pruned from the cluster will only be pruned if all of its data under management is replicated elsewhere. Else, Hadron will just mark the node with a specific tag indicating that data will be lost if removed from the cluster. Keeping a replication factor > 1 will make this less likely for streams.

**storage**
- instead of using a FS structure, we can use sled.
    - use its ID generator.
    - no immediate flush is needed, just allow the background flush every Xms.
    - just use bincode to encode the raw bytes of the message.
    - searching through the data is quite fast.

- messages need to be stored with some associated metadata which can be used to determine actions to take during consumption, within the context of compression &c.

- MAYBE: clients should be required to provide a minimal amount of data when producing such that a proper CloudEvents 1.0 object could be constructed.

**datacenters**
- in the future we can roll out a hyper-cluster feature.
- it will allow for disparate clusters to be linked.
- linked clusters will be able to asynchronoysly replicate streams from peer clusters. This primarily offers faster stream consuming for streams which have been replicated into a local cluster

TODO: review general server level configs here: https://kafka.apache.org/documentation/#brokerconfigs

### producer
- https://kafka.apache.org/documentation/#acks we should follow basically the same policy, except use more well-defined enums.
- https://kafka.apache.org/documentation/#buffer.memory will need options for this.
- https://kafka.apache.org/documentation/#compression.type will need to support these various options. We can encapsulate this in the Rust core driver, then other lang libs will only need to specify the params.
- https://kafka.apache.org/documentation/#retries will need this.
- https://kafka.apache.org/documentation/#batch.size will need batching.
- https://kafka.apache.org/documentation/#client.id probs.
- https://kafka.apache.org/documentation/#delivery.timeout.ms definitely.
- https://kafka.apache.org/documentation/#linger.ms probably call this batch_delay_ms.
- https://kafka.apache.org/documentation/#request.timeout.ms pretty standard.
- https://kafka.apache.org/documentation/#transactional.id need to pin down cross stream & stream + pipeline transactions.

- need to look into using a key, along with other metadata, per message in order to perform hashing on key for load balancing to partitions as well as determining decompression server side.

**out table**
In order to support a fully idempotent stream producer, with actual "exctly once" guarantees, let's build upon the out table pattern.
- teams will be transactionally generating events in the DB.
- these events will be written as part of the same transactions updating the team's business critical data.
- any number of application specific routines, or a Hadron OutTable Connector in the future, will be able to read the events from the out table, transactionally consuming them, and then writing them to the Hadron stream.
- once the batch write has finished, the out table events will be deleted and the transaction committed. A failure to committ the transaction will just caused them to be redelivered.
- here's the secret sauce of our storage system: we can have the stream configured to take entry IDs only from the producer, and given that the out table will have an ID column which is monotonically increasing, we can use that ID as the ID of the event, and then events produced bearing the same ID will simply be no-op'd.

Implications
- stream can only be single partition, though it can still have normal replication factor. The reduced write throughput should be fine, as it is very unlikely that a transactional RDBMS will be able to produce rows fast enough to outpace batched Hadron writes.

### consumers
per https://kafka.apache.org/documentation/#theconsumer
- there may be a good bit of value in following the model described here where consumers pull data; **HOWEVER,** they also employ long-polling ... which is damn near the same as server push.
- to make things a bit more optimized, we could have the server send a stateless message to interested consumers to tell them when new data is available, this would help to reduce wait times
- long-polling with a batch size & wait period is what Kafka currently uses

- kafka uses a single value "offset" for tracking a consumers progress through a partition
- if the consumer fails while processing a batch, the whole batch fails and will be redelivered

- static consumer group IDs will not work well in K8s environments, so let's not worry about the static membership feature. Keep it dynamic.

### transactions
- we will use a cluster transaction controller.
- it will replicate its changes to all nodes of the cluster, but only require majority replication to ack.
- leadership of the controller will be determined similarly to how stream leadership is determined, based on health of nodes in the overall Raft cluster and a live stream of peer connectivity from the view of the Raft master.
    - stream leadership & Hadron controller leadership failover should be triggered as soon as it detects that a delegated leader's connection is dead.
    - this will be informed from the network layer's peer healthcheck routines, which will always record the last successful healthcheck, and will prune dead connections and build new ones.

- perhaps a pipeline controller will be similar. We could abstract over this pattern and call it a Hadron controller (which is internal only). So far:
    - transaction controller
    - pipeline controller
    - stream consumer controller
    - stream partition controller: replication factor is user controlled for these, based on the DDL of the stream.
    - leadership controller: responsible for nominating the leader of the various controller groups.

**workflow**
- beginning a transaction will create a new TX record & will associate a transaction stream with the connection. Transactions are bound to the lifetime of the connection-bound transactional bi-directional channel. If the channel is dropped, the transaction will be rolled back.
- the connection is then the only thing which can manipulate the transaction.
- as transaction statements come in, they will be written to the TX record itself, and then also sent over to the target stream / pipeline to prep for commit.
- once a commit comes in, we will immediately update the state of the TX to committing, and then begin comitting each of the statements in parallel, starting with streams, then pipelines, then ephemeral messages.
- when streams commit the messages added as part of a TX, they will update the write intent object with the IDs of the messages written, and then set the write intent to state committed.
    - this is to ensure that if communication fails after writing the message to the stream as part of committing the write intent, the transaction controller will still be able to access the IDs of the messages committed as they are needed for triggering pipelines.
- once everything has committed, we go through and delete the write intents on streams, and then delete the TX record.

- when a stream record is written as part of a transaction, we need to be able to return the ID of the stream event (though it has not yet committed).
    - we will need to use a "write intents" pattern on the stream — maybe just a parallel tree marking keys which are in tx.
    - as keys are committed we will remove the write intent and commit the key.
    - as keys are aborted, we will remove them.

**stream consumers**
- stream consumer patterns will be different than Kafka.
- we will use a stream consumer controller for managing offsets and coordinate load balancing consumption across partitions of a stream.
- stream consumers are able to process messages one by one. The entire consumer group can receive a large number of different messages.
- we track a consumer groups offsets per partition as using a head index — the ID of the most recently processed +1 — along with a set of outstanding messages which are behind the head index.
    - stream partition controllers publish info on their stream partitions, which is used internally and for metrics/monitoring.
    - the stream consumer controller has the overall goal of keeping each consumer group busy, keeping them as close as possible to real time processing of events as they become available for the target stream.
    - for each consumer group, the SCC maintains (disk & mem) offsets of the consumer group's progress through a stream per partition. Progress is tracked as a head index for the group per partition, along with a set of outstanding message IDs per partition.
        - messages are added to the outstanding set when they are being processed, or if they were within a batch range but are part of a TX which has not yet committed.
        - SPCs emit events as keys are written, TXs committed/rolled back, so the SCCs will not need to poll partitions for the status outstanding keys, but will be able to react to their state changes in real time.

CONSUMER PATTERNS:
- exactly-once end-to-end strict ordering:
    - use a single partition out table stream as producer (this guarantees exactly once for the producer).
    - use a consumer group with max-in-flight of 1 which does blocks for pending transactions.
    - consumer group must transactionally materialize consumed events into a DB which supports unique constraints on the message ID or equivalent.

- all other patterns:
    - consumer group will receive messages from all partitions as they become available.
    - can set a max-in-flight per consumer.
    - consumer connections are always healthchecked, and if the connection misses healthchecks, the messages delivered on that connection will be redelivered to another member of the CG. Healthcheck rates and failure threasholds are configurable per consumer.
    - will have two different API endpoints, one for batch and one for individual message consumption.
    - batch size may be set per consumer when using the batch API.
    - ack'ing of messages is done one-by-one when using the individual message API and will be by batch when consuming the batch API.

**pipeline stage consumers**
- pipeline stage consumers should just use the transaction system. We can verify that the statements of the transaction satisfy the requrements of the pipeline stage being processed, and only accept a committ which is valid as such.
- only stream publications which are declared as part of the output of the stage are allowed in the TX.
- any number of ephemeral messages may be published as part of the TX.
    - when a pipeline is first triggered — either via stream pub or via admin triggering a re-run — the pipeline controller will already have a pipeline object in memory for the pipeline, and any active stage subscriptions will have subscription channel sinks on the object.
        - the pipeline controller will send the trigger over to the pipeline object which tracks all active pipeline instances.
        - each pipeline instance will track outputs as they are generated so that it knows when all dependencies of a stage are available. It will also track execution order so that it knows when order based dependencies are met. This is all tracked in the DB but is indexed in memory.
        - on each event processed by a pipeline object, it will trigger a series of zero or more stage invocations which are passed over to viable/registerd stage handlers. They are only triggered when all stage deps are met and there is a viable consumer.
        - stage handler objects are responsible for fetching all of the stage dependencies, which are known by namespace/stream/eventID and which are contained in the stage handler invocation. The handler is then able to pass that info down to the actual stage consumer via channel.
        - the fact that a consumer is actively consuming a stage event is volatile, and held in memory only. A server crash will cause redelivery if the stage was not successfully completed before the crash.






-->
