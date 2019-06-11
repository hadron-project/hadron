RPC Messaging
=============
RPC messaging is a service-oriented request/response system, akin to traditional REST APIs or other RPC systems like gRPC.

- RPC messaging is understood in terms of "endpoints".
- RPC messages are published to a specific endpoint within a namespace.
- RPC endpoints are declared explicity in code and must be created on the server.
- RPC messages are not durable, but if an endpoint being published to has no live consumers, an error response will be immediately returned for better control flow.
- Railgun offers RPC request/response messaging as a standalone feature as it is such a common need.
- Keeping RPC messaging as a distinct feature in Railgun offers more avenues for innovation and optimization. Control flow is more explicit, and the code which needs to be written on both ends of the RPC workflow can remain more clear and concise.
- RPC responses do not need to be immediately returned, and the data of an RPC request may be passed along to other Railgun streams and pipelines before returning a response. This offers a great deal of flexibility for server-side workflows.

Railgun's `Services` feature is built upon the RPC endpoints system, and offers very powerful patterns for streaming-first microservice architectures to be built within Railgun. See the [services chapter](https://railgun-rs.github.io/railgun/getting-started/services.html) for more details.
