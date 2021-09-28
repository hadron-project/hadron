Pipelines
=========
Pipelines are workflow orchestration for data on Streams, providing structured concurrency for arbitrarily complex multi-stage workflows.

Pipelines are defined as CRDs stored in Kubernetes. Pipelines exist side by side with their source Stream, and Streams may have any number of associated Pipelines. Pipelines are triggered for execution when an event published to a Stream has an event `type` which matches one of the trigger patterns of an associated Pipeline.

```yaml
apiVersion: hadron.rs/v1beta1
kind: Pipeline
metadata:
  ## The name of this Pipeline.
  name: :string
  ## The Kubernetes namespace of this Pipeline.
  ##
  ## This Pipeline's associated `sourceStream` must exist within
  ## the same Kubernetes namespace.
  namespace: :string
spec:
  ## The name of the Stream which feeds this Pipeline.
  ##
  ## Events published to the source Stream which match this Pipeline's
  ## `triggers` will trigger a new Pipeline execution with the matching
  ## event as the root event.
  sourceStream: :string

  ## Patterns which must match the event `type` of an event on the
  ## source Stream in order to trigger a Pipeline execution.
  triggers: [:string]

  ## The maximum number of Pipeline executions which may be executed in parallel.
  ##
  ## This is calculated per partition.
  maxParallel: :integer

  ## The location of the source Stream which this Pipeline
  ## should start from when first created.
  startPoint:
    ## The start point location.
    location: :string # One of "beginning" | "latest" | "offset"
    ## The offset to start from, which is only evaluated when `location` is `offset`.
    ##
    ## This is applied to all partitions of the source Stream identically.
    offset: :integer

  ## Workflow stages of this Pipeline.
  stages:
    ## Each stage must have a unique name.
    - name: :string
      ## The names of other stages in this Pipeline which
      ## must be completed first before this stage may start.
      after: [:string]
      ## The names of other stages in this Pipeline
      ## which this stage depends upon for input.
      ##
      ## All dependencies listed will have their outputs delivered
      ## to this stage at execution time.
      ##
      ## When a stage is listed as a dependency, there is no need to
      ## also declare it in the `after` list.
      dependencies: [:string]
```
