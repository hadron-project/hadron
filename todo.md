
TODO
====
Single Node

- [ ] Update docs for new metadata / schema system as well as new architecture.
- [ ] Need to update stream records to allow for header inclusion.
- [ ] **PIPELINES:** When a pipeline is created, as it is simply another consumer of the input stream, we need to be able to allow users to specify the pipeline's starting point. We should support all of the same options as a normal consumer.
- [ ] Long-term, create TTL tokens. Hadron will create a TTL index on the tokens to ensure they are removed from disk on expiration.

**Future stream pub:**
- [ ] In the future, the Producers will be able to specify durability of payloads, and the async replication system will report back as batches are replicated.
- [ ] If there are no replicas of a replica set, then we will fsync each batch written to disk. If there are replicas, then "durable" writes means that a majority of the replica set has the data on disk, and no flush required on write path.
- [ ] Writes should be batched across clients for better efficiency, and will be applied to disk using a durable batch. This is actually exactly how the sled periodic fsync behavior will be once we introduce the replication system.

Cluster of Replica Sets
=======================
Each replica set is statically configured with exactly one master and any number of replicas. Roles are established statically in config.

- Config changes require restarts. When a node first comes online, it will not fully start until it is able to establish a connection with the members of its replica set & determine that their config matches. This is treated as the handshake. Data to match:
    - replica set name
    - node names
    - role per node
- If handshake fails, backoff and try again. Warn about the potential misconfiguration issue.
- It is recommended that replica set names semantically indicate geo and or cloud infomration, as well as purpose. Though the only hard requirement is that replica set names be unique throughout a cluster.
    - An example of such a partition naming scheme could be: `aws.us-east-1/p0`, `aws.us-east-1/p1`, `gcp.asia-southeast1/p0`, `gcp.asia-southeast1/p1`.
    - In this example, replica sets span two different clouds, and two different regions.

### Consensus
Consensus within a replica set is dead simple, and is based on static config. The leader is always known and doesn't change. An entry is committed based on majority replication.

### Cluster
Clusters are composed of multiple replica sets which dynamically discover each other.

- Each replica set may be part of only one cluster. If the cluster discovery info & ports match, then the replica sets will form a cluster.
- A single replica set may be statically configured as the metadata group. This group will then be responsible for all cluster wide metadata changes, such as stream and pipeline definitions.
- HA is achieved by having multiple partitions assigned to the same stream. Clients are able to detect when a partition is down, and so they will failover to one which is alive.

### Metadata
- A single replica set acts as the metadata leader for a cluster based on static config.
- Metadata is asynchronously replicated to all members of the cluster, always with an associated metadata index.
    - This allows for a cluster to more easily span the globe, multiple regions and differnet clouds.
    - Clients can query for metadata at any location, and they will always be as up-to-date as a direct query to metadata replica set directly, due to the constant async replication of config.
- Hadron schema objects now declare their "partition" assignments explicitly.
    - For example, when a stream is declared, it will declare its partitions as a list of replica set names.
    - It could declare that the stream has 4 partitions, as follows: `aws.us-east-1/p0`, `aws.us-east-1/p1`, `gcp.asia-southeast1/p0`, `gcp.asia-southeast1/p1`.
    - These would match replica set names of actual replica sets in the cluster.
    - As clients publish data to these partitions, they can hash to a specific partition of the region where their data needs to reside, or hard code the partition they are targetting.
    - This supports cases where data semantically belongs on the same stream, but is optimized to reduce latency by geolocating replica sets in the regions where the data is being processed, GDPR or similar requirements.
    - The cluster will still see all of these partitions as participating in the same stream, and consumers can be configured to consume from any subset of the partitions of a stream.

### Stream Subscribers
- Clients will create stream subscriber channels on all partitions of a target stream.
- Clients are always part of "group", and each partition will load balance deliveries across all active members of a group.
- A client may receive multiple concurrent deliveries from different partitions. Concurrent consumption rate is configurable.

### Pipelines
Pipelines are a mechanism for driving complex behavior based on an input stream.

