core
====
This is the core crate of the Railgun project.

This is where the various components of the system are assembled together to form a cohesive whole.

## components
### raft ctl
- Owns the raft objects and drives the raft in a loop.
- Based on tokio.
- Uses channels to communicate with and control other components of the system.
- Owns the storage component.

### network
- Owns the network interface.
- Handles connection requests and the overall lifecycle of sockets.

##### clients
- Client connection handshakes will create a new client connection which will be multiplexed by the network component.
- Server may be configured with a client key which will be used to establish secure end-to-end encrypted channel between clients and the cluster.
- Traffic can be left unencrypted between client and server. This is not recommended.

##### peers
- Peer connection handshakes will create a new peer connection which will be multiplexed by the network component.
- Server may be configured with a peer key which will be used to establish a secure end-to-end encrypted channel between peers in the cluster.
- If members of the cluster do not all have the same private key and certificate chain, then secure communications will fail and the cluster will not be able to commune.
- Traffic can be left unencrypted between peers. This is not recommended.

###### rbac
TODO: flesh this out. Needs to support simple provisioning patterns, as well as modifications while running.
- read, write, admin.
- access control may be disabled cluster wide (this is not recommended).
- roles are first applied per region. `*` will provide access to all regions.
- roles are then applied per stream. `*` will provide access to all streams in the region.

### discovery
- Owns the discovery interface.
- Is responsible for using the configured discovery protocol to monitor for peers and initiate clustering.
- Communicates with the network component to establish connections to peers and initiate clustering.

### storage
- Is owned by the raft ctl component.
- Implements the raft `storage` trait for persisting raft cluster information.
- Q: db per region?
- Q: raft group per region?
- Q: one global raft region? What about latency?
- Q: what about a passive protocol where regional masters coordinate placement?
    - This would mean that service discovery takes into account the regions a discovered node participates in.
    - The discovered node will only participate in and be registered as part of a raft for each region it participates in.

----
----
----

### prototyping plans
- [ ] build a simple network interface.
    - each node spun up within this process will be able to take client & server requests.
    - mutating requests should be forwarded to master.
- [ ] build a discovery interface. For now, it should only work by way of receiving client requests to add or remove nodes.
    - this will simply spawn a new thread with a configured node. It will attempt to join the cluster &c.
- [ ] build storage system.
    - we should be able to create new persistent streams.
    - we should be able to write new data to streams.
    - we should be able to read from streams.

Items to keep in mind during implementation.
- multi-raft setup. Each region gets its own raft. Global is the default region.
- discovery is about other peer hosts to communicate with, not necessarily raft groups.
- look into building new raft abstractions:
    - `Node` simply wraps a `RawNode` and handles all of the `has_ready` workflow lifecycle steps automatically.
    - wraps multiple `RawNode`s and groups them by a cluster ID (provides multi-raft support).
- may not want to invest much in raft groups quite yet. May want to contribute that back to raft crate first.


----
----
----

### masterless data | single master raft for clustering
- Standard single raft for clustering.
    - nodes are configured as being master eligible.
    - nodes are configured for which regions they can participate in.
- Data has no master. Everything is based on CRDTs.
    - operational synchronization will only be required between regional members.
    - many operations will require only majority sync for respective region. Eg, writing new event to stream, consumer group offset tracking.
    - fewer operations will require full sync. WHICH???
