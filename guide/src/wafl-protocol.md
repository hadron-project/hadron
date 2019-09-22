The WAFL Protocol
=================
A protocol which provides a robust pattern for building event-driven streaming architectures.

WAFL is a protocol which operates at the highest level of systems architecture. WAFL creates a bridge between the synchronous request/response world and the asynchronous event-driven world. In the most simple terms, WAFL inverts the paradigm of a WAL (Write-Ahead Log) feeding into a state machine, and instead uses a transaction log which applies fallible requests to a state machine and then records them in a WAFL (Write-After Log) to provide the event-driven streaming capabilities.

To understand the motivation behind the WAFL protocol, we need to understand the tradeoffs between various architecture types. The guarantees of the synchronous request/response world are nice. ACID transactions. Ease of creating idempotent workflows. The problem is that we don't get any of the benefits of an asynchronous event-driven system. We want those benefits. The difficulties start to show up when we realize that we can't just blindly write to a data stream first and then treat that stream a source of truth, because there are very often state-based constraints which must be upheld. Before we transfer $100 from Bob to Alice, we need to be sure that Bob actually has $100 dollars to transfer.

WAFL emerges as a general solution to this problem of needing a simple and comprehensible pattern for enforcing state-based constraints, while still achieving an event-driven architecture. Other attempts to solve this problem include building partial or complete in-memory representations of the system's state at ingest points for enforcing these state-based constraints. This "local state" pattern has obvious overhead and disaster conditions. Many teams have a knee-jerk reaction to hearing of such patterns, and are often times justified.

WAFL leverages the source of truth state machines of a system to perform dynamic validation and enforce state-based constraints on inbound events. This keeps architectures simple, allowing engineers to develop against thier databases as they normally would, while still achieving an overall event-driven architecture, all without needing to build, maintain, and coordinate local state ingress nodes. This also means that WAFL is immune to eventual consistency and can be used with traditional request/response APIs and frameworks (REST, gRPC, GraphQL &c).

### What We Want
Before diving into the details of the WAFL protocol, it is important to describe what our overall goals are with building a system which includes WAFL as part of its architecture.

##### Events & Streams
We want an event-driven system. We want a streaming system. We want a durable log of the events which have taken place in the system overall, and we want to be able to quickly and easily build new features using that same data. These new features might involve introducing new datastores. Eg, Redis for caching, Elasticsearch for full-text search, a new database for a new microservice, whatever.

Overall, the benefits of an event-driven architecture might be summarized as follows:
- Asynchronous by default.
- New features can be added by adding a new consumer of your data streams.
- Inherent modularity, as new consumers may exist as a standalone module or even as a microservice if needed.
- Events are condusive to the world of reactive client applications. Websockets and server-sent events can be easily built by leveraging event streams.
- DDD becomes much more simple to adhere to. Decoupling can take place at the event level. Services no longer need to build complex intra-service communication patterns for most write-paths, as new streams and events can be used for downstream work.
- Event streams can be seen as a WAL for an entire system.
    - Allows us to rebuild state machines or build new state machines needed for new features.
    - Allows us to implement new features — planned or unplanned — using the event streams (think auditing, caching, billing, stats/metrics, push notifications, provisioning systems).

##### ACID
Though we want the benefits of an event-driven system, we don't want to sacrifice the guarantees provided by traditional ACID, request/response, transactional systems.

These sorts of systems offer benefits along the lines of:
- Comprehension. It is much more simple to reason about business requirements and how they are enforced when you can immediately go to your SQL/NoSQL database and run a few queries.
- Enforcing business level constraints often requires unique indexes, and dynamic data lookups. Being able to leverage battle-hardened databases, which are built for this purpose, means that teams have less code to write and maintain. The local state pattern, where ingress nodes build copies of the full or partial state of the system for these purposes, can be faster for enforcing these constraints as no network requests would be needed; however, this comes at the cost of design, writing code, maintaining code and often includes subtle race conditions which can cause critical problems if not addressed.

### WAFL
WAFL is a protocol which adds a set of strong and desirable attributes to a system's architecture, and also ensures that some non-desirable attributes do not become attributes of that system. WAFL provides the desirable attributes of event-driven streaming systems, without the eventual consistency and indirection; and also provides the foundational attributes and guarantees of ACID transactions found in traditional systems.

WAFL is needed when a system needs to enforce dynamic constraints on inbound events. Events which have no dynamic constraints associated with them may short-circuit the WAFL protocol and may be directly written to the domain stream (described below).

#### Components
The WAFL protocol has three core components:

- Request/Response Feed (RRF): this could be any request/response system. REST, gRPC, GraphQL and the like.
- Transaction Log (TXL): a semi-durable stream which holds all events which have been requested to be applied to the system. This should be a private stream, only accessible to the respective domain.
- Write-After Log (WAFL): a fully durable stream which records data mutations which have been applied to a system. Events on this stream are intended to be treated as the source of truth for its given domain, and may be consumed by components even from other domains.

