# CPC - Cluster Placement Controller
## Overview
This controller is responsible for tracking all live objects in the system which are in some way distributed across the cluster. Streams, Pipelines, Endpoints and other such objects all all have their own controllers which run on specific nodes of the cluster and work together to accomplish their overall goal.

This controller is responsible for determining exactly where on the cluster those controllers are to run, and acts as a Raft client, sending various placement decision making requests to the CRC in order to ensure it is known and properly replicated across the cluster.

This controller will only perform mutating actions when it is running on the same node as the CRC leader (the Raft leader) — when it is the leader. Otherwise it will be completely passive and never forwards placement commands to other nodes.

## Leadership
This controller is responsible for placement of objects & other controllers within the cluster, and uses the Raft state machine as a command bus in order to do so.

All cluster members process the data in the state machine in serial order as the data is applied. When a new node joins the Raft cluster, it will begin from a recovered snapshot state, and will then resume with live updates from the cluster.

## Placement
Placement is driven from data held by the Raft state machine. As Streams, Pipelines and other objects are created, data records describing their various distributed components (known as controllers) are also created. They are initially created without any placement details.

The CPC will observe these data records, and reconcile their state against the overall goal of the CPC, which is to have all objects of the cluster distributed evenly and in a well-balanced fashion. As such, unassigned objects will be assigned to nodes of the cluster based on various heuristics (detailed below). The assignments will be committed to the cluster via Raft, and the requests will not be forwarded, which ensures that only the CPC on the Raft leader node is able to make changes to the cluster.

When the CPC detects objects with an aspired state, it will spawn tasks to drive that aspired state to completion. This is done by issuing network commands to other nodes of the cluster, or by issuing local commands to the node on which the CPC is running if the changes apply to its own node. Once the work has completed successfully, the CPC will update the records via Raft. Upon successfully updating the aspired state to the current state, the reconciliation process for that object will then be complete.

Disruptions to a reconciliation task will simply be restarted. If a CPC group leader is deposed, it will observe such a change via Raft metrics, via a response from another peer, or it will have died and recovered nodes never start as Raft leaders.

## Placement Heuristics
For the Hadron beta, the only heuristic used to drive the placement algorithm is simple load balancing by object type. The CPC will attempt to distribute objects evenly across the cluster by object type.

## Control Groups
The CPC is responsible for creating Control Group (CG) records and assigning controllers to groups. This differs based on the type of controller. For Streams, CGs are formed per partition. This is also true for Pipelines, except that Pipelines only ever have 1 partition.

CGs use Raft for consensus, leadership, failure detection and the like. This comprises a multi-raft topology, as each CG has its own independent Raft group which is separate from the cluster Raft and is also separate from other CGs.

### Scaling Raft
Our solution to scaling Raft, specifically given the fact that we are using a multi-raft topology, is primarily based on the transport layer of Hadron, namely HTTP2 (H2). H2 offers a few excellent advantages over other transport protocols in terms of connection re-use, multiplexing of streams/channels over a single H2 connection, and also has a built-in mechanism for flow control which will buffer outbound data based on configurable window sizes.

Use of H2 provides a simple mechanism for scaling network communication between cluster nodes, including Raft groups, and provides a built-in multiplexing and demultiplexing solution which allows Hadron to keep our application code straightforward and intuitive.

<!--
### Old ISR Model
All Control Groups use the In-Sync Replicas (ISR) pattern for data replication. Hadron dynamically maintains a set of ISRs that are caught-up to the leader per CG. Only members of this set are eligible for election as leader — which is handled as part of the cluster Raft, not the Control Group. A write to a Hadron partition is not considered committed until all ISRs have received the write. This ISR set is persisted to the cluster Raft whenever it changes. Because of this, any replica in the ISR set is eligible to be elected leader. With this ISR model and `n+1` replicas, a Hadron stream can tolerate `n` failures without losing committed messages.

The Hadron CPC is able to seamlessly add a new member to a CG via the Raft cluster. As nodes observe the added member to their CG, they are able to react and bring the node up-to-date. Once the node has been brought up-to-date, the CG leader will commit an update to the cluster Raft updating its ISR set to include the new node. In the case of the new node being added, this will also resolve the aspired state of the new node's placement within the cluster.

## Failure Detection
Every CG leader must maintain an active connection with the cluster CPC. These connections are initiated by the CPC, and are used for committing ISR changes. When a connection is severed and can not be restored within a specific failover threshold, the CPC will elect a new leader for the CG based on its most recent ISR data. -->
