Pipelines
=========
Pipelines are multi-stage data workflows, composed of multiple streams, structured as a directed acyclic graph (DAG). Pipelines provide transactional guarantees for multi-stage asynchronous workflows. Pipelines orchestrate the delivery of events to specific pipeline stages, collect outputs from pipeline stages, and enforce stage execution order. Pipelines provide a source of truth for codifying asynchronous event-driven architectures.

## DDL
Pipelines are declared in YAML as part of the [Schema Management system](./schema.md). The DDL for the `Pipeline` object is as follows:

```yaml
## The kind of object being defined. In this case, a pipeline.
kind: Pipeline
## The spec of this object.
spec:
  ## The namespace in which this pipeline is to be created.
  namespace: required string
  ## The name of the pipeline. Each pipeline must have a unique name per namespace.
  name: required string

  ## The stream from which this pipeline may be triggered. Only events on this
  ## stream may be used to trigger this pipeline, though triggering piplines
  ## must still be explicit. The trigger stream must exist in the same
  ## namespace as the pipeline.
  triggerStream: required string

  ## A number used to track consecutive updates to the pipeline definition. If
  ## the pipeline is updated using an old serial number or the same number
  ## currently tracked by Hadron, then the update will be ignored.
  ##
  ## Hadron will ensure that the values used here are monotonically increasing.
  revision: required unsigned integer

  ## `optional array of <stage>`: An array all stages of which this pipeline is
  ## composed. Pipelines are composed of one or more stages.
  stages:
    ## The name of the stage. Each stage has a unique name per pipeline.
    - name: required string

      ## `optional array of string`: A stage will be executed after all of the
      ## stages in its `after` array have successfully completed.
      ##
      ## Execution order must be acyclic. If this value is empty or omitted,
      ## then this stage is considered to be an initial stage of the pipeline,
      ## and will be invoked with a copy of the event which triggered
      ## the pipeline.
      after:
        ## The name of another stage in this pipeline.
        - required string

      ## `optional array of string`: Stages may depend upon the output of
      ## earlier stages, which also implies an `after` relationship with the
      ## specified stages.
      ##
      ## If no dependencies are declared, then the stage will be provided with
      ## the root event which triggered this pipeline. The root event can be
      ## referred to directly as `root_event` along with other stage outputs.
      dependencies:
        ## A dot-separated string declaring a stage and one its outputs by name.
        ## E.G., `stage-123.output-xyz`. The special name `root_event` refers
        ## to the root event.
        - required string

      ## `optional array of <output>`: Each stage may declare any number of
      ## outputs. Outputs declare events which must be provided by the stage
      ## handler and which will be published to the output's named stream
      ## transactionally along with the completion of the stage.
      outputs:
        ## Each output must have a unique name per stage.
        - name: required string

          ## The name of the stream to which this output's event will
          ## be published.
          stream: required string

          ## The namespace in which the output stream exists.
          namespace: required string
```

### Details
- Pipelines can be updated the same way all other Hadron DDL objects can be updated. See the [Schema Management](./schema.md) chapter for more details.
- Pipeline names may be 1-100 characters long, containing only `[-_.a-zA-Z0-9]`. The `.` can be used to form hierarchies for authorization matching wildcards.
- Pipelines may be composed of 1 or more stages (nodes of the graph).
- The flow of data through the pipeline — inputs and outputs — are the edges of the graph.
- Pipelines may have more than one initial stage. All initial stages of a pipeline will receive a copy of the event which triggered the pipeline.
- Pipelines are exclusive. They represent a single type of action to be taken over the data moving through the pipeline. All consumers of pipeline stages will be treated as being part of the same consumer group and messages will be load balanced across all consumers.
- Pipelines have unique names.
- Pipelines are directed acyclic graphs, cycles are not allowed.
- Hadron ensures that the output of a stage is delivered to all downstream stages with a connected edge, and that the message is `ack`'ed transactionally along with the delivery of that output. This greatly reduces the overhead of error handling and minimizes the difficulties of modelling exactly-once idempotent workflows.
- Pipeline stages may execute in parallel when stages are children of the same parent stage in the graph, or in serial order when stages are ordered one after another in the graph.

## Consumers
Pipeline stages must each be individually subscribed to useing the pipelines subscription API. The pipelines subscription API is uniform for all stages, and allows consumers to think purely in terms of reciving inputs and producing outputs. Hadron consumers are channel based, and multiple channels may exist per network connection to the Hadron cluster.

When the server sends a payload to a stage consumer to begin its work for that stage, it does so within the context of a new transaction. That consumer must then produce the expected stream outputs for its stage. If the transaction is committed before all of the required outputs are delivered, the transaction will rollback. If the transaction committs successfully, then the stage will be complete, and all outputs will have been committed to their respective streams.
