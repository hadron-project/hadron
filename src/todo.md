todo
====
After much additional analysis, and after implementing an Actix actor-based Raft framework, using Raft for everything related to data makes sense.

Two of the biggest features of this system to advance the world of streaming architecture by leaps and bounds:
- ack operations which can atomically write to other streams as part of the ack.
- stream pipelines: Streams can have pipelines defined on them. Each pipeline defines a series of "stages". The pipeline defines a series of tasks, each of which must belong to exactly one stage. All tasks belonging to the same stage are executed in parallel and must be independently ack'd. Stages are executed in order, and will only proceed to the next stage when all tasks of the previous stage are complete. Consumers can consume the stream itself, or a specific pipeline of that stream, but never combimations of these options.

Given the above, starting out with a single Raft cluster (no Raft groups or multi-raft) will make this a much easier implementation, as multi-raft would require coordinated distributed transactions for streams which perform ID checking. This a single Raft, all of the functionality can be enforced on the leader.
