Use Case: Service Provisioning
==============================

This use case assumes some familiarity with Hadron. It is recommended to have at least read the [Quick Start](../overview/quick-start.md) chapter before continuing here.

### Getting Started
We've joined a new company, ExampleCloud, which offers a service where customers may provision various types of storage systems in the cloud. Though conceptually simple, there are lots of individual stages in this workflow, each of which will take different amounts of time to complete, and all having different failure conditions and data requirements.

With Hadron, defining a Pipeline to model this workflow is simple:

```yaml
apiVersion: hadron.rs/v1beta1
kind: Pipeline
metadata:
  name: service-creation
spec:
  # The source Stream of this Pipeline.
  sourceStream: events
  # Event types which trigger this Pipeline.
  triggers:
    - service.created
  # When first created, the position of the Source stream to start from.
  startPoint:
    location: beginning
  # Maximum number of parallel executions per stage.
  maxParallel: 50
  stages:
    # Deploy the customer's service in Kubernetes.
    - name: deploy-service
```
<small><i>The existance of the Stream `events` is assumed, review [Quick Start](../overview/quick-start.md) chapter for details.</i></small>

Here we've defined a Pipeline called `service-creation` to model this workflow. Hadron uses this config to generate resources within its cluster to store and process the data for this new Pipeline. As show above, we can even document the purpose and expectations of or workflow stages.

With this configuration, any time our ExampleCloud application publishes a new event of type `service.created` to our Stream `events`, Hadron will automatically trigger a new Pipeline execution which will pass that new event through the Pipeline stages defined above (right now, only 1 stage).

### Client Setup
Next let's create a program which uses the Hadron Client to publish events of type `service.created` to our Stream `events`, and then we will also create a subscription to our new Pipeline which will process the stage `deploy-service`.

```rust
// Event producer which publishes events to stream `events`.
let client = hadron::Client::new("http://events.default:7000", /* ... params ... */)?;
let publisher = client.publisher("example-cloud-app").await?;

// Publish a new event based on some application logic.
let event = hadron::NewEvent { /* ... snip ... */ };
publisher.publish(event).await?;
```

```rust
// Process Pipeline stage `deploy-service`.
client.pipeline("service-creation", "deploy-service", deploy_handler);
```

Awesome! The consumer code of `deploy_handler` shown above can do whatever it wants. The only requirement is that when it is done, it must return a `Result<NewEvent, Error>` — that is, it must return an output event for success, or an error for failure cases (resulting in a retry).

Client disconnects will trigger retries. Errors will be tracked by Hadron and exposed for monitoring. Output events from successful processing of stages are persisted to disk. Once a stage is completed successfully for an event, that stage will never be executed again for the same event.

### New Requirements
Things are going well at our new company. Service creation is trucking along, and life is good. However, as it turns out, there are a few important steps which we've neglected, and our boss would like to have that fixed.

First, we forgot to actually charge the customer for their new services. Company isn't going to survive long unless we start charging, so we'll need to add a new stage to our Pipeline to handle that logic.

Next, customers have expressed that they would really like to know the state of their service, so once their service has been deployed, we'll need to deploy some monitoring for it. Let's add a new stage for that as well.

Finally, customers are also saying that it would be great to receive a notification once their service is ready. Same thing, new stage.

With all of that, we'll update the Pipeline's stages section to look like this:

```yaml
  stages:
    # Deploy the customer's service in Kubernetes.
    - name: deploy-service

    # Setup billing for the customer's new service.
    - name: setup-billing
      dependencies: ["deploy-service"]

    # Setup monitoring for the customer's new service.
    - name: setup-monitoring
      dependencies: ["deploy-service"]

    # Notify the user that their service is deployed and ready.
    - name: notify-user
      dependencies: ["deploy-service"]
```

Pretty simple, but let's break this down. First, the original `deploy-service` stage is still there and unchanged. Next, we've added our 3 new stages `setup-billing`, `setup-monitoring`, and `notify-user`. There are a few important things to note here:
- Each of the new stages depends upon `deploy-service`. This means that they will not be executed until `deploy-service` has completed successfully and produced an output event.
- Once `deploy-service` has completed successfully, our next 3 stages will be executed in parallel. Each stage will receive a copy of the root event which triggered this Pipeline execution, as well as a copy of the output event from the `deploy-service` stage.

This compositional property of Hadron Pipelines sets it apart from the crowd as a powerful data processing and data integration system.

Given that we've added new stages to our Pipeline, we need to add some new stage consumers to actually handle this logic. This is nearly identical to our consumer for `deploy-service`, really only the business logic in the handler will be different for each.

```rust
client.pipeline("service-creation", "setup-billing", billing_handler).await?;
client.pipeline("service-creation", "setup-monitoring", monitoring_handler).await?;
client.pipeline("service-creation", "notify-user", notify_handler).await?;
```

### Synchronization
A subtle, yet very impactful aspect of doing parallel processing is synchronization. How do we guard against conflicting states? How do we prevent invalid actions?

At ExampleCloud, we want to provide the best experience for our users. As such, we will add one more stage to our Pipeline which we will use to synchronize the system. Here is the new stage:

```yaml
    # Synchronize the state of the system.
    #
    # - Update tables using the event data from all of the upstream stages.
    # - Update the state of the new service in the database so that the
    #   user can upgrade, resize, or otherwise modify the service safely.
    - name: sync
      dependencies:
        - deploy-service
        - setup-billing
        - setup-monitoring
        - notify-user
```

The `sync` stage we've defined here will integrate the data signal from all previous stages in the Pipeline, receiving a copy of the root event and output events of each stage. There is a lot that we can do with this data, but here are a few important things that we should do:

- Update the state of the user's new service in our database. This will ensure that users see the finished state of their services in our fancy ExampleCloud API and UI.
- With this small amount of synchronization, we can prevent race conditions in our system by simply not allowing certain actions to be taken on a service when it is in the middle of a Pipeline.

### Next Steps
From here, we could begin to model other workflows in our system as independent Pipelines triggered from different event types. Examples might be:

- **Upgrade a service:** maybe the user needs a bigger or smaller service. There are plenty of granular tasks involved in making this happen, which is perfect for Pipelines.
- **Delete a service:** resources originally created and associated with a service may live in a few different microservices. Have each microservice process a different stage for their own service deletion routines as part of a new Pipeline specifically for deleting services.

See the [Use Case: Transactional Processing](./transactional-processing.md) for more details on how to build powerful stateful systems while still leveraging the benefits of an event-driven architecture.
