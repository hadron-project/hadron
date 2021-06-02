# Kubernetes Native | Clustering
```
Author: Dodd
Status: InDesign
```

Hadron is the Kubernetes native and CloudEvents native distributed event streaming, event orchestration & messaging platform.

Key Advantages with this cluster design:
- Relentless pursuit of simplicity.
- All K8s native, using the K8s API as the universal API. Stand on the shoulders of giants. Don't reinvent the wheel.
- No distributed consensus overhead for read/write path to streams, pipelines &c.
- We get to leverage — often at no cost overhead for us — the innovation in the K8s ecosystem: policy, networking, distribution, common knowledge base &c.

## Hadron Cluster
Hadron clusters are deployed as StatefulSets. Hadron uses CRDs to define the various types of resources which are to be made available within a Hadron cluster for client use.

All Hadron clusters leverage K8s Leases for consensus & leadership rulings, and use K8s ServerSideApply (SSA) to ensure idempotent updates which do not overwrite during race conditions.

- When an a Hadron cluster replica has acquired its cluster's Lease, it then works to perform reconciliation tasks, other leadership tasks, and renews the lease for as long as possible.
- Other replicas will contend for lease acquisition, and will follow the same pattern as K8s core controllers: https://kubernetes.io/docs/reference/command-line-tools-reference/kube-controller-manager/ Lease checking will be based on lease expiration time plus randomized interval to reduce contention. This is a very low-overhead and efficient process.
- All resource updates from the leader will use SSA to avoid race conditions. Conflicts will result in a check-lock-check followed by a forced update when it is safe to do so — ensuring that updates do not fail indefinitely.
- All members of a Hadron cluster observe changes to the CRs which apply to their cluster, and react when applicable.
- Hadron Cluster CRs will be able to declare robust HPA targets which will allow for dense packing of objects when possible, as well as dynamic StatefulSet scaling when more partitions are needed.

## Operations
- Hadron clusters are deployed as StatefulSets along with various other CRs.
- Horizontally scaling write throughput for a cluster is as simple as scaling the number of partitions for the corresponding object (E.G., stream, pipeline) and ensuring that there are enough replicas of the cluster's StatefulSet for the partition to be scheduled.
- StatefulSets are created with zone information to allow the scheduler to optimize spread and data survivability. Multiple StatefulSets can be created as part of the same logical cluster and deployed in different zones for multi-zone high-availability and replication.
- Hadron cluster leaders are able to observe all pods of a multi-StatefulSet cluster based on a configured pod selector label matcher. This value is typically just the name of the cluster.
- For globally distributed operations, all data on streams and pipelines are isolated to the partition where they are published, with no concern over data consistency.
    - We are designing global data stream integration patterns with the MaterializedView system. See [RFC#007](./007-views.md).

## Cluster Resources
Streams, Pipelines, Exchanges, RPC Endpoints, MaterializedViews and any other future objects will be declared as K8s CRDs.

- Object CRs will declare selectors which will influence the Hadron Operator's scheduler to place objects on matching cluster pods. Selectors may include cluster, region, zone, &c.
- For now, let's stick with the most simple label selectors.
- Hadron can generate K8s Services with label selectors pointing to various pods matching resource assignments. May be useful for exchanges, endpoints, and maybe streams. This data will be updated as object leadership changes take place as well.
- Client metadata queries would literally be a read of the in-memory config in K8s, and can be sent to any node of a Hadron cluster.
- We do not destroy objects which are no longer declared in K8s config, instead we can represent them as stale objects which admins can remove.
- We can allow for objects to declare that they need to be replicated to other specific regions for survivability, this will also be based on labels in CRs.
- The Hadron Operator also functions as the Validating Admissions Webhook for Hadron CRs.

### K8s Services
K8s services are created for many Hadron objects to facilitate load balancing to the Hadron cluster pods which back the target objects. Services which need to target specific pods of a Hadron cluster StatefulSet will have their endpoints managed by the Hadron Operator.

- Hadron K8s services are always created in the K8s namespace in which the Hadron cluster is running, named as `{objectType}-{objectName}`. E.G., `pipeline-new-user`.
- Network policy may be added to Hadron K8s services, but Hadron does not add such network policy on its own.
- Hadron creates a service for accessing metrics which target all nodes of a Hadron cluster.
- Instead of clients needing to query for cluster metadata to know which pods to target, clients will simply target the service corresponding to the object they intend to interact with.

### Credentials
#secrets #users #tokens #serviceaccount #clusterrole #opa

- Hadron uses Tokens to declare a secret JWT to be minted and granted access to the Hadron API and its various resources.
- The corresponding generated tokens can be referenced by workloads within the K8s cluster seamlessly.
- Creation of all Hadron CRs can be limited to ServiceAccounts with various roles using native K8s resources, helping to ensure secure access to Hadron resources.
- JWT creation & verification use standard RSA keys. Administrators are recommended to use cert-manager for CA creation and generation of public and private keys for cluster use.

### Stream ISR
The ISR model is used by partition leaders to report their in-sync replicas.

- Partition leaders report this info to whichever Hadron Operator claims to be their leader.
- The Operator makes idempotent and atomic changes to CRs for assignments, and all other nodes just react to these changes.
- Partition leadership changes will be made in such a way that entry commitment will uphold cluster consensus requirements. Cluster leader can use two-phase updates to nodes & CRs to ensure an inoperable partition doesn't enter into an invalid state, potentially marking a partition as unavailable when a new leader can not be nominated.
- When a stream partition leader dies, the Operator leader will make an epoch-stamped update to the partition's leader assignment, and all Hadron worker nodes will observe this change, and can reject replication for an old leader which is supposed to be dead.

### Multi-Region
- Multi-region will continue to work exactly as expected, and in a K8s native manner. CRs can be federated, and updated to select "regional" pods as "regional" partitions &c.
- Publishers can select on "regions" to optimize write path.

### Use Case: Global RPC Endpoints & Exchanges:
- DNS-based networking will send traffic to local regional LB (using cloud provider Global LB or the like).
- Ingresses can be used to map RPC or Exchange traffic to a provisioned Hadron service in K8s (handles leadership via labels).
- RPC or Exchange consumers within the backend Hadron cluster can then handle the inbound connections/requests.
- Consumers from other clusters could establish their own connections to consume cross-region data if needed.
