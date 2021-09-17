Introduction
============
Hadron is a distributed data storage system designed to rapidly ingest large amounts of data, and to facilitate working with that data in the form of arbitrarily complex structured workflows.

Building distributed applications can be tough. Teams might have tens, hundreds or even thousands of microservices. Platforms may have thousands of data signals ranging from business critical application events, to telemetry signals including logs, tracing, metrics and the like. All of this data is important, and now more than ever teams need a way to not only capture this data, but also to work with this data in a scalable and extensible way.

Hadron offers a powerful solution to these problems using the following primitives:

- **Events** - all data going into and coming out of Hadron is structured in the form of events.
- **Streams** - durable logs for storing arbitrary data, with absolute ordering and horizontal scalability.
- **Pipelines** - workflow orchestration for data on Streams, providing structured concurrency for arbitrarily complex multi-stage tasks.
- **Exchanges** - ephemeral messaging hubs used to exchange non-durable events between processes, perfect for GraphQL Subscriptions, WebSockets, Push Notifications and the like.
- **Endpoints** - general-purpose RPC handlers for leveraging Hadron's powerful networking capabilities.
- **Producers** - client processes connected to Hadron, written in any language, working to produce and publish data to Hadron.
- **Consumers** - client processes connected to Hadron, written in any language, working to consume data from Streams, process Pipeline stages, consume ephemeral messages from Exchanges, or even handle RPC Endpoints.

Hadron was born into the world of Kubernetes, and Kubernetes is a core assumption in the Hadron operational model. See the next section for more details.

For a use case demonstrating the power of these components working together, head on over to [Use Case: Service Provisioning](./usecases/service-provisioning.md).
