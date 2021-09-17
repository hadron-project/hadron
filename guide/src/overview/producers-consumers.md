Producers & Consumers
=====================

Hadron clients which produce and publish data to a Stream, an Exchange or an Endpoint are considered to be Producers. Hadron clients which consume data from a Stream, an Exchange or an Endpoint are considered to be Consumers.

Producers and Consumers establish durable long-lived connections to backend components in the target Hadron cluster, which helps to avoid unnecessary setup and teardown of network connections.

Producers and Consumers typically exist as user defined code within larger applications. However, they may also exist as open source projects which run independently based on runtime configuration, acting as standalone components, often times both producing and consuming data. The latter are typically referred to as Connectors.

Producers and Consumers may be created in any language. The Hadron team maintains a common Rust client which is used as the shared foundation for clients written in other languages, which provides maximum performance and safety across the ecosystem. The Hadron team also maintains the Hadron CLI, which is based upon the Rust client and which can be used for basic production and consumption of data from Hadron.

Producer and Consumer behavior varies depending on where data is being published to or consumed from, whether for Streams, Exchanges or for RPC Endpoints. Each respective section of the guide contains further details on these differences.
