railgun
=======
A distributed streaming and messaging platform written in Rust.

The railgun platform offers ephemeral message passing and message routing patterns, request/response messaging, pub/sub and consumer group patterns, as well as persistent message streams with built-in deduplication capabilities for more powerful "exactly once" semantics.

Railgun is distributed as a single binary, and can be easily setup and deployed on bare metal servers, VMs and containers. Deploying a Railgun StatefulSet to a Kubernetes cluster using the official Helm chart is as simple as it gets.

### docs
- [Overview](./docs/README.md#overview)
- [Persistent Streams](./docs/README.md#persistent-streams)
- [Ephemeral Messaging](./docs/README.md#ephemeral-messaging)
- [Clustering](./docs/README.md#clustering)
- [Data Replication and Sharding](./docs/README.md#data-replication-and-sharding)
- [Internals](./docs/internals/README.md)
- [Operations](./docs/operations/README.md)

### development
Railgun is implemented in Rust. This system is built upon the [Actix](https://actix.rs) actor framework, which in turn is built upon the [Tokio](https://tokio.rs/) asynchronous runtime. Leveraging the actor pattern with message passing allows us to completely avoid locking in the Railgun application code, leverage Rust’s ownership system to its maximum potential, and the asynchrounous foundation keeps resource usage minimal while still providing very high throughput.

Railgun uses the Rust stable channel. Get started with development by [installing `rustup`](https://rustup.rs/) on your system. Once `rustup` has been successfully installed, execute `rustup component add clippy rustfmt`. The `clippy` and `rustfmt` components are used for consistent code style and formatting. Editor and IDE integrations are widely available for all of these tools.

Building Railgun is as simple as `cargo build`, append `--release` for an optimized build.

All testing and CI is built around the docker ecosystem. We're using [MicroK8s](https://microk8s.io/) for CI testing related to Kubernets, as well as [MiniKube](https://github.com/kubernetes/minikube) for local development and testing.

----

## LICENSE
Unless otherwise noted, the Railgun source files are distributed under the Apache Version 2.0 license found in the LICENSE file.
