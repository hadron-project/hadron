buildHadrond:
    cargo build --manifest-path hadrond/Cargo.toml --release

kindCreateCluster:
    kind create cluster --name hadron

buildAndLoadKind:
    docker-compose run builder && \
        docker-compose build --no-cache hadron && \
        kind load docker-image --name hadron hadron-local-release:latest

helmUp:
    helm --kube-context="kind-hadron" upgrade hadron ./kubernetes/helm -i \
        --set image.fullName=hadron-local-release:latest \
        --set image.pullPolicy=Never \
        --set-string statefulSet.replicas=3
