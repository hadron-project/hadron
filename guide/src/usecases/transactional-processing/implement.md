Implement Transactional Processing
==================================

<small><i>This sub-chapter builds upon the ideas established in the previous sub-chapter [What is Transactional Processing](./learn.md) and it is recommended to have a firm understanding of that content before continuing here.</i></small>

Armed with the knowledge of event identity, we are now ready to implement our own transactional processing system.

For this use case we will make the following starting assumptions:
- We are using an RDBMS (like PostgreSQL or CockroachDB) application state.
- We are using the [`OutTableIdentity` pattern](./learn.md#establishing-identity) to transactionally generate our events.
- We are using a microservices model, so we have multiple out-tables each in its own database schema (think namespace except for a database) owned by a different microservice.
- Very importantly, each out-table assigns a different `source` value to its generated events corresponding to the microservice's name.
- Every event published to our Hadron Stream is coming from one of our microservice out-tables.

What properties does this model have?
- Every event on our Stream will have a guaranteed unique identity based on the combination of the `id` and `source` fields.
- Duplicates may still exist, but we know they are duplicates based on the `id` and `source` field combination. Our implementation below will easily deal with such duplicates.
- We can seamlessly add new microservices to our system following this same pattern, and our uniqueness properties will still hold.

This is not the only way to implement a pattern like this, but it is a great stepping stone which can be adapted to your specific use cases.

### Transactional Consumers
Given our starting assumptions and the properties of our model, we are ready to implement a transactional processing algorithm for our consumers.
