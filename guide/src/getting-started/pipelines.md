Pipelines
=========
Pipelines are pre-defined multi-stage data workflows, structured in terms of a graph. Pipelines provide transactional guarantees over acking a stages work and the delivery of the next stages data. With Pipelines, Railgun provides a platform with greater guarantees, and reduced error handling for building streaming-first service-oriented architectures.

Pipelines may be used to model services which are responsible for handling request/response traffic — in which case the pipline will start with an RPC endpoint. Pipelines may also be used to model background tasks which do not handle client traffic — in which case the pipeline will start with a durable stream stage. This is perfect for asynchronous build systems, provisioning, data processing, custom jobs, and the like.

- Pipeline names may be 1-100 characters long, containing only `[-_.a-zA-Z0-9]`. The `.` can be used to form hierarchies for authorization matching wildcards.
- Pipelines may be composed of 1 or more stages (nodes of the graph), which are just streams with their associated handler functions.
- Pipelines must have exactly one entrypoint stage. That stage may be an RPC message handler or a stream handler. All other downstream stages of a pipeline must be stream handler stages (not ephemeral or RPC message handler stages).
- Pipelines are exclusive. They represent a single type of action to be taken over the data moving through the pipeline. All consumers of pipeline stages will be treated as being part of the same consumer group and messages will be load balanced across all consumers. To consume from a public stage of a pipeline for some purpose outside of the scope of the pipeline, use the standard stream consumer or RPC consumer APIs.
- Pipelines may only be consumed by the user which originally created it, which helps to clarify the intent of pipelines.
- The flow of data through the pipeline — inputs and outputs — are the edges of the graph.
- Pipelines have unique names per namespace.
- Pipelines are directed acyclic graphs.
- Pipelines allow users to model complex workflows, while only having to write handler code which takes an input and returns an output.
- Railgun ensures that the output of a stage is delivered to all downstream stages with a connected edge, and that the message is `ack`'ed transactionally along with the delivery of that output. This greatly reduces the overhead of error handling and minimizes the difficulties of modelling exactly-once idempotent workflows.
- Pipeline stages may execute in parallel when stages are children of the same parent stage in the graph, or in serial order when stages are ordered one after another in the graph.
- Pipelines starting with an RPC endpoint may return a response from any stage of the pipeline, and other stages of the pipeline may execute asynchronous work even after the response is returned.
- Pipelines may have internal/private stages, which are durable streams which can only be published to or read from as part of its pipeline.
- Pipelines are configured as a whole to have all stages check message ID uniqueness or not. That is, the entire pipeline is configured to be an "exactly once" pipeline or not. This option is specified at pipeline creation time.

### pipeline private stages
- Private stages must have an inboud edge (they may not be the entrypoint stage of the pipeline).
- Private stages must have an outbound edge (they may not be the final resting place for data).

### pipeline creation
Pipelines are created in code via the client `EnsurePipeline` request, and are created as a whole. For existing pipelines, new streams with new edges can be added, and some of the configuration parameters of streams within the pipeline can be changed, but streams can not be removed from a pipeline, and edges can neither be changed nor removed.

To ensure that conflicts do not arise, a version number must be provided in the `EnsurePipeline` call. If a call is received which has an older version number, the call will succeed without any changes being made to the pipeline. If a newer version is received, the requested changes will be applied to the pipeline.

Client libraries in various languages may have different ways of modelling and gathering the data needed for the `EnsurePipeline` call, but regardless of the language, in the end the request will be sent to the Railgun cluster via the protobuf `EnsurePipeline` message.

### pipeline deletion
Pipelines can only be deleted via the `rgctl` CLI.

### consumers
Pipeline stages must each be individually subscribed to useing the pipelines subscription API. The pipelines subscription API is uniform for all stage types (RPC endpoints & streams), and allows consumers to think purely in terms of reciving an input and returning an output. Railgun will transactionally ack the message and deliver the output to all downstream stages with a connected edge.

### example
TODO: build an example covering a workflow which is common to most systems. Maybe password reset, sign-up or the like.
