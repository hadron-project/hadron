Implement Transactional Processing
==================================

<small><i>This sub-chapter builds upon the ideas established in the previous sub-chapter [What is Transactional Processing](./learn.md) and it is recommended to have a firm understanding of that content before continuing here.</i></small>

Armed with the knowledge of event identity, we are now ready to implement our own transactional processing system.

For this use case we will make the following starting assumptions:
- We are using an RDBMS (like PostgreSQL or CockroachDB) for application state.
- We are using the [`OutTableIdentity` pattern](./learn.md#establishing-identity) to transactionally generate our events.
- We are using a microservices model, so we have multiple out-tables each in its own database schema (think namespace except for a database) owned by a different microservice.
- Every event published to our Hadron Stream is coming from one of our microservice out-tables. Let's say our Stream is named `events`.

What properties does this model have?
- Every event on our Stream will have a guaranteed unique identity based on the combination of the `id` and `source` fields.
- Duplicates may still exist, but we know they are duplicates based on the `id` and `source` field combination. Our implementation below will easily deal with such duplicates.
- As requirements evolve, we can seamlessly add new microservices to our system following this same pattern, and our uniqueness properties will still hold.

This is not the only way to implement a pattern like this, but it is a great stepping stone which can be adapted to your specific use cases.

### Transactional Consumers
Given our starting assumptions and the properties of our model, we are ready to implement a transactional processing algorithm for our consumers.

First, we'll show what this looks like for Streams, then we'll show what this looks like for Pipelines.

#### Stream Consumers
For any given microservice, we will need an in-table. For transactional processing, an in-table is the logical counterpart to an out-table, but is far more simple. The in-table in this case needs only two columns, `id` and `source`, corresponding to an event's `id` and `source` fields. The in-table will also have a compound primary key over both of these fields.

Time to implement. Our microservice will have a live subscription to our Stream `events`, and when an event is received, the algorithm of the event handler will be roughly as follows:

- Open a new transaction with our database.
- Attempt to write a new row to the in-table using the event's `id` and `source` fields as values for their respective columns.
    - If an error is returned indicating a primary key violation, then we know that we've already processed this event. Close the database transaction. Return a success response from the event handler. Done.
    - Else, if no error, then continue.
- Now time for business logic. Do whatever it is your microservice needs to do. Manipulate some rows in the database using the open transaction. Whatever.
- If your business logic needs to produce a new event as part of its business logic, then craft the new event, and write it to the out-table.
- Commit the database transaction. If errors take place, no worries. Just return the error from the event handler and a retry will take place.
- Finally, return a success from the event handler. Done!

For cases where a new event was generated as part of the business logic, the out-table is already setup to ship these events over to Hadron.

The code implementing this model can be found here: [examples/stream-transactional-processing](https://github.com/hadron-project/hadron/tree/v0.1.0-beta.0/examples/stream-transactional-processing).
<!-- TODO: ensure this tag is in place. -->

#### Pipeline Consumers
For Pipeline consumers, the in-table needs four columns, `id`, `source`, `stage` and `output`. The in-table will have a compound primary key over the columns `id`, `source` and `stage`.

Let's do this. Our microservice will have a Pipeline stage subscription to whatever Pipeline stages it should process, and when an event is received, the algorithm of the event handler will be roughly as follows:

- Open a new transaction with our database.
- Query the in-table using the tables primary key, where `id` is the root event's `id`, `source` is the root event's `source`, and `stage` is the name of the stage being processed.
    - If a row is returned, then we know that we've already processed this root event for this stage, and the row contains the output event which our Pipeline stage handler needs to return. Close the database transaction. Return the output event. Done.
    - Else, if no row is found, then this event has not yet been processed. Continue.
- Now time for business logic. Do whatever it is your microservice needs to do. Manipulate some rows in the database using the open transaction, use the root event or any of the dependency events of this Pipeline stage. Whatever.
- Pipeline stage handlers are required to return an output event indicating success. Construct a new output event. For simplicity, use the `id` of the root event and simply make the `source` something unique to this microservice's Pipeline stage.
- Using the open database transaction, write a new row to the in-table where `id` is the root event's `id`, `source` is the root event's `source`, `stage` is the name of the stage being processed, and `output` is the serialized bytes of our new output event.
- Commit the database transaction. If errors take place, no worries. Just return the error from the event handler and a retry will take place.
- Finally, return the output event from the event handler. Done!

With Pipelines, we are able to model all of the workflows of our applications, even spanning across multiple microservices, teams, and even system boundaries. Because Pipelines require an output event to be returned from stage consumers, we do not need an independent out-table process to ship these events over to Hadron.

The code implementing this model can be found here: [examples/pipeline-transactional-processing](https://github.com/hadron-project/hadron/tree/v0.1.0-beta.0/examples/pipeline-transactional-processing).
<!-- TODO: ensure this tag is in place. -->
