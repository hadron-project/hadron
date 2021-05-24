Namespaces
==========
Namespaces are the central unit of resource grouping and access control in Hadron.

## Schema
Namespaces are declared in YAML as part of the [Schema Management system](./schema.md). The schema for the `Namespace` object is as follows:

```yaml
## The kind of object being defined. In this case, a namespace.
kind: Namespace
## The name of the namespace. Each namespace must have a unique name.
name: required string
## A description of this namespace.
description: required string
```

### Details
- Namepsace names must conform to the DNS subdomain naming standards. 1-63 characters, `[-a-zA-Z0-9]`, may not start or end with a hyphen.
- Ephemeral Messaging Exchanges, RPC Endpoints, Streams & Pipelines all exist within namespaces.
- A Namespace always has exactly one Ephemeral Messaging Exchange, and may have any number of RPC Endpoints, Streams and Pipelines.
- Tokens are granted permissions on Namespaces which determine their level of access to those Namespaces and the resources they contain.
