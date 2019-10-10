Users & Tokens
==============
Railgun uses a simple but effective permissions model. There are users and there are tokens. Users represent administrators and viewers of the Railgun cluster, while tokens represent consumers of the Railgun services.

There are three different user roles:
- `root`: this user has full control over the cluster, all namespaces and all resources.
- `admin`: admins are limited to only specific namespaces, but may manage all resources of those namespaces, may grant other users permissions on those namespaces, and may create tokens for resources of those namespaces.

Users are not able to directly use the resources of Railgun, that is what tokens are for. Tokens represent a static set of permissions for the bearer of that token. The token ID is retained within Railgun and the token may be deleted, which revokes that tokens access to the cluster. Token permission claims are represented as three different types:
- `Root`: a root token claim which represents full access to the cluster's resources.
- `Metrics`: a metrics token claim which represents access only to the cluster monitoring system.
- `Namespaces`: a namespaces token claim which represents a set of namespace specific grants.

Namespace grants are modelled as two different types:
- `Full`: a grant of full permissons on the target namespace.
    - `namespace`: the namespace which the grant pertains to.
- `Limited`: a grant of limited access to specific resources within the target namespace.
    - `namespace`: the namespace which the grant pertains to.
    - `can_create`: a boolean value. If `true`, this token can be used to create new endpoints, streams and pipelines in the associated namespace.
    - `messaging`: a `read/write/none` enum value.
    - `endpoints`: a list of endpoint permissions with the following structure:
        - `matcher`: the endpoint name matcher to use. May include a wildcard to match endpoints hierarchically. Same wildcard rules apply as described in the ephemeral messaging chapter.
        - `access`: a `read/write/both` enum value.
    - `streams`: a list of stream permissions with the following structure:
        - `matcher`: the stream name matcher to use. May include a wildcard to match streams hierarchically. Same wildcard rules apply as described in the ephemeral messaging chapter.
        - `access`: a `read/write/both` enum value.

In addition to the above:
- Railgun caches the results of permissions checks in-memory on the actors which handle client connections. This keeps permissions logic snappy.
- Railgun clusters are initialized with a configurable default user which will start with the `root` role.
- A user's permissions are determined at connection time and are used throughout the lifetime of a connection. Changes to a user's permissions may cause a connection to terminate if the connection no longer has sufficient permissions after the update.
- User & token management is performed via the `rgctl` CLI which ships with Railgun.
