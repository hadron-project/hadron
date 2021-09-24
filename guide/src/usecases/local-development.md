Local Development
=================
Let's get started with Hadron for local application development using Kubernetes and Kind.

[Kind](https://kind.sigs.k8s.io/) is a tool for running local Kubernetes clusters using Docker container “nodes” and is the easiest way to get started with Kubernetes.

### Install Kind
Follow the instructions in the [Kind installation guide](https://kind.sigs.k8s.io/docs/user/quick-start/#installation) to ensure kind is installed locally and ready for use.

Next, create a cluster.

```sh
kind create cluster
```

Once your cluster is up and running, you are ready to move on to the next step.

### Install Hadron
[Helm](https://helm.sh/) is the package manager for Kubernetes, and it will need to be installed and available for command-line usage for this use case. If you've been using Kubernetes for any amount of time, then you are probably already using helm.

Before we install the Hadron Operator, we are going to install [cert-manager](https://cert-manager.io/). Though cert-manager is not required by Hadron, it does greatly simplify the setup of TLS certificates, which Hadron uses for its validating webhooks. Instead of manually crafting our own certs, we'll stick with cert-manager.

```sh
helm repo add jetstack https://charts.jetstack.io
helm upgrade cert-manager jetstack/cert-manager --install --set installCRDs=true
```

Now we are ready to install the Hadron Operator:

<!-- TODO: add links to the Hadron Operator helm chart once it is published. -->

```
helm repo add hadron https://helm.hadron.rs
helm upgrade hadron-operator hadron/hadron-operator --install
```

### Install Example Resources
For this use case, let's use the example resources found in the Hadron repo. [Here is the code](https://raw.githubusercontent.com/hadron-project/hadron/tree/v0.1.0-beta.0/charts/hadron-operator/examples/full.yaml).

Apply the code to the cluster:

```sh
wget -qO- https://raw.githubusercontent.com/hadron-project/hadron/tree/v0.1.0-beta.0/charts/hadron-operator/examples/full.yaml | kubectl apply -f -
```

The example file is about 75 lines of code, so here we will only show the names and types of the resources for brevity.

```yaml
apiVersion: hadron.rs/v1beta1
kind: Stream
metadata:
  name: events
  ...

---
apiVersion: hadron.rs/v1beta1
kind: Pipeline
metadata:
  name: service-creation
# ...

---
apiVersion: hadron.rs/v1beta1
kind: Token
metadata:
  name: hadron-full-access
# ...

---
apiVersion: hadron.rs/v1beta1
kind: Token
metadata:
  name: hadron-read-only
# ...

---
apiVersion: hadron.rs/v1beta1
kind: Token
metadata:
  name: hadron-read-write
# ...
```

This example code defines a Stream, a Pipeline associated with that Stream, and 3 Tokens which can be used to experiment with Hadron's authentication and authorization system.

### Application Integration
Integrating you application with Hadron involves three simple steps:
- Add the Hadron client as an application dependency. This is language dependent. In Rust, simply add `hadron-client = "0.1.0-beta.0"` to the `[dependencies]` section of your `Cargo.toml`.
- Next, determine the access token which your application will use. For this use case, we'll use the `hadron-full-access` Token.
- Finally, we need the URL to use for connecting to the Hadron Stream. This is always deterministic based on the name of the Stream itself, and follows the pattern: `http://{streamName}.{namespaceName}.svc.{clusterApex}:7000`.
    - `{streamName}` is `events`,
    - `{namespaceName}` is `default`,
    - `{clusterApex}` defaults to `cluster.local` in Kubernetes,
    - which all works out to `http://events.default.svc.cluster.local:7000`,
    - for details on how to connect from outside of the Kubernetes cluster, see the [Streams reference](../reference/streams.md).

Now that we have this info, let's define a Kubernetes Deployment which uses this info. In this case we will just be using the Hadron CLI to establish Stream subscriptions, but it will sufficiently demonstrate how to integrate an application using these details.

Create a file called `deployment.yaml` with the following contents:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: demo-client
  namespace: default
spec:
  replicas: 3
  selector:
    matchLabels:
      app: demo-client
  template:
    metadata:
      labels:
        app: demo-client
    spec:
      containers:
      - name: cli
        image: ghcr.io/hadron-project/hadron/hadron-cli:v0.1.0-beta.0
        command: ["hadron", "stream", "sub", "--group=demo-client", "--start-beginning"]
        env:
        - name: HADRON_TOKEN
          valueFrom:
            secretKeyRef:
              name: hadron-full-access
              key: token
        - name: HADRON_URL
          value: http://events.default.svc.cluster.local:7000
```

Now apply this file to the cluster:

```sh
kubectl apply -f deployment.yaml
```

This will create a new deployment, with 3 replicas, and each replica pod will be running an instance of the Hadron CLI. The CLI will create a subscription to all partitions of the Stream `events`, and will print the contents of each event it receives and will then `ack` the event.

### Next Steps
From here, some good next steps may be:

- **Make some changes:** you've made it pretty far through the guide! Now might be a good time to try some experimentation of your own.
- **Model application workflows:** start modeling your own application workflows by encoding them as [Pipelines](../reference/pipelines.md). Start writing your client code for publishing application events to your Stream, and then write the code which will process the various stages of your Pipelines. Check out the [Use Case: Service Provisioning](./service-provisioning.md) for some deeper exploration.
- **Prepare for Production deployment:** reviewing the reference sections for Hadron's resources is a great way to prepare for deploying Hadron in a production environment. [Streams](../reference/streams.md) have various configuration options which can be tuned for scaling, storage, resource quotas and the like.
