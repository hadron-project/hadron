Namespaces
==========
Namespaces are the central unit of resource grouping and access control.

- Namepsace names must conform to the DNS subdomain naming standards. 1-63 characters, `[-a-zA-Z0-9]`, may not start or end with a hyphen.
- Ephemeral messaging, RPC endpoints, streams & pipelines all exist within namespaces.
- A namespace always has exactly one ephemeral messaging exchange, and may have any number of endpoints, streams and pipelines.
- Users are granted roles on namespaces which determine their level of access to those namespaces.
- Railgun clusters are initialized with the `default` namespace. It is a normal namespace.
- The `rgctl` CLI is used for creating new namespaces.
