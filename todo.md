
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
