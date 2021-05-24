# Pipelines
```
Author: Dodd
Status: Implementing
```

Pipelines are a mechanism for driving complex behavior based on an input stream.

- Pipelines are divided into stages.
- Each stage may produce an output.
- Stages may depened upon previous stages, in which case they will receive the output of the pervious stages.
- All stage outputs are stored as part of the pipeline, on the node of the pipeline controller responsible for the pipeline partition, and are not sent to other locations in the cluster (except for replication within a replica set).
- Transactional cross-stream pipeline outputs via `streamOutputs`:
    - These will be required to be supplied as part of the pipeline `ack` payload.
    - The payloads will be immediately written to disk along side the pipeline stage's normal output, but will be asynchronously transactionally applied to the target streams. The offsets of the stream outputs will be updated in the pipeline instance's records as a pointer to the stream & partition to which the record was written.
    - This will provide a more structured model for transactional processing overall.
    - Only a normal stage output can be used as input to another stage, not stream outputs.

## Setup & Delivery
- Pipeline controllers will run parallel to the stream controller of the trigger stream, per partition. This is where pipeline horizontal scalability will come from.
- Pipelines are somewhat akin to a single subscription group, as every subscriber of a pipeline stage are part of the same group implicitly per pipeline.
- Each pipeline controller acts very similar to the subscription controller. It will await offset updates from the stream, and when updates are detected, the pipeline will query for data from the stream and analyze each record for matching event types (based on a record header value).
- For each matching record found, a new pipeline will be instantiated using that data.
- For each entrypoint stage of the pipeline, data will be delivered to a randomly selected channel subscribed to the stage.
- MAYBE: allow pipelines to declare a `maxParallel` value on the schema, which controls the max number of pipeline instances which may run in parallel. No limit by default, adding more consumers will provide more throughput.

## Acks & Output
- An ack response from a stage subscriber must include the output required by that stage (if applicable), else the server will reject the ack and treat it instead as a nack.
- For a complete ack containing the required stage output, the data will be written to the pipeline's storage for that `{pipeline_instance_id}/{stage}`. Once that data has been successfully applied to disk (also recording that the stage was successfully processed), and all other stages of the same dependency tier have finished, then the stages of the next tier may begin.

## Data Model
- A tree for the pipeline itself `/pipelines/ns/name/` & maybe one for pipeline metadata (tracking input stream offsets &c) `/pipelines/ns/name/metadata/`.
- The pipeline tree will record pipeline instances/executions based on the input stream's offset, which provides easy "exactly once" consumption of the input stream. In the pipeline tree, the key for a pipeline instance will be roughly `/{instance}/`, where `{instance}` is the input stream record's offset.
- Per pipeline instance, each stage will have its state and outputs (when completed successfully) stored under `/{instance}/{stage_name}`.
- All pipeline instance data, including stages, their outputs and the like, will be held in memory for all active pipeline instances. This facilitates better output delivery performance & less disk IO. Once a pipeline instance is complete, its memory will be released.

## Other
- **Pipeline cancellation:** In the future, maybe we create an explicit cancellation mechanism along with a retry system. For now, it must be encoded by users as stage output. Pretty much always, we should push users away from the idea of fallible pipelines. They should be idempotent.

#### Hadron Pipelines VS Other
**Airflow**
- Airflow is all Python, Hadron is any language.
- Hadron pipelines are versioned & declarative; Airflow is dynamic Python, a bit more on the loose and wild side.
- Hadron pipelines are fully integrated into the Hadron data store, inputs and outputs are durable.

**Temporal**
- In Hadron, state is encoded as stage outputs. Outputs can be merged and flow into downstream stages.
- In Hadron, stage consumers need only worry about stage inputs and the action to take on the outside world.