- Pipelines are divided into stages.
- Each stage may produce an output.
- Stages may depened upon previous stages, in which case they will receive the output of the pervious stages.
- All stage outputs are stored as part of the pipeline, on the node of the pipeline controller responsible for the pipeline partition, and are not sent to other locations in the cluster (except for replication within a replica set).
- FUTURE:MAYBE: as a complete replacement to the future transactions system, we instead allow pipelines to declare `streamOutputs`.
    - These will be required to be supplied as part of the pipeline `ack` payload.
    - The payloads will be immediately written to disk along side the pipeline stage's normal output, but will be asynchronously transactionally applied to the target streams. The offsets of the stream outputs will be updated in the pipeline instance's records as a pointer to the stream & partition to which the record was written.
    - This will provide a more structured model for transactional processing overall.
    - Only a normal stage output can be used as input to another stage, not stream outputs.

**Setup & Delivery**
- Pipeline controllers will run parallel to the stream controller of the trigger stream, per partition. This is where pipeline horizontal scalability will come from.
- Pipelines are somewhat akin to a single subscription group, as every subscriber of a pipeline stage are part of the same group implicitly per pipeline.
- Each pipeline controller acts very similar to the subscription controller. It will await offset updates from the stream, and when updates are detected, the pipeline will query for data from the stream and analyze each record for matching event types (based on a record header value).
- For each matching record found, a new pipeline will be instantiated using that data.
- For each entrypoint stage of the pipeline, data will be delivered to a randomly selected channel subscribed to the stage.
- MAYBE: allow pipelines to declare a `maxParallel` value on the schema, which controls the max number of pipeline instances which may run in parallel. No limit by default, adding more consumers will provide more throughput.

**Acks & Output**
- An ack response from a stage subscriber must include the output required by that stage (if applicable), else the server will reject the ack and treat it instead as a nack.
- For a complete ack containing the required stage output, the data will be written to the pipeline's storage for that `{pipeline_instance_id}/{stage}`. Once that data has been successfully applied to disk (also recording that the stage was successfully processed), and all other stages of the same dependency tier have finished, then the stages of the next tier may begin.

**Data Model**
- A tree for the pipeline itself `/pipelines/ns/name/` & maybe one for pipeline metadata (tracking input stream offsets &c) `/pipelines/ns/name/metadata/`.
- The pipeline tree will record pipeline instances/executions based on the input stream's offset, which provides easy "exactly once" consumption of the input stream. In the pipeline tree, the key for a pipeline instance will be roughly `/{instance}/`, where `{instance}` is the input stream record's offset.
- Per pipeline instance, each stage will have its state and outputs (when completed successfully) stored under `/{instance}/{stage_name}`.
- All pipeline instance data, including stages, their outputs and the like, will be held in memory for all active pipeline instances. This facilitates better output delivery performance & less disk IO. Once a pipeline instance is complete, its memory will be released.

**Other**
- **Pipeline cancellation:** In the future, maybe we create an explicit cancellation mechanism along with a retry system. For now, it must be encoded by users as stage output. Pretty much always, we should push users away from the idea of fallible pipelines. They should be idempotent.

#### Hadron Pipelines VS Other
**Airflow**
- Airflow is all Python, Hadron is any language.
- Hadron pipelines are versioned & declarative; Airflow is dynamic Python, a bit more on the loose and wild side.
- Hadron pipelines are fully integrated into the Hadron data store, inputs and outputs are durable.

**Temporal**
- In Hadron, state is encoded as stage outputs. Outputs can be merged and flow into downstream stages.
- All complexity is removed, and stage consumers need only worry about stage inputs and the action to take on the outside world.

### Transactions
Hadron transactions provide a mechanism for ACID stream processing. Subscribers can seamlessly enter into a transaction while processing a stream payload, transactionally publish payloads to other streams, and then commit. Either everything will be committed successfully and applied to the system, or all of the statements will be rolledback and the subscription payload nacked.

**Wire Protocol**
- Any standard stream or pipeline subscriber may enter into a transaction by responding with a TxBegin response variant.
- This will change the protocol such that multiple TxStatements may be issued in response by the subscriber, where each statement adds new payloads of data to be published to target streams of the cluster as part of the TX.
- A TX response will be issued back to the subscriber to acknowledge the TX statement or rejecting the statement and thus closing the delivery (a server side nack, effectively).
- The TX statements will be applied to disk locally (causing the local node to be the distributed TX driver). Location TBD, but probably a dedicated local tree.
- Once the subscriber acks the delivery, this will commit the TX. A response will be issued back to the subscriber to indicate if the ack was successful or if the TX had to be aborted/rolledback for any reason.

