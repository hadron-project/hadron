### todo
main: big DB CG overhaul

- [ ] refactor setup & usage of CFs
- [ ] given that storage initialization may take some time, pass a signal emitter down to the storage engine so that it can tell the rest of the app when initialization has actually finished.
    - [ ] the network layer should refuse to perform peer handshakes and refuse client connections until the system is ready.
- [ ] update permissions system to have namespaces once again
    - [ ] admins are always for the full system, same with viewers. They span all namespaces.
    - [ ] token permissions will be updated to be namespace specific as they were
    - [ ] pipelines may span any number of namespaces. The consumer of the stage, which is the entity which actually reads and writes data on the streams involved, is the component which must have proper permissions. As such, token permissions will be fully vetted for the stage the token is attempting to consume.
    - [ ] update guide
- [ ] basically ready to build stream subscriptions.
- [ ] build dynamic membership system, most everything is in place.
- [ ] ensure delays are set on raft requests when a peer channel has been disconnected.
- [ ] finish up tests on DDL.
    - [ ] add namespace DDL (the "default" namespace is always present and can not be removed)
    - [ ] validate namespace names
    - [ ] perform cycle tests to ensure stage `after` & `dependencies` constraints do not form cycles in the graph

---

- [ ] look into using rockdb's TTL compaction system to be able to remove "application keys" used for a stream. This pattern would allow for disk uniquness checking during stream publication to help guard against duplicates.
- [ ] open issue for creating initial streams for
    - CRUD on objects in the system
    - stream for metrics
- [ ] open an issue on future integration with Vault as a token provider.
- [ ] open issue for having admin UI setup with OAuth handler so that orgs can grant viewer permissions to anyone in their org.
- [x] combine all internal error types to a single type for more uniform handling.
