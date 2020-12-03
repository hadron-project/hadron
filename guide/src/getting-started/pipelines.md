Pipelines
=========
Pipelines are multi-stage data workflows, composed of multiple streams, structured as a directed acyclic graph (DAG). Pipelines provide transactional guarantees for multi-stage asynchronous workflows. Pipelines orchestrate the delivery of events to specific pipeline stages, collect outputs from pipeline stages, and enforce stage execution order. Pipelines provide a source of truth for codifying asynchronous event-driven architectures.

Pipelines are declared in YAML. The spec is defined as follows:

```yaml
## The kind of object being defined. In this case, a pipeline.
kind: pipeline
## The namespace in which this pipeline is to be created.
namespace: required string
## The name of the pipeline. Each pipeline must have a unique name.
name: required string
## The stream from which this pipeline may be triggered. Only events on this stream may be used to
## trigger this pipeline, though triggering piplines must still be explicit. The trigger stream
## must exist in the same namespace as the pipeline.
triggerStream: required string
## A number used to track consecutive updates to the pipeline definition. If the pipeline is
## updated using an old serial number or the same number currently tracked by Hadron, then the
## update will be ignored.
##
## Hadron will ensure that the values used here are monotonically increasing.
revision: required unsigned integer
## `optional array of <stage>`: An array all stages of which this pipeline is composed. Pipelines
## are composed of one or more stages.
stages:
  ## The name of the stage. Each stage has a unique name per pipeline.
  - name: required string
    ## `optional array of string`: A stage will be executed after all of the stages in its `after`
    ## array have successfully completed.
    ##
    ## Execution order must be acyclic. If this value is empty or omitted, then this stage is
    ## considered to be an initial stage of the pipeline, and will be invoked with a copy of the
    ## event which triggered the pipeline.
    after:
      ## The name of another stage in this pipeline.
      - required string
    ## `optional array of string`: Stages may depend upon the output of earlier stages, which also
    ## implies an `after` relationship with the specified stages.
    ##
    ## If no dependencies are declared, then the stage will be provided with the root event which
    ## triggered this pipeline. The root event can be referred to directly as `root_event` along
    ## with other stage outputs.
    dependencies:
      ## A dot-separated string declaring a stage and one its outputs by name. E.G.,
      ## `stage-123.output-xyz`. The special name `root_event` refers to the root event.
      - required string
    ## `optional array of <output>`: Each stage may declare any number of outputs. Outputs
    ## declare events which must be provided by the stage handler and which will be published
    ## to the output's named stream transactionally along with the completion of the stage.
    outputs:
      ## Each output must have a unique name per stage.
      - name: required string
        ## The name of the stream to which this output's event will be published.
        stream: required string
        ## The namespace in which the output stream exists.
        namespace: required string
```

// TODO: the content below needs revision. The data above is most recent and up-to-date.


Pipelines may be used to model services which are responsible for handling request/response traffic — in which case the pipline will start with an RPC endpoint. Pipelines may also be used to model background tasks which do not handle client traffic — in which case the pipeline will start with a durable stream stage. This is perfect for asynchronous build systems, provisioning, data processing, custom jobs, and the like.

- Pipeline names may be 1-100 characters long, containing only `[-_.a-zA-Z0-9]`. The `.` can be used to form hierarchies for authorization matching wildcards.
- Pipelines may be composed of 1 or more stages (nodes of the graph), which are just streams with their associated handler functions.
- Pipelines may have more than one initial stage. All initial stages of a pipeline will receive a copy of the event which triggered the pipeline.
- Pipelines are exclusive. They represent a single type of action to be taken over the data moving through the pipeline. All consumers of pipeline stages will be treated as being part of the same consumer group and messages will be load balanced across all consumers. To consume from a public stage of a pipeline for some purpose outside of the scope of the pipeline, use the standard stream consumer or RPC consumer APIs.
- Pipelines may only be consumed by the user which originally created it, which helps to clarify the intent of pipelines.
- The flow of data through the pipeline — inputs and outputs — are the edges of the graph.
- Pipelines have unique names.
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
