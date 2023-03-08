Quick Start
===========
Hadron is designed to run within a Kubernetes cluster. This chapter will bring you up-to-speed on everything you need to know to start running your own Hadron clusters.

### Kubernetes
If you already have Kubernetes clusters, then choose a cluster to use. Starting with a development oriented cluster or namespace is typically a good idea. If you do not have a Kubernetes cluster available for use, then you have a few options:

- Get warmed up to Kubernetes using [Kind](https://kind.sigs.k8s.io/) - a tool for running local Kubernetes clusters using Docker container “nodes”. The [Use Case: Local Development](../usecases/local-development.md) in this guide is a great way to get started with Hadron and Kind for local development.
- Managed Kubernetes. Every cloud provider has a managed Kubernetes offering, take your pick.
- **The Hadron Collider** — our hosted Hadron Cloud **_(under construction)._** The only hosted and fully managed Hadron solution. Pick your cloud, pick your region, and start using Hadron. The Hadron Cloud offers deep integrations with major cloud providers, and is the best way to get started with Hadron.

[Helm](https://helm.sh/) is the package manager for Kubernetes, and it will need to be installed and available for command-line usage to get started. If you've been using Kubernetes for any amount of time, then you are probably already using helm.

### Installation
Before we install the Hadron Operator, we are going to install [cert-manager](https://cert-manager.io/). Though cert-manager is not required by Hadron, it does greatly simplify the setup of TLS certificates, which Hadron uses for its validating webhooks. Instead of manually crafting our own certs, we'll stick with cert-manager.

```sh
helm repo add jetstack https://charts.jetstack.io
helm upgrade cert-manager jetstack/cert-manager --install --set installCRDs=true
```

Now we are ready to install the Hadron Operator:

```
# Helm >= v3.7.0 is required for OCI usage.
helm install hadron-operator oci://ghcr.io/hadron-project/charts/hadron-operator --version 0.1.3
```

This will install the Hadron Operator along with roles, service accounts, validating webhooks, and other Kubernetes resources which the Operator requires.

### Create a Stream
You are now ready to create a Stream. Create a file `stream.yaml` with the following contents:

```yaml
apiVersion: hadron.rs/v1beta1
kind: Stream
metadata:
  name: events
spec:
  partitions: 3
  image: ghcr.io/hadron-project/hadron/hadron-stream:latest
  pvcVolumeSize: "5Gi"
```

See the [Stream Reference](../reference/streams.md) for more details on the spec fields listed above, as well as other config options available for Streams. Now apply the file to your Kubernetes cluster as shown below using the `kubectl` CLI (part of the Kubernetes distribution).

```sh
kubectl apply -f stream.yaml
```

In this example, we are applying the Stream CR instance as an independent Kubernetes manifest. For production usage and long-term maintainability, it is recommended to include your Hadron manifests as part of a helm chart which will likely include other Streams, Pipelines and Tokens.

Applying this resource to your cluster will result in the creation of a Kubernetes StatefulSet bearing the same name, along with the creation of a few Kubernetes Services.

### Create a Token
In order to access the resources of a Hadron cluster, a Token CR must be created describing a set of permissions for the bearer of the Token. Create a file `token.yaml` with the following contents:

```yaml
apiVersion: hadron.rs/v1beta1
kind: Token
metadata:
  name: hadron-full-access
spec:
  all: true
```

See the [Token Reference](../reference/tokens.md) for more details on the spec of Hadron Tokens. Now apply the file to your Kubernetes cluster as shown below.

```sh
kubectl apply -f token.yaml
```

Applying this resource to your cluster will result in the creation of a Kubernetes Secret in the same namespace bearing the same name. The generated secret may be mounted and used as an env var for your application deployments and the like, or it may be used by the Hadron CLI for cluster access.

### CLI Access
Now that we've defined a Stream along with a Token to allow us to access that Stream, we are ready to start publishing and consuming data.

First, let's get a copy of the Token's generated Secret (the actual JWT) for later use. We will use `kubectl` to extract the value of the secret:

```sh
HADRON_TOKEN=$(kubectl get secret hadron-full-access -o jsonpath='{.data.token}' | base64 --decode)
```

With the decoded token set as an environment variable, let's now run the CLI:

```sh
kubectl run hadron-cli --rm -it \
    --env=HADRON_TOKEN=${HADRON_TOKEN} \
    --env=HADRON_URL='http://events.default.svc.cluster.local:7000' \
    --image ghcr.io/hadron-project/hadron/hadron-cli:latest
```

Here we are running a temporary pod which will be removed from the Kubernetes cluster when disconnected. Once the pod session is started, you should see the help text of the CLI displayed, and then you should have access to the shell prompt.

From here, we can execute CLI commands to interact with our new Stream. Let's publish a simple event:

```sh
hadron stream pub --type example.event '{"demo": "live"}'
```

This will publish a simple event as a JSON blob. Publishing of binary events, such as protobuf, is also fully supported. Let's create a consumer to read this event.

```sh
hadron stream sub --group demo --start-beginning
```

You should see some output which looks like:

```sh
handling subscription delivery id=<generated> source=hadron-cli specversion=1.0 type=example.event optattrs={} data='{"demo": "live"}'
```

### Wrapping Up
This example shows the most basic usage of Hadron. From here, the next logical steps might be:

- **Continue reading:** learn more about how Hadron leverages the Kubernetes platform in the next chapter of this guide.
- **Define Pipelines:** start modeling your application workflows as code using Pipelines. See the [Use Case: Service Provisioning](../usecases/service-provisioning.md) for deeper exploration on this topic.
- **Application Integration:** go to the [Use Case: Local Development](../usecases/local-development.md) for more details on how to integrate with Hadron.
