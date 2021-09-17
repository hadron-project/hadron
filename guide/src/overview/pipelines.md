Pipelines
=========
Pipelines are workflow definitions for data on Streams, providing structured concurrency for arbitrarily complex multi-stage tasks.

Pipelines exist side by side with their source Stream, and Streams may have any number of associated Pipelines. Pipelines are triggered for execution when an event published to a Stream has an event `type` which matches one of the trigger patterns of an associated Pipeline.

### Why
So, why do Pipelines exist, and what are they for?

Practically speaking, as software systems grow, they will inevitibly require sequences of tasks to be executed, usually in some logical ordering, and often times these tasks will cross system/service/microservice boundaries. When a system is young, such workflows might be simple, and may involve only one or two stages — such as making an API call and then emailing a user. However, as the system evolves, these workflows will grow in the number of stages, and ordering may become difficult.

When companies are young, these asynchronous tasks may be implicit. They exist in code, but their actual role in the system overall may be easily forgotten, and tracking down individual functions for these tasks may not be easy. The workflow may not even have a name.

Pipelines offer a way to name these workflows, to define them as declarative specifications so that they can be versioned and reviewed. Pipelines are a way to avoid sprawl, confusion, and to bring clarity to how a software system actually operates at the highest levels.

Pipelines can be used to define the entire logical composition of a company's software systems. No more unnamed workflows which exist only as tribal knowledge in the minds of a few developers. No more grueling debug sessions where the first few hours are spent just trying to trace an implicit, unnamed, multi-stage workflow across multiple microservices.

### Scaling & High Availability
Pipelines exist side by side with their source Stream. All scaling and availability properties of the source Stream apply to any and all Pipelines associated with that Stream. See the [Streams Overview](./streams.md) for more details on these properties.

### Publishers
Pipelines do not have their own direct mechanism for publishing data to a Pipeline. Instead, data is published to the Pipeline's source Stream, and when an event on that source Stream has a `type` field which matches one of the Pipeline's trigger patterns, then a new Pipeline execution will be started with that event as the "root event" of the Pipeline execution.

### Triggers
Every Pipeline may be declared with zero or more `triggers`. When an event is published to a Pipeline's source Stream, its `type` field will be compared to each of the matcher patterns in the Pipeline's `triggers` list. If any match is found, then a new Pipeline execution will begin for that event.

If a Pipeline is declared without any `triggers`, or with a trigger which is an empty string (`""`), then it will match every event published to its source Stream.

### Consumers
Pipelines are consumed in terms of their stages. As Hadron client programs register as Pipeline consumers, they are required to speciy the stage of the Pipeline which they intend to process. All Pipeline consumers form an implicit group per stage.

### Pipeline Evolution
As software systems evolve over time, it is inevitible that Pipelines will also evolve. Pipelines may be safely updated in many different ways, the only dangerous update is to remove a Pipeline's stage. Doing so should ALWAYS be considered to result in data loss. These semantics may change in the future, however it is best avoided.

Barring removal of Pipeline stages, most other updates are perfectly safe and encouraged. Adding new stages, changing dependencies and ordering, all of this is perfectly fine and expected.

There is no renaming of Pipeline stages, this is tantamount to deleting a stage and adding a new stage with a different name.

#### Updating Consumers
As Pipelines evolve, users should take care to ensure that their applications have been updated to process any new stages added to the Pipeline. Hadron makes this very simple:

- Before applying the changes to the Pipeline which adds new stages, first update the application's Pipeline consumers.
- Add a consumer for any new stages.
- Deploy the new application code. The new consumers will log errors as they attempt to connect, as Hadron will reject the consumer registry until the new stages are applied to the Pipeline. This is perfectly fine, and will not crash the application code. Hadron will simply back off, and retry the connection again soon.
- Now it is safe to apply the changes to the Pipeline.

In essence: add your Pipeline stage consumers first.

If this protocol is not adhered to, then the only danger is that the Pipeline will eventually stop making progress, as too many parallel executions will remain in an uncomplete state as they wait for the new Pipeline stages to be processed. Avoid this by deploying your updated application code first.