**Subscriber Workflow**
- The node on which the transaction statements are being applied is responsible for driving the application of statements to other replica sets / streams of the system.
    **MAYBE** but it might be nice to have each subscription controller handle the TX functionality as well.
    - Communication with the TXController from other controllers will be done purely via a shared MPSC. Begin statements will require a onshot channel to be sent in the message. This is used by the TXController to communicate the ultimate success or failure of a TX.
    - New statements are added to the shared disk location, and those locations are then sent to the TXController via statements sent along its MPSC.

**TX State Flow**
Every subscription delivery has an associated TX, bound to the liveness of the H2 channel. TXs transition through the following states `pending` -> `committed` | `rejected`, and always start in the `pending` state.
- Statements may only be added to a `pending` TX. Once a TX is `committed`, no more statements may be added. Such statements will simply be dropped and accompanied by warnings about the protocol error.
- An internal network path will be taken to communicate with an internal endpoint on remote nodes for applying TX statements to remote stream controllers.
- When a subscription is `ack`ed, this appends a final statement to the TX which `ack`s the processing of the delivered payload, and also commits the TX and all of its statements. This will transition the TX into the `committed` state.
    - A TX can only be committed after all of its statements have been `applied`. If any statements are `rejected` then the TX will be aborted, including the processing of the associated delivery of the subscription.
- Once the TX is `committed`, then a clean-up routine will need to be performed.
    - In parallel, all statements will be applied transactionally (local sled TX) to their actual streams. For each statement applied, the offset of the applied record will be retained and record in the TX statement on the remote node, and will be returned to the TX driver.
    - Once all records' offsets have been returned to the TX driver and have been safely recorded, then it is safe to respond to the user with the success response.
    - Then the driver will issue a final cleanup call to remove the remote TX statement.
    - Once all statements have been fully cleanedup, then the driver's TX can be removed and closed.

Individual statements of a TX locally and remotely have the following states: `pending` -> `applied` -> `committed` | `rejected`. When a TX statement is first added to a TX, the TX driver will record the statement in a `pending` state, and then works in parallel to apply all statements to their final destinations.
- If the process of applying a TX statement to its final destination node fails for any reason, then the TX statement will be marked as `rejected`. Any `rejected` statements within a TX will cause the TX to be `rejected` overall before it can be `committed`.
- Statements are `applied` by the TX driver to other streams of the system in parallel as soon as the TX statement is received.
- When a node receives a TX statement on its internal endpoint to be applied, it will validate the request statically, and then will write the statement to disk. If an error takes place during this initial phase, the statement will be `rejected`, which will cause the parent TX to be aborted.
- At this point, the statement will not yet be applied to the target stream, and will not yet have an associated offset. At this point, the statement is ready to be committed or aborted.
- Once the parent TX is committed — which can only happen if all statements have been successfully `applied` — then all statements will be `committed` by the TX driver in parallel.
    - All statements will be applied transactionally (local sled TX) to their actual streams. For each statement applied, the offset of the applied record will be retained and recorded in the TX statement on the remote node, and will be returned to the TX driver.
    - After all statements have been `committed`, the TX driver will issue a final cleanup call to remove the remote TX statements.

### Exchanges & Endpoints
Ephemeral messaging exchanges & RPC endpoints.

- This model will be quite simple. A replica set is declared in the schema for exchanges as well as for endpoints.
- Exchange and endpoint consumers can attach to any node of the cluster. The node to which the connection has been established will consult the cluster's metadata and will then reach out to the controller node for the exchange or endpoint in order to establish itself as a consumer.
- This information is held in memory only by the leader of the exchange or endpoint.
- If the leader dies or a consumer connection dies, that information will be available almost immediately as durable connections are used, and clients will simply reconnect.
- The leader is then responsible for making load balancing decisions, and brokering RPC bidirectional communication — which is an easy setup with Rust's channel system.

### Future Possibilities
#### cron | scheduled tasks
- Execute a wasm function according to a given cron tab.
- Emit an ephemeral message according to a given cron tab.

#### task pool
- Unordered, non-batch processing of messages.
- General purpose queue.
- Delays & timers supported.



----










