Namespaces
==========
Namespaces are the central unit of resource grouping and access control.

- Ephemeral messaging, RPC endpoints, streams, pipelines & services all exist within namespaces.
- A namespace always has exactly one ephemeral messaging exchange, and may have any number of durable streams.
- Users are granted roles on namespaces which determine their level of access to those namespaces.
- Railgun clusters are initialized with the `default` namespace. It is a normal namespace.
- The `rgctl` CLI is used for creating new namespaces.
