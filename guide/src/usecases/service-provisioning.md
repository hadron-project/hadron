Use Case: Service Provisioning
==============================

### Getting Started
We've joined a new company, ExampleCloud, which offers a service where customers may provision various types of storage systems in the cloud. Though conceptually simple, there are lots of individual stages in this workflow, each of which will take different amounts of time to complete, and all having different failure conditions and data requirements.

With Hadron, defining a Pipeline to model this workflow is simple:

```yaml
---
apiVersion: hadron.rs/v1beta1
kind: Stream
metadata:
  name: events
spec:
  partitions: 3

---
apiVersion: hadron.rs/v1beta1
kind: Pipeline
metadata:
  name: service-creation
spec:
  sourceStream: events
  triggers:
    - service.created
  stages:
    - name: deploy-service
```

Here we've define a Stream called `events` and a Pipeline called `service-creation` to model this workflow. Hadron uses this config to generate resources within its cluster to store and process the data for this new Stream and Pipeline.

With this configuration, any time our ExampleCloud application publishes a new event of type `service.created` to our Stream `events`, Hadron will automatically trigger a new Pipeline execution which will pass that new event through the Pipeline stages defined above (right now, only 1 stage).

### Client Setup
To get started, we will create a program which uses the Hadron Client to publish events of type `service.created` to our Stream `events`, and then we will also create a subscription to our new Pipeline which will process the stage `deploy-service`.

```rust
// Event producer which publishes events to stream `events`.
let client = hadron::Client::new("cluster.url:7000", /* ... */)?;
let publisher = client.publisher("example-cloud-app").await?;

// Publish a new event based on application logic.
publisher.publish(new_event).await?;
```

```rust
// Process Pipeline stage `deploy-service`.
client.pipeline("service-creation", "deploy-service", deploy_handler);
```

Awesome! The consumer code (the function `deploy_handler` above) can do whatever it wants. The only requirement is that when it is done, it must return a `Result<NewEvent, Error>` — that is, it must return an output event for success, or an error for failure cases (resulting in a retry).

Client disconnects will trigger retries. Errors will be tracked by Hadron and exposed for monitoring. Output events from successful processing of stages are persisted to disk. Once a stage is completed successfully for an event, that stage will never be executed again for the same event.

### New Requirements
Things are going well at our new company. Service creation is trucking along, deploying services for customers, and life is good. However, as it turns out, there are a few important steps which we've neglected, and our boss would like to have that fixed.

First, we forgot to actually charge the customer for their new services. Company isn't going to survive long unless we start charging, so we'll need to add a new stage to our Pipeline to handle that logic.

Next, customers have expressed that they would really like to know the state of their service, so once their service has been deployed, we'll need to deploy some monitoring for it. Let's add a new stage for that as well.

Finally, customers are also saying that it would be great to receive a notification once their service is ready. Same thing, new stage.

With all of that, we'll update the Pipeline's stages section to look like this:

```yaml
  stages:
    - name: deploy-service
    - name: setup-billing
      dependencies: ["deploy-service"]
    - name: setup-monitoring
      dependencies: ["deploy-service"]
    - name: notify-user
      dependencies: ["deploy-service"]
```

Pretty simple, but let's break this down. First, the original `deploy-service` stage is still there and unchanged. Next, we've added our 3 new stages `setup-billing`, `setup-monitoring`, and `notify-user`. There are a few important things to note here:
- Each of the new stages depends upon `deploy-service`. This means that they will not be executed until `deploy-service` has completed successfully and produced an output event.
- Once `deploy-service` has completed successfully, our next 3 stages will be executed in parallel. Each stage will receive a copy of the root event which triggered this Pipeline execution, as well as a copy of the output event of any stages declared as a dependency.

This compositional property of Hadron Pipelines sets it apart from the crowd as a powerful data processing and data integration system.

Given that we've added new stages to our Pipeline, we need to add some new stage consumers to actually handle this logic. This is nearly identical to our consumer for `deploy-service`, really only the business logic in the handler will be different for each.

```rust
client.pipeline("service-creation", "setup-billing", billing_handler).await?;
client.pipeline("service-creation", "setup-monitoring", monitoring_handler).await?;
client.pipeline("service-creation", "notify-user", notify_handler).await?;
```

### Analytics
The team is quite happy with the ease of extending our Pipeline and adding new stage consumers. However, the company has continued to grow, and we've agreed that some deeper analytics would be great as a final touch.

In this case, we would like to evaluate execution time, errors, re-delivery rates, and other such metadata on our stage handlers. Let's add a new stage to our Pipeline.


```yaml
    - name: analytics
      dependencies:
        - deploy-service
        - setup-billing
        - setup-monitoring
        - notify-user
```

The `analytics` stage we've defined here will integrate the data signal from all previous stages in the Pipeline, receiving a copy of the root event and output events of each stage. With all of this data, we can generate metrics based on event metadata or even application specific data within each event.

The sky is the limit. If needed, we could transactionally materialize this data into a traditional RDBMS like PostgreSQL, or we could ship this data over to other systems for graphing, metrics, alerting and the like. As we've seen, adding new stages to a Pipeline is straightforward. Backfilling stages to take place earlier in a Pipeline's workflow is possible, and even adding that output as a dependency to other existing stages is fully backwards compatible.
