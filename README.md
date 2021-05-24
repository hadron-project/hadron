Hadron
======
The Hadron distributed event streaming, event orchestration & messaging platform.

Hadron is built around three core primitives: ephemeral messaging, RPC messaging & durable streams. Ephemeral messaging offers topic based publish/subscribe patterns with wildcards, fanout, load balancing queue groups, and more. RPC messaging offers request/response patterns for service-oriented architectures. Durable streams offer configurable durability, and robust consumer patterns.

Hadron builds upon these core primitives to offer piplines. Pipelines are pre-defined multi-stage data workflows, structured as a graph. Pipelines provide transactional guarantees over ack'ing a stage's work and the delivery of the next stage's data. With Pipelines, Hadron provides a platform with better features, and greater guarantees for building distributed architectures.

Older AMQP style systems were great, but fall short of Kafka-like streaming capabilities, specifically message persistence. Kafka-like systems lack ephemeral & RPC messaging. More recent blends of the technologies, offer a nice combination, but lack many of the core features needed from both domains. Hadron offers the best of all of these paradigms, adds Pipelines for more powerful architectures, and is shipped as a single binary, written in Rust, with blazing speed, safety, and reliability.

Hadron also features a very simple wire protocol based on protocol buffers for client-server communication, but also ships with a powerful Rust client library for native use in Rust, or for use in any other language supporting C FFI. Language specific wrappers are (will be) maintained in various languages to smooth out any FFI roughness.

### Learn
Head over to the [Hadron Guide](https://hadron-project.github.io/hadron/) to learn more.

### LICENSE
Unless otherwise noted, the Hadron source files are distributed under the Apache Version 2.0 license found in the LICENSE file.
