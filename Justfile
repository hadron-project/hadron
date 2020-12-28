build-hadrond:
    cargo build --manifest-path hadrond/Cargo.toml --release

kind-cluster:
    kind create cluster --name hadron

build-and-load-kind:
    docker-compose run builder && \
        docker-compose build --no-cache hadron && \
        kind load docker-image --name hadron hadron-local-release:latest

helm-upgrade-kind:
    helm upgrade hadron ./kubernetes/helm -i \
        --set image.fullName=hadron-local-release:latest \
        --set image.pullPolicy=Never \
        --set-string statefulSet.replicas=3