<!--
====
Kafka and others help with building EDA apps, Hadron helps more. Pipelines provide a native mechainism which greatly simplifies the building of complex applications.

- [x] placement system is receiving initial payload and is receiving events as they take place.
- [ ] placement driver needs reconciliation loop to be implemented.
    - implement placement algorithm & update CRC Raft based on this info.
    - movement of a replica from one node to another will simply be the process of adding the new node, and once it is up-to-date, we remove the old.
    - spawn controllers based on data. As soon as placement is determined, controller can be spawned.
    - Initialize control group rafts.
        - Easy. Exactly the same as the CRC. Initial set of members is used as config for initialization.
    - Control groups must be resilient to old members joining and disrupting clusters.
        - When PreVote is implemented in Raft, that will help.
        - For now, controllers only accept traffic from nodes which the CPC says are part of its cluster.
    - Add and remove members from control group rafts.
        - CPC can simply pass this data down to controllers and the controllers can take action based on the data. Only when controller is raft leader.

## Controllers
Build remaining controllers:
- [ ] cluster placement controller (CPC): this is the controller which maintains state on all active objects across the cluster, and when it is running on the Raft leader node, it will take executive action to make placement decisions and the like.
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

- [ ] controllers should have a channel sent up to the network layer for direct communication between clients & controllers.
- [ ] clients should have a configurable behavior where the client may reconnect to a specific node of the cluster in order to reduce forwarding between nodes.
- [ ] given that storage initialization may take some time, pass a signal emitter down to the storage engine so that it can tell the rest of the app when initialization has actually finished.
    - [ ] the network layer should refuse to perform peer handshakes and refuse client connections until the system is ready.
- [ ] if Raft triggers a shutdown, the rest of the node should be notified and should go into shutdown.
- [ ] build dynamic membership system, most everything is in place.
- [ ] ensure delays are set on raft requests when a peer channel has been disconnected.
- [ ] finish up tests on DDL.
    - [ ] add namespace DDL (the "default" namespace is always present and can not be removed)
    - [ ] validate namespace names
    - [ ] perform cycle tests to ensure stage `after` & `dependencies` constraints do not form cycles in the graph

---

- [ ] https://github.com/async-raft/async-raft/issues/101 for more stable & robust consensus.
- [ ] design for stream's to optionally register WASM functions as schema validators for events.
- [ ] open issue for creating initial streams for
    - CRUD on objects in the system
    - stream for metrics
- [ ] open an issue on future integration with Vault as a token provider.
- [ ] open issue for having admin UI setup with OAuth handler so that orgs can grant viewer permissions to anyone in their org.
- [x] combine all internal error types to a single type for more uniform handling.

---

# Designs WIP
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

Review general server level configs here: https://kafka.apache.org/documentation/#brokerconfigs

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

- need to look into using a key, along with other metadata, per message in order to perform hashing on key for client side load balancing to partitions as well as determining decompression server side.

### consumers
per https://kafka.apache.org/documentation/#theconsumer
- there may be a good bit of value in following the model described here where consumers pull data; **HOWEVER,** they also employ long-polling ... which is damn near the same as server push.
- to make things a bit more optimized, we could have the server send a stateless message to interested consumers to tell them when new data is available, this would help to reduce wait times.
- long-polling with a batch size & wait period is what Kafka currently uses.

- kafka uses a single value "offset" for tracking a consumers progress through a partition.
- if the consumer fails while processing a batch, the whole batch fails and will be redelivered.

- static consumer group IDs will not work well in K8s environments, so let's not worry about the static membership feature. Keep it dynamic.

#### Current Consumer Design
- Each stream / pipeline will also be able to define an `offsetsReplicationFactor` which determines the number of replicas which will participate in the control group for the stream / pipeline consumer controller.
- All offsets, consumer groups, and other such info is managed by these control groups (SCC, PCC).
- These are designed to be horizontally scalable, so as to not conflict with producer workloads and other workloads running on the cluster.

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
- we track a consumer groups offsets per partition using a head index — the ID of the most recently processed +1 — along with a set of outstanding messages which are behind the head index.
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

- make pipelines configurable such that they can be automatically triggered by their trigger stream based on the "event type" of the record being published to the stream.
- pipelines should still be able to be manually triggered based on stream event+id.





-->
