Stream Transactional Processing
===============================

Before getting started, it is expected that you have the following tools installed and available on your PATH:
- [Just](https://github.com/casey/just) (think make ... except better)
- [kubectl](https://kubernetes.io/docs/tasks/tools/)
- [helm](https://helm.sh/)
- [kind](https://kind.sigs.k8s.io/docs/)
- [docker](https://docs.docker.com/get-docker/)

```sh
# Create the kind development cluster used for development in this repo.
# NOTE: this command is defined in the repo's root Justfile.
just kindCreateCluster

# Deploy cert-manager.
# NOTE: this command is defined in the repo's root Justfile.
just helmUpCertManager

# Deploy the Hadron Operator.
# NOTE: this command is defined in the repo's root Justfile.
just helmUp

# Get a Postgres database running in the cluster for application state.
just helmUpPostgres

# Deploy our demo app.
#
# This will deploy the Stream used by this example app. Database schema
# migrations are handled by the app.
just deployDemoApp

# NOTE: if experimenting with the demo app, and you've made changes that
# you would like to deploy to the cluster, the following commands will help:
just buildDemoApp "my-tag" # Build new docker image.
just kindLoadDemoApp "my-tag" # Load docker image into kind cluster.
just deployDemoApp "my-tag" # Deploy demo app using new tag.
```
