# CPC - Cluster Placement Controller
## Overview
This controller is responsible for tracking all live objects in the system which are in some way distributed across the cluster. Streams, Pipelines, Endpoints and other such objects all all have their own controllers which run on specific nodes of the cluster and work together to accomplish their overall goal.

This controller is responsible for determining exactly where on the cluster those controllers are to run, and acts as a Raft client, sending various placement decision making requests to CRC in order to ensure it is known and properly replicated across the cluster.

This controller will only perform mutating actions when it is running on the same node as the CRC leader (the Raft leader) — when it is the leader. Otherwise it will be completely passive and never forwards placement commands to other nodes.

## Leadership
This controller is responsible for placement of objects & other controllers within the cluster, and uses the Raft state machine as a command bus in order to do so.

When a CPC instance is on the leader node, it will assume the responsibility of issuing commands to the rest of the cluster based on the state within the Raft state machine.

This ensures that we do not end up with nodes attempting to take actions on commands which are indeed in the state machine, but which are old and which no longer apply. This could happen when a node has become disconnected from the cluster but still has outstanding tasks to apply.

Instead, we use the approach of having the CPC leader issue commands. It passes along the Raft term and its node ID, and other nodes simply verify that it is indeed the cluster leader. This guards against any processing of stale commands as only the leader will process commands, and in order to become leader, a node must be up-to-date.

## Placement
Placement is driven from data held by the Raft state machine. As Streams, Pipelines and other objects are created, data records describing their various distributed components (known as controllers) are also created. They are initially created without any placement details.

The CPC will observe these data records, and reconcile their state against the overall goal of the CPC, which is to have all objects of the cluster distributed evenly and in a well-balanced fashion. As such, unassigned objects will be assigned to nodes of the cluster based on various heuristics (detailed below). The assignments will be committed to the cluster via Raft, and the requests will not be forwarded, which ensures that only the CPC on the Raft leader node is able to make changes to the cluster.

When the CPC detects objects with an aspired state, it will spawn tasks to drive that aspired state to completion. This is done by issuing network commands to other nodes of the cluster, or by issuing local commands to the node on which the CPC is running if the changes apply to its own node. Once the work has completed successfully, the CPC will update the records via Raft. Upon successfully updating the aspired state to the current state, the reconciliation process for that object will then be complete.

Disruptions to a reconciliation task will simply be restarted. If a CPC group leader is deposed, it will observe such a change via Raft metrics, via a response from another peer, or it will have died and recovered nodes never start as Raft leaders.

## Placement Heuristics
For the Hadron beta, the only heuristic used to drive the placement algorithm is simple load balancing by object type. The CPC will attempt to distribute objects evenly across the cluster by object type.

## Control Groups
The CPC is responsible for creating Control Group (CG) records and assigning controllers to groups. This differs based on the type of controller. For Streams, CGs are formed per partition. This is also true for Pipelines, except that Pipelines only ever have 1 partition.

When control groups are first formed, they are able to initialize their Raft groups based purely on the CG membership info from which they were created. As the replication factor for Streams, Pipelines, and other objects, is increased, the CPC will detect that the new object is not yet part of a CG and it will start a new reconcilitation task to add it to the CG.

In order to add the new object to its CG, the CPC will commit an update to the CG record and the object record indicating the new CG assignment, and it will then issue a request to the CG Raft leader to add the new member. Once that task has completed successfully, it will issue a final update to the CG record indicating that the new member has been integrated into the cluster.

Disruptions to this reconciliation workflow will simply cause a new reconciliation task to be started, and the process of adding a member to a Raft cluster is idempotent, so it will eventually succeed and be committed to the cluster Raft, finalizing the reconciliation workflow.

## Multi-Raft Scalability Concerns
For any data replication protocol, replicating data is fundamentally required and not considered as overhead. Raft's heartbeat protocol however is considered overhead, as empty RPC frames are used, and when scaling to tens of thousands of Raft groups, this can result in a very large amount of constant network overhead. Hadron uses HTTP2 for all network activity, and HTTP2 stream multiplexing does help to reduce the overhead of replication and heartbeats quite a lot.

The primary reason that multi-Raft would be used in the first place is to mitigate the bottleneck of a single cluster-wide Raft, as it would not be able to handle the concurrent throughput of tens of thousands of Streams and its performance would degrade as the number of active Streams increases. With multi-Raft, we are able to mitigate this bottleneck.

The tradeoff is the overhead of Raft's heartbeats. This can be mostly mitigated by configuring Raft groups with lower heartbeat frequencies, perhaps on the order of every 10 seconds or so. This would come at the cost of leadership elections taking longer.

For active CGs, there will be no heartbeat overhead, as our Raft implementation will not send extra heartbeats when data replication is covering heartbeat windows.

### Exploratory Mitigations of Heartbeat Overhead
#### Cross-Group Batching
This one is a bit tricky because the success of a heartbeat batch depends upon the success of the heartbeat of all Raft groups within the batch.

---

ALT without multi-Raft for CGs

## Leadership Nomination
The CPC uses Raft for its own consensus across the cluster; however other CGs do not. Streams, Pipelines and other objects form groups based on the decisions of the CPC, this also includes the nomination of the leader of such groups. All in all, this is still a data driven approach which uses Raft for consensus and reliability across the entire cluster.

Leadership information is recorded in the CG records, and CG records are versioned based on a monotonically increasing value.

### Overview
Control groups do not use their own leadership election protocol as Raft does. Instead, they rely upon the cluster Raft — and the CPC which uses the cluster Raft — in order to establish leadership.

This protocol is closely related to the data replication protocol which control groups use to replicate their data, and overall it is identical to the way Raft replicates data, with the one major exception being that leader election is handled outside of the control group.

### Initial Nomination
New CGs will have a leader randomly assigned. This value will stay the same until the CPC detects that cluster nodes are missing Raft cluster heartbeats. Once node of the cluster is seen as being unhealthy according to the CPC, it will look at all leadership assignments for that node, and begin nominating new leaders per CG.

### Leader Failure Detection
All of the same considerations which Raft uses to detect leadership failure are applied to detecting CG leadership failure. These considerations are enumerated below.

- **False positive — CPC partial network partition:** The CG leader may be healthy and working fine, but the CPC node may be partially network partitioned. This means that the CPC leader will not be deposed, but it is not able to communicate with the CG leader, even though the CG leader is able to communicate with other nodes.
    - In these cases, the CG leader will only be deposed if the CPC determines that the other members of the CG are also not able to communicate with the CG leader. This is established by the CPC polling all members of the CG and checking to see if they have a live replication stream from their leader.
    - The CPC will stay within a failure reconsiliation mode for that CG until the network partition is resolved, and will emit metrics describing the issue for observability and alerting.
- **CG leader partitioned from CG members but available to CPC:** this condition is not technically a failure. The CG leader is still alive and able to handle requests. This is a highly selective network partition. CG leaders will report errors when they are not able to replicate to CG members, so there will still be observability and alerting available to handle such selective cases.

### Leadership Transfer
When the CPC has determined that the leader of a CG must be changed, and that a majority of the followers of the CG are reporting that they do not have a live replication stream from their leader, then the CPC will nominate one of the nodes which has the highest value for the CG's commit index. The CPC will then commit an update to the CG record in Raft, and once it has been committed the reconciliation task will poll all CG members until they have cut over to the new leader, at which point the reconciliation task for leadership transfer will be complete.
