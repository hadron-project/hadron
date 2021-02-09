# Create a kind cluster used for local development.
kindCreateCluster:
    kind create cluster --name hadron

# Build Hadron docker image.
dockerBuild:
    docker-compose run builder && docker-compose build --no-cache hadron

# Load Hadron docker image into kind cluster.
kindLoadImage:
    kind load docker-image --name hadron hadron-local-release:latest

# Perform a upgrade --install of the Hadron chart into the kind cluster.
helmUp:
    helm --kube-context="kind-hadron" upgrade hadron ./kubernetes/helm -i \
        --set image.fullName=hadron-local-release:latest \
        --set image.pullPolicy=Never \
        --set-string statefulSet.replicas=3
