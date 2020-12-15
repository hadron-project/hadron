development
===========
Railgun is implemented in Rust. This system is built upon the [Actix](https://actix.rs) actor framework, which in turn is built upon the [Tokio](https://tokio.rs/) asynchronous runtime. Leveraging the actor pattern with message passing allows us to completely avoid locking in the Railgun application code, leverage Rust’s ownership system to its maximum potential, and the asynchrounous foundation keeps resource usage minimal while still providing very high throughput.

Railgun uses the Rust stable channel. Get started with development by [installing `rustup`](https://rustup.rs/) on your system. Once `rustup` has been successfully installed, execute `rustup component add clippy`. The `clippy` component is used to "catch common mistakes and improve your Rust code". Editor and IDE integrations are widely available for all of these tools. We are also using `rustfmt`. Both `clippy` & `rustfmt` have solid editor inntegrations.

### just build
Building Railgun is as simple as `cargo build`, append `--release` for an optimized build. However, we are using the [just command runner](https://github.com/casey/just) which allows us to keep track of common commands in the `Justfile`. Think `make`, but better. Install just via `cargo install just`.

```bash
# List available commands and thier docs.
just -l

# Execute a specific command.
just COMMAND
```

### thoughts on scale
What do we want from scale?

- to be able to store more and more data, and continue to do so by just adding more nodes.
- to be able to increase throughput of request processing.
