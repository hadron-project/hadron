railgun
=======
A distributed streaming and messaging platform written in Rust.

Railgun's mission statement is to be the singular platform for all streaming and messaging needs, to advance the state of the art in exactly-once idempotent stream processing, all to provide developers a better tool to reduce error handling and complexity in building distributed systems. Railgun altogether replaces other messaging and streaming platforms, removes the need for other intra-service communication protocols like gRPC or REST, and inherently acts as a service discovery mechanism.

Railgun is built around three core primitives: ephemeral messaging, RPC messaging & durable streams. Ephemeral messaging offers topic based publish/subscribe patterns with wildcards, fanout, load balancing queue groups, and more. RPC messaging offers request/response patterns for service-oriented architectures. Durable streams offer configurable durability, robust consumer patterns and also offer opt-in unique ID validation on messages for server enforced "exactly once" semantics.

Railgun builds upon these core primitives to offer piplines. Pipelines are pre-defined multi-stage data workflows, structured in terms of a graph. Pipelines provide transactional guarantees over ack'ing a stages work and the delivery of the next stages data. With Pipelines, Railgun provides a platform with greater guarantees, and reduced error handling for building streaming-first service-oriented architectures.

Older AMQP style systems were great, but fall short of Kafka-like streaming capabilities, specifically message persistence. Kafka-like systems lack ephemeral & RPC messaging. More recent blends of the technologies, like Nats, offer a nice combination, but lack many of the core features needed from both domains. Railgun offers the best of all of these paradigms, adds new paradigms for more powerful architectures, and is shipped as a single binary, written in Rust, with blazing speed and safety.

Railgun also features a very simple wire protocol based on protocol buffers. Protobuf has more and more become a universal serialization language for data interchange. This choice was made to reduce the barrier of entry for building Railgun client libraries in various languages.

### getting started
Head over to the [getting started](https://railgun-rs.github.io/railgun/getting-started.html) page to launch into all things Railgun. Check out the [client libraries](https://railgun-rs.github.io/railgun/client-libraries.html) page for details on the official client libraries maintained by the Railgun team.

### development
Have a look at [DEVELOPMENT.md](https://gitlab.com/docql/railgun/blob/master/DEVELOPMENT.md) for more details on getting started with development.

### LICENSE
Unless otherwise noted, the Railgun source files are distributed under the Apache Version 2.0 license found in the LICENSE file.
