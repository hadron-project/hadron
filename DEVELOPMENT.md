development
===========
Railgun is implemented in Rust. This system is built upon the [Actix](https://actix.rs) actor framework, which in turn is built upon the [Tokio](https://tokio.rs/) asynchronous runtime. Leveraging the actor pattern with message passing allows us to completely avoid locking in the Railgun application code, leverage Rust’s ownership system to its maximum potential, and the asynchrounous foundation keeps resource usage minimal while still providing very high throughput.

Railgun uses the Rust stable channel. Get started with development by [installing `rustup`](https://rustup.rs/) on your system. Once `rustup` has been successfully installed, execute `rustup component add clippy`. The `clippy` component is used to "catch common mistakes and improve your Rust code". Editor and IDE integrations are widely available for all of these tools.

Building Railgun is as simple as `cargo build`, append `--release` for an optimized build.

All testing and CI is built around the docker ecosystem. We're using [MicroK8s](https://microk8s.io/) for CI testing related to Kubernets, as well as [MiniKube](https://github.com/kubernetes/minikube) for local development and testing.
