hadron-cli
==========
The Hadron CLI.

## Usage
Given Hadron's deployment model, there are two ways to use this CLI: `Internal`ly within the Kubernetes cluster where Hadron itself is running, or `External`ly from outside of the cluster in which Hadron is running.

### Internal
This is the most simple approach, as long as access to the Kubernetes cluster is available. For this approach, simply execute a temporary pod using the Hadron CLI docker image.

```bash
kubectl run hadron-cli --rm -it --image ghcr.io/hadron-project/hadron/hadron-cli:latest
```

### External
For external use cases, the CLI will need to be invoked with `-e/--external`, and the URL used to connect to the cluster will depend upon how Hadron has been configured, either using `Ingress`es or `NodePort`s.

#### Ingress
Coming soon.

#### NodePort
Coming soon.
