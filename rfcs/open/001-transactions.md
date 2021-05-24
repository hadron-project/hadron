# Transactions
```
Author: Dodd
Status: InDesign
```

Hadron transactions provide a mechanism for ACID stream processing. Subscribers can seamlessly enter into a transaction while processing a stream payload, transactionally publish payloads to other streams, and then commit. Either everything will be committed successfully and applied to the system, or all of the statements will be rolledback and the subscription payload nacked.

## Wire Protocol
- Any standard stream or pipeline subscriber may enter into a transaction by responding with a TxBegin response variant.
- This will change the protocol such that multiple TxStatements may be issued in response by the subscriber, where each statement adds new payloads of data to be published to target streams of the cluster as part of the TX.
- A TX response will be issued back to the subscriber to acknowledge the TX statement or rejecting the statement and thus closing the delivery (a server side nack, effectively).
- The TX statements will be applied to disk locally (causing the local node to be the distributed TX driver). Location TBD, but probably a dedicated local tree.
- Once the subscriber acks the delivery, this will commit the TX. A response will be issued back to the subscriber to indicate if the ack was successful or if the TX had to be aborted/rolledback for any reason.

## Subscriber Workflow
- The node on which the transaction statements are being applied is responsible for driving the application of statements to other replica sets / streams of the system.
    **MAYBE** but it might be nice to have each subscription controller handle the TX functionality as well.
    - Communication with the TXController from other controllers will be done purely via a shared MPSC. Begin statements will require a onshot channel to be sent in the message. This is used by the TXController to communicate the ultimate success or failure of a TX.
    - New statements are added to the shared disk location, and those locations are then sent to the TXController via statements sent along its MPSC.

## TX State Flow
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
