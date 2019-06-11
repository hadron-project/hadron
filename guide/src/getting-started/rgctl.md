rgctl
=====
The Railgun Control CLI — rgctl.

The rgctl CLI is shipped with Railgun and is the primary mechanism for performing administrative operations on a Railgun cluster.

- Provides mechanism for creating new namespaces, users, and JWTs for client connections.
- A `create-new-service` API endpoint will be available which will create everything needed for getting started with a new service. It returns a JWT for immediate use by the new service.
