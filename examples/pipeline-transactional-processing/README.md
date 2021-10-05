Pipeline Transactional Processing
=================================

Before getting started, it is expected that you have the following tools installed and available on your PATH:
- [Just](https://github.com/casey/just) (think make ... except better)
- [kubectl](https://kubernetes.io/docs/tasks/tools/)
- [helm](https://helm.sh/)
- [kind](https://kind.sigs.k8s.io/docs/)
- [docker](https://docs.docker.com/get-docker/)

This demo uses a Hadron Pipeline (which can be [found here](https://github.com/hadron-project/hadron/tree/main/charts/hadron-operator/examples/full.yaml)) to model a conceptual "service creation" workflow:
```yaml
apiVersion: hadron.rs/v1beta1
kind: Pipeline
metadata:
  name: service-creation
  namespace: default
spec:
  sourceStream: events
  triggers:
    - service.created
  maxParallel: 50
  startPoint:
    location: beginning
  stages:
    - name: deploy-service

    - name: setup-billing
      dependencies: ["deploy-service"]
    - name: setup-monitoring
      dependencies: ["deploy-service"]
    - name: notify-user
      dependencies: ["deploy-service"]

    - name: cleanup
      dependencies:
        - deploy-service
        - setup-billing
        - setup-monitoring
        - notify-user
```

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
