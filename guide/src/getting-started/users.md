Users
=====
Users are username + password entries which may be granted roles.

- Users may have different roles:
    - `root`: full control over the cluster & all namespaces.
    - `read`: may only read resources of the associated namespace.
    - `write`: same as `read`, but may also write to all resources of the associated namespace.
    - `admin`: same as `write`, but may also create and delete any resources of the associated namespace. Other than the `root` role, this is the only other role allowed to use the `rgctl` CLI.
- Railgun clusters are initialized with a configurable default user which will start with the `root` role.
- A user's permissions are determined at connection time and are used throughout the lifetime of a connection. Changes to a user's permissions may cause a connection to terminate if the connection no longer has sufficient permissions after the update.
- User management is performed via the `rgctl` CLI which ships with Railgun.
- Users may only connect to a Railgun cluster using JWTs issued by Railgun.
- The `rgctl` CLI is used to issue JWTs. This is one extra step which needs to be taken to get started, but it ensures a more secure system.

### permissions updates
If a user is connected to a Railgun cluster and their permissions are updated:
- If the change is additive, the connection's permissions will be updated in place. No reconnect needed.
- If the change is negative, any operations which were being performed which violate the new permissions will cause the operation to stop and return an error.
- If the change has removed the user, the connection will be terminated.
