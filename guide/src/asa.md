Idempotence Guidelines
======================
In the world of software systems, idempotence is a very important property. When teams are building APIs for their platforms, user intractions need to be idempotent, database updates need to be idempotent, we don't want duplication, we don't want incorrect data, we don't want to double-charge a user when they purchase something, and the user certainly doesn't want to be double-charged either.

In order to understand idempotence, we should start off with a definition. I like Wikipedia's:

> Idempotence (UK: /ˌɪdɛmˈpoʊtəns/, US: /ˌaɪdəm-/) is the property of certain operations in mathematics and computer science whereby they can be applied multiple times without changing the result beyond the initial application.
>
> — https://en.wikipedia.org/wiki/Idempotence

Within traditional non-event-driven systems, it is often times more difficult to make an operation idempotent. Without durable streams, request information is ephemeral. Data related to a request could be persisted in a database, but reading that data out of the database in a timely fashion, building events around that data, and applying different operations on that data ... this is no small task. At that point a team would be dealing with multiple failure domains, the design would quickly become unwieldy, and evolving the system for new features may quickly become intractable.

With the rise of event-driven architectures, many of the difficulties of older systems are relieved, but other difficulties arise. The most nasty of these difficulties is "eventual consistency". Not in the sense of your database being an "AP" system (a la CAP theorem), but in the sense that the components of the system which are responsible for applying events to a database — what is often referred to as _materialization_ — could be offline for some amount of time, and the errors which may arise in a system like this can also become a nightmare.

### Applied-Stream Architecture
The Railgun team proposes the Applied-Stream Architecture (ASA) as a way to maximize scalability, extensibility, reliability and the overall robustness of systems which involve state. ASA builds upon the paradigms of event-driven architecture, and borrows from CQRS & event sourcing patterns. ASA includes an idempotence guideline for common data mutation patterns. ASA also ensures that eventual consistency will not become a property of a system built according to the ASA pattern.

ASA has three moving parts:
- request/response feed: this could be any request/response system. Railgun's RPC system is recommended. Within Railgun, this should be the entrypoint of a pipeline.
- transaction stream: a semi-durable stream which holds all events which have been requested to be applied to the system. This should be a private stream, only accessible to the respective domain. Within Railgun, this should be a private stream of a pipeline.
- applied stream: a fully durable stream which records data mutations which have been applied to a system. Events on this stream are intended to be treated as the source of truth for its given domain, and may be consumed by components even from other domains. Within Railgun, this should be a public stream of a pipeline.

The flow of data is as follows:
- A request enters the system, and as long as the contents of the request pass preliminary validation, it will be written to the transaction stream with a unique ID as part of the data model. We will call this the TXID.
- The consumer of the transaction stream will attempt to apply the data to the system. Typically this will be a database of some sort. If the database is transactional, a table should be written to as part of the transaction which records the TXID being applied. Once the transaction has been applied successfully, the handler should publish the data to the applied stream as part of its ack operation. Non-transactional databases may just use a denormalized model to record that the TXID has been applied.
    - When the transaction handler first begins to execute, it should check the transaction table to ensure the transaction has not already been applied. If it has already been applied, it may short-circuit the handler and immediately write to the applied stream. For non-transactional systems, check the denormalized data model for the TXID. Within Railgun, pipeline consumers are always members of a queue group, so it is guaranteed that two handlers will not be processing the same event as part of the same consumer group.
    - Because of Railgun's transactional/atomic ack + publish feature, it is guaranteed that once the message has been ack'ed, it will not be redelivered or retried by the same handler. This also means that downstream stages may use the TXID found on the applied stream to perform cleanup operations or the like.
- Now that the data has landed on the applied stream, which means that it has been applied to the systems datastore, there is no concern about eventual consistency, there will be no stale reads, and any number of additional downstream stages may be triggered from the event on the applied stream.
    - New features, such as auditing, metrics, billing, push events, notifications, caching ... all of these things may be simply added as new features of the system due to the applied stream having a permanent record of the data which has been applied to the system.

ASA yields all of the benefits of event-driven design, streaming architecture, and event sourcing all with a more simple implementation. One which many teams are accustomed to with more traditional architectures.

#### Railgun Properties
- Railgun's transactional/atomic ack + publish means that cleanup operations can be safely be performed as a downstream consumer stage, because once a message from the parent stream is ack'ed, it will not be consumed/retried by the same consumer.
- The ASA model works with transactional and non-transactional datastores, as non-transactional datastores can just denormalize the data, and embed the txid.

#### idempotence guideline
As the general rule of thumb, creating idempotent operations can be difficult, and there are many factors to account for. Keeping operations small and concise is typically a boon. If lots of sparwling actions need to be taken, typically breaking them up into isolated stream consumer stages which are triggered from an applied stream would be best.

##### record creation
This is a simple case to deal with. Using the ASA model, ensure that the TXID of the transaction stream has not already been applied. This can be performed as a single transactional operation in many data storage systems.

##### record mutation
Updating data is a bit more tricky; however, with the ASA model, this is as simple as checking the TXID just as was the case for record creation.

##### record deletion
For this pattern, TXIDs will still be needed, as retry scenarios may cause confusing error messages to be returned to clients. Use the ASA model of recording the TXID. This will not only guarantee a properly function system, but will also keep the client experience predictable.

##### retries
With the ASA model, retries can be handled in a few different ways, depending on the desired outcome. When a handler receives a message, it can check the message's delivery count. If the value is greater than 1, this indicates a retry.

Given the ASA model of recording the application of TXIDs, the TXID can be checked, and if it exists, this means the data mutation has already been applied. It is safe to skip that operation and proceed to the next step (which should just be the ack + pub to the applied stream). Else, if the TXID does not exist, it is safe to abort the operation and return a failure to the client, if such behavior is needed.