#### Implementation
To implement the WAFL protocol, data will flow through the three components (defined above) in the following pattern:

1.) A request enters the system via the RRF, and as long as the contents of the request pass preliminary static validation, it will be written to the TXL with a unique ID as part of the data model. We will call this unique ID the TXID.

2.) The consumer of the TXL will attempt to apply the data to the system's state machine. Typically this will be a database of some sort. If the database is transactional, a table should be written to as part of the transaction which records the TXID being applied, and the TXID should have a unique index on it. This ensures that the same transaction will not be applied multiple times.

If the database is non-transactional, then some other mechanism must be use to guarantee that the TXL event is not applied multiple times. For many NoSQL systems, this can be accomplished with atomic operations using a denormalized data model.

3.) Once the transaction has been applied successfully, the handler must transactionally publish its output data to the WAFL as part of its TXL ack operation. Not all streaming platforms support this capability. If it is not supported, then deduplication may be needed on the WAFL, in which case the TXID should be used for deduplication.

For streaming platforms which support transactional ack + publish operations, a consumer of the WAFL should be built which will delete TXID records from the database after the event has been written to the WAFL. This ensures that data does not endlessly grow, and is safe as a retry will not be performed once the message has been written to the WAFL.

Now that the data has landed on the applied stream, which means that it has been applied to the system's datastore, there is no concern about eventual consistency, there will be no stale reads (depending on the datastore and write concerns), and any number of additional downstream stages may be triggered from the event on the WAFL. New features, such as auditing, metrics, billing, push events, notifications, caching ... all of these things may be easily added as new features of the system due to the applied stream having a permanent record of the data which has been applied to the system.

The WAFL protocol yields all of the benefits of event-driven design, streaming architecture, and event sourcing. All with a more simple implementation and without the threat of eventual consistency.

### Alternatives
#### Local State
There are many different approaches to the local state pattern. Ultimately, they all come down to having a copy of the system's state — whole or partial — present on the stream processing node which needs access to the state. This has various tradeoffs, but the coordination involved in avoiding race conditions related to client requests which mutate state and which have state-based constraints make the local state pattern a bit of a non-starter for many teams. With WAFL, teams may leverage their databases as normal, and simply implement the WAFL protocol to achieve an event-driven architecture with strong consistency and exactly-once guarantees.

#### Change Data Capture
Change Data Capture (CDC) is a pattern which allows inserts, updates and deletes taking place in database to be captured as events and emitted to connected clients. Conceptually, CDC is an excellent pattern, and is very useful for many use cases. The primary difficulty with using CDC as the backbone for a system's architecture is the disconnect between higher-level events and the events received by CDC consumers. There is ambiguity between the event which caused the change and the actual captured changes. Building a declarative log from CDC events is a formidable task, not to be taken lightly. Scaling these patterns to drive an entire system can be quite complex. With WAFL, events are observed in their original form and thus do not suffer from the same ambiguities which CDC events do. CDC may likely still prove useful within a WAFL architecture, but does not seem to offer the same benefits overall.

----










TODO: finish this up

### Railgun & WAFL
Railgun was created to address the lack of features needed to create event-driven streaming architectures with ease. Other messaging/streaming platforms would offer some of the needed capabilities but would be lacking others. Railgun was builRailgun's RPC system for

#### Railgun Properties
- Railgun's transactional/atomic ack + publish means that cleanup operations can be safely be performed as a downstream consumer stage, because once a message from the parent stream is ack'ed, it will not be consumed/retried by the same consumer.
- The ASA model works with transactional and non-transactional datastores, as non-transactional datastores can just denormalize the data, and embed the txid.

#### idempotence guideline
As the general rule of thumb, creating idempotent operations can be difficult, and there are many factors to account for. Keeping operations small and concise is typically a boon. If lots of sparwling actions need to be taken, typically breaking them up into isolated downstream processing stages would be best.

##### record creation
This is a simple case to deal with. Using the ASA model, ensure that the TXID of the transaction stream has not already been applied. This can be performed as a single transactional operation in many data storage systems.

##### record mutation
Updating data is a bit more tricky; however, with the ASA model, this is as simple as checking the TXID just as was the case for record creation.

##### record deletion
For this pattern, TXIDs will still be needed, as retry scenarios may cause confusing error messages to be returned to clients. Use the ASA model of recording the TXID. This will not only guarantee a properly function system, but will also keep the client experience predictable.

##### retries
With the ASA model, retries can be handled in a few different ways, depending on the desired outcome. When a handler receives a message, it can check the message's delivery count. If the value is greater than 1, this indicates a retry.

Given the ASA model of recording the application of TXIDs, the TXID can be checked, and if it exists, this means the data mutation has already been applied. It is safe to skip that operation and proceed to the next step (which should just be the ack + pub to the applied stream). Else, if the TXID does not exist, it is safe to abort the operation and return a failure to the client, if such behavior is needed.
