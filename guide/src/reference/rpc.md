These docs are currently under construction.
<!-- RPC Endpoints
=============
RPC Endpoints provide a service-oriented request/response system, akin to traditional REST APIs or other RPC systems like gRPC.

## Schema
RPC Endpoints are declared in YAML as part of the [Schema Management system](./schema.md). The schema for the `Endpoint` object is as follows:

```yaml
## The kind of object being defined. In this case, a pipeline.
kind: Endpoint
## The namespace in which this endpoint is to be created.
namespace: required string
## The name of the endpoint. Each endpoint must have a unique name per namespace.
name: required string
## The input RPC mode.
input: enum Single | Stream
## The output RPC mode.
output: enum Single | Stream
```

### Details
- Endpoint names may be 1-100 characters long, containing only `[-_.a-zA-Z0-9]`. The `.` can be used to form hierarchies for authorization matching wildcards. Consumers do not use wildcards for endpoints.
- RPC messages are published to a specific endpoint within a namespace.
- RPC endpoints are declared explicity in code and must be created on the server.
- RPC messages are not durable, but if an endpoint has no live consumers when a message is published, an error response will be immediately returned for better control flow.

## Consumers
RPC Endpoints offer consumer patterns similar to the ephemeral messaging system, except that wildcards are not allowed. -->
