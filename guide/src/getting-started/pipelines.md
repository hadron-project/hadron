Pipelines
=========
Pipelines allow users to model data streams within Railgun in terms of a graph.

- Pipelines may be composed of 1 or more stages (nodes of the graph), which are just streams with their associated handler functions.
- The flow of data through the pipeline — inputs and outputs — are the edges of the graph.
- Pipelines have unique names per namespace.
- Pipelines are directed acyclic graphs.
- Pipelines allow users to model complex workflows, while only having to write handler code which takes an input and returns an output.
- Pipelines must have exactly one entrypoint stage. That stage may be an RPC message handler or a stream handler. All other downstream stages of a pipeline must be stream handler stages (not ephemeral or RPC message handler stages).
- Railgun ensures that the output of a stage is delivered to all downstream stages with a connected edge, and that the message is `ack`'ed transactionally along with the delivery of that output. This greatly reduces the overhead of error handling and the difficulties of modelling exactly-once idempotent workflows.
- Pipeline stages may execute in parallel when stages are children of the same parent node in the graph, or in serial order when stages are ordered one after another in the graph.
- Pipelines may have internal/private stages, which are durable streams which can only be published to or read from as part of its pipeline.
