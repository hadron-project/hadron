Stream Transactional Processing
===============================

Before getting started, it is expected that you have the following tools installed and available on your PATH:
- [Just](https://github.com/casey/just) (think make ... except better)
- [kubectl](https://kubernetes.io/docs/tasks/tools/)
- [helm](https://helm.sh/)
- [kind](https://kind.sigs.k8s.io/docs/)
- [docker](https://docs.docker.com/get-docker/)

Now, execute the following commands to get the demo app up and running.
```sh
# Create the kind development cluster used for development in this repo.
just kindCreateCluster

# Deploy cert-manager.
just helmUpCertManager

# Deploy the Hadron Operator.
just helmUp

# Get a Postgres database running in the cluster for application state.
just helmUpPostgres

# Apply Hadron example resources for the demo.
#
# The Hadron resources used are found here:
# https://github.com/hadron-project/hadron/tree/main/charts/hadron-operator/examples/full.yaml
just applyExample

# Deploy our demo app.
just deployDemoApp

# NOTE: if experimenting with the demo app, and you've made changes that
# you would like to deploy to the cluster, the following commands will help:
just buildDemoApp "my-tag" # Build new docker image.
just kindLoadDemoApp "my-tag" # Load docker image into kind cluster.
just deployDemoApp # Deploy demo app. Update `deployment.yaml` as needed.
```

Once the demo app is up and running, you will need to publish some data to the Hadron stream in order to see it in action. Let's use the Hadron CLI for this: `just runCli`. Once this command completes, your shell will be connected to a Hadron CLI instance running within the kind cluster with credentials in place for accessing the Hadron cluster.

To learn how to publish data, execute `hadron stream pub -h`.
