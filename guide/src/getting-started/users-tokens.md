Users & Tokens
==============
Hadron uses a simple but effective permissions model. There are users and there are tokens. Users represent administrators of the Hadron cluster, while tokens represent access grants to specific Hadron resources.

There are two different user roles:
- `root`: this user has full control over the cluster, all namespaces and all resources.
- `admin`: admins are limited to only specific namespaces, but may manage all resources of those namespaces, may grant other users permissions on those namespaces, and may create tokens for resources of those namespaces.

Users are not allowed to directly use the resources of Hadron, that is what tokens are for. Tokens represent a static set of permissions for the bearer of that token. The token ID is retained within Hadron and the token may be deleted, which revokes that token's access to the cluster. Tokens come in two forms:
- `Namespace`: a namespace token claim which represents a set of namespace specific permissions.
- `Metrics`: a metrics token claim which represents access only to the cluster monitoring system.

Namespace grants come in two different forms:
- `Full`: a grant of full permissons on the target namespace.
    - `namespace`: the namespace to which the grant pertains.
- `Limited`: a grant of limited access to specific resources within the target namespace.
    - `namespace`: the namespace to which the grant pertains.
    - `can_create`: a boolean value. If `true`, this token can be used to create new endpoints, streams and pipelines in the associated namespace.
    - `messaging`: an optional `pub/sub/all` enum value.
    - `endpoints`: a list of endpoint permissions with the following structure:
        - `matcher`: the endpoint name matcher to use. May include a wildcard to match endpoints hierarchically. Same wildcard rules apply as described in the ephemeral messaging chapter.
        - `access`: a `pub/sub/all` enum value.
    - `streams`: a list of stream permissions with the following structure:
        - `matcher`: the stream name matcher to use. May include a wildcard to match streams hierarchically. Same wildcard rules apply as described in the ephemeral messaging chapter.
        - `access`: a `pub/sub/all` enum value.

In addition to the above:
- Hadron clusters are initialized with a default `root:root` user bearing the `root` role. It is expected that cluster admins will use the root credentials to initialize any other users for the system, and it is expected that the root password will be changed and stored securely.
- User & token management is performed via the `hadctl` CLI which ships with Hadron.
