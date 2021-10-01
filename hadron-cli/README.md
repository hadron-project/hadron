hadron-cli
==========
The Hadron CLI.

The best way to use the Hadron CLI is by launching a temporary pod inside of the Kubernetes cluster where your Hadron cluster is running. This makes access clean, simple, and secure because the credentials never need to leave the cluster. For a Stream named `events` deployed in the `default` namespace, and a Token named `app-token`, the following command will launch the Hadron CLI:

```bash
kubectl run hadron-cli --rm -it \
    --env HADRON_TOKEN=$(kubectl get secret app-token -o=jsonpath='{.data.token}' | base64 --decode) \
    --env HADRON_URL="http://events.default.svc.cluster.local:7000" \
    --image ghcr.io/hadron-project/hadron/hadron-cli:latest
```

Accessing Hadron from outside of the Kubernetes cluster is currently in progress, and details will be added here when ready.
