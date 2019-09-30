Discovery
=========
Discovery is what allows members to automatically join a cluster by way of network communication. This is often referred to as cluster formation, peer discovery, auto clustering &c.

Currently only DNS based peer discovery is implemented in this system. However, Railgun has been designed so that new discovery backends can be easily added in the future as needed.

When a node first comes online, the discovery system will be booted to monitor for peers. When new peers are discovered, the Railgun leader will propose a new cluster configuration to add the peer. This allows for dynamic cluster growth. When a cluster member goes offline and remains offline, the dead cluster member will be removed. This is called cluster auto-healing, and ensures that the consensus protocol is not blocked when too many nodes go offline without being removed from the cluster.

### kubernetes
When deploying Railgun in a Kubernetes environment, DNS setup is as simple as could be. Typically, everything will already be configured on the Kubernetes platform level, and containers will have their DNS configured properly. The only step which will need to be taken is to populate the `RG_DISCOVERY_BACKEND` environment variable with the value `"DNS"` and the environment variable `RG_DISCOVERY_DNS_NAME` with the appropriate DNS name so that Railgun cluster peers can be discovered and connected to.

Railgun should be deployed in Kubernetes using a stateful set. If a service is generated for the cluster, it may be any of the standard service types, including a headless service. The `RG_DISCOVERY_DNS_NAME` should be set as follows to ensure all peer IPs are properly resolved: `*.${statefulSetName}`. Typically this is all that will be needed for the DNS peer discovery system to work in Kubernetes.

### todo
- [ ] #15: Certificate checking during discovery & handshake protocol.
