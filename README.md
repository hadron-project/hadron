Hadron
======
The Hadron distributed event streaming, event coordination & messaging platform.

Hadron's mission statement is to be the singular platform for all streaming and messaging needs, to advance the state of the art in idempotent stream processing, all to provide developers a better tool to reduce error handling and complexity in building distributed systems. Hadron altogether replaces other messaging and streaming platforms, removes the need for other intra-service communication protocols like gRPC or REST, and inherently acts as a service discovery mechanism.

Hadron is built around three core primitives: ephemeral messaging, RPC messaging & durable streams. Ephemeral messaging offers topic based publish/subscribe patterns with wildcards, fanout, load balancing queue groups, and more. RPC messaging offers request/response patterns for service-oriented architectures. Durable streams offer configurable durability, and robust consumer patterns.

Hadron builds upon these core primitives to offer piplines. Pipelines are pre-defined multi-stage data workflows, structured in terms of a graph. Pipelines provide transactional guarantees over ack'ing a stage's work and the delivery of the next stage's data. With Pipelines, Hadron provides a platform with greater guarantees, and reduced error handling for building streaming-first service-oriented architectures.

Older AMQP style systems were great, but fall short of Kafka-like streaming capabilities, specifically message persistence. Kafka-like systems lack ephemeral & RPC messaging. More recent blends of the technologies, like Nats, offer a nice combination, but lack many of the core features needed from both domains. Hadron offers the best of all of these paradigms, adds new paradigms for more powerful architectures, and is shipped as a single binary, written in Rust, with blazing speed, safety, and reliability.

Hadron also features a very simple wire protocol based on protocol buffers for client-server communication, but also ships with a powerful Rust client library for native use in Rust, or for use in any other language supporting C FFI. Language specific wrappers are maintained in various languages to smooth out any FFI roughness.

### getting started
Head over to the [getting started](https://hadron-project.github.io/hadron/getting-started.html) page to launch into all things Hadron. Check out the [client libraries](https://hadron-project.github.io/hadron/client-libraries.html) page for details on the official client libraries maintained by the Hadron team.

### development
Have a look at [DEVELOPMENT.md](https://gitlab.com/docql/hadron/blob/master/DEVELOPMENT.md) for more details on getting started with development.

### LICENSE
Unless otherwise noted, the Hadron source files are distributed under the Apache Version 2.0 license found in the LICENSE file.
