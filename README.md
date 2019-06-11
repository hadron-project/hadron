railgun
=======
A distributed streaming and messaging platform written in Rust.

Railgun is built around three core primitives: ephemeral messaging, RPC messaging & durable streams.

Ephemeral messaging is topic based and supports all of the standard patterns such as pub/sub, fanout, queueing, consumer group load balancing and the like. RPC messaging offers request/response patterns for microservice architectures. Durable streams offer configurable durability, robust consumer patterns and also offer opt-in unique ID validation on messages for server enforced "exactly once" semantics. Railgun also features a few novel paradigms in this space, including pipelines and services. Railgun is secure by default with namespaces, users and roles for secure deployments.

Older AMQP style systems were great, but fall short of Kafka-like streaming capabilities, specifically message persistence. Kafka-like systems lack ephemeral & RPC messaging. More recent blends of the technologies, like Nats, offer a nice combination, but lack many of the core features needed from both domains. Railgun offers the best of all of these paradigms, adds new paradigms for more powerful architectures, and is shipped as a single binary, written in Rust, with blazing speed and safety.

Railgun also features a very simple wire protocol based on protocol buffers. Protobuf has more and more become a universal serialization language for data interchange. This choice was made to reduce the barrier of entry for building Railgun client libraries in various languages.

### getting started
Head over to the [getting started](https://railgun-rs.github.io/railgun/getting-started.html) page to launch into all things Railgun. Check out the [client libraries](https://railgun-rs.github.io/railgun/client-libraries.html) page for details on the official client libraries maintained by the Railgun team.

### development
Have a look at [DEVELOPMENT.md](https://gitlab.com/docql/railgun/blob/master/DEVELOPMENT.md) for more details on getting started with development.

### LICENSE
Unless otherwise noted, the Railgun source files are distributed under the Apache Version 2.0 license found in the LICENSE file.
