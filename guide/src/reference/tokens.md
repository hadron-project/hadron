Tokens
======
These docs are currently under construction.

<!--
Hadron uses a simple but effective permissions model. There are users and there are tokens. Users represent administrators of the Hadron cluster, while tokens represent access grants to specific Hadron resources for use in application code and automation tools.

There are a few different user roles:
- `Owner`: this user has full control over the cluster, all namespaces and all resources.
- `Admin`: admins are the same as owners, except that admin permissions may be revoked by other admins and owners, but admins can not revoked the permissions of an owner.
- `Viewer`: viewers are read-only. They may view any of ther resources of the cluster, but may not modify them.

Users are not allowed to directly use the resources of Hadron, that is what tokens are for. Tokens represent a set of permissions for the bearer of that token. The token ID is retained within Hadron and the token may be deleted, which revokes that token's access to the cluster. Tokens come in a few forms:
- `All`: a permissions grant on all resources in the system.
- `Namespaced`: a set of permissions granted on namespace scoped resources.
- `Metrics`: A permissions grant on only the cluster metrics system.

Namespace grants come in two different forms:
- `Full`: a grant of full permissons on the target namespace.
    - `namespace`: the namespace to which the grant pertains.
- `Limited`: a grant of limited access to specific resources within the target namespace.
    - `namespace`: the namespace to which the grant pertains.
    - `messaging`: an optional `pub/sub/all` enum value indicating access to the namespace's ephemeral messaging exchange.
    - `endpoints`: a list of endpoint permissions with the following structure:
        - `matcher`: the endpoint name matcher to use. May include a wildcard to match endpoint hierarchies. Same wildcard rules apply as described in the [ephemeral messaging chapter](./ephemeral-messaging.md).
        - `access`: a `pub/sub/all` enum value.
    - `streams`: a list of stream permissions with the following structure:
        - `matcher`: the stream name matcher to use. May include a wildcard to match streams hierarchically. Same wildcard rules apply as described in the ephemeral messaging chapter.
        - `access`: a `pub/sub/all` enum value.
    - `schema`: a boolean indicating if the token has permissions to modify the schema of the namespace.

In addition to the above:
- Hadron clusters are initialized with a default `root:root` user bearing the `Owner` role. It is expected that cluster admins will use the root credentials to initialize any other users for the system, and it is expected that the root password will be changed and stored securely or removed in favor of other credentials.
- User & token management is performed via the Hadron CLI. -->
