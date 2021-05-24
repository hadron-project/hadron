# Clustering
```
Author: Dodd
Status: InDesign
```

Hadron clusters will be composed of multiple replica sets, where each replica set represents a logical "partition" for streams & pipelines. Replica sets communicate with each other via a standard discovery system. A cluster must have a single replica set configured as the metadata replica set. HA is reckoned in terms of stream & pipeline availability, not in terms of the availability of a single replica set.

Each replica set is statically configured with exactly one master and any number of replicas. Roles are established statically in config.

- Config changes require restarts. When a node first comes online, it will not fully start until it is able to establish a connection with the members of its replica set & determine that their config matches. This is treated as the handshake. Config to match:
    - replica set name
    - node names
    - role per node
- If handshake fails, backoff and try again. Warn about the potential misconfiguration issue.
- It is recommended that replica set names semantically indicate geo and or cloud infomration, as well as purpose. Though the only hard requirement is that replica set names be unique throughout a cluster.
    - An example of such a partition naming scheme could be: `aws.us-east-1/p0`, `aws.us-east-1/p1`, `gcp.asia-southeast1/p0`, `gcp.asia-southeast1/p1`.
    - In this example, replica sets span two different clouds, and two different regions.

## Consensus
Consensus within a replica set is dead simple, and is based on static config. The leader is always known and doesn't change. An entry is committed based on majority replication.

## Cluster
Clusters are composed of multiple replica sets which dynamically discover each other.

- Each replica set may be part of only one cluster. If the cluster discovery info & ports match, then the replica sets will form a cluster. Certificates are exchanged for clustering security.
- A single replica set may be statically configured as the metadata group. This group will then be responsible for all cluster wide metadata changes, such as stream and pipeline definitions.
- HA is achieved by having multiple partitions assigned to the same stream. Clients are able to detect when a partition is down, and so they will failover to one which is alive.

## Metadata
- A single replica set acts as the metadata leader for a cluster based on static config.
- Metadata is asynchronously replicated to all members of the cluster, always with an associated metadata index.
    - This allows for a cluster to more easily span the globe, multiple regions and differnet clouds.
    - Clients can query for metadata at any location, and they will always be as up-to-date as a direct query to metadata replica set directly, due to the constant async replication of config.
- Hadron schema objects declare their "partition" assignments explicitly.
    - For example, when a stream is declared, it will declare its partitions as a list of replica set names.
    - It could declare that the stream has 4 partitions, as follows: `aws.us-east-1/p0`, `aws.us-east-1/p1`, `gcp.asia-southeast1/p0`, `gcp.asia-southeast1/p1`.
    - These would match replica set names of actual replica sets in the cluster.
    - As clients publish data to these partitions, they can hash to a specific partition of the region where their data needs to reside, or hard code the partition they are targetting.
    - This supports cases where data semantically belongs on the same stream, but is optimized to reduce latency by geolocating replica sets in the regions where the data is being processed, GDPR or similar requirements.
    - The cluster will still see all of these partitions as participating in the same stream.

## Stream Subscribers
- Clients will create stream subscriber channels on all partitions of a target stream.
- Clients are always part of "group", and each partition will load balance deliveries across all active members of a group.
- A client may receive multiple concurrent deliveries from different partitions. Concurrent consumption rate is configurable.

## Stream Publishers
- Producers will be able to specify durability of payloads, and the async replication system will report back as batches are replicated.
- If there are no replicas of a replica set, then we will fsync each batch written to disk. If there are replicas, then "durable" writes means that a majority of the replica set has the data on disk, and no flush required on write path.
- Writes should be batched across clients for better efficiency, and will be applied to disk using a durable batch. This is actually exactly how the sled periodic fsync behavior will be once we introduce the replication system.
