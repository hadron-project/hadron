# Create a kind cluster used for local development.
kindCreateCluster:
    kind create cluster --name hadron

# Build the Hadron CLI docker image.
buildCli mode="debug" tag="latest":
    #!/usr/bin/env bash
    set -euo pipefail
    case '{{mode}}' in
    'debug') opts='';;
    'release') opts='--build-arg=RELEASE_OPT=--release';;
    *) echo 'unrecognized value for `mode`, must be either `debug` or `release`'; exit 1;;
    esac

    docker build ${opts} --target builder -f hadron-cli/Dockerfile .
    docker build ${opts} --target release -f hadron-cli/Dockerfile -t ghcr.io/hadron-project/hadron/hadron-cli:{{tag}} .

# Load the Hadron CLI docker image into kind cluster.
kindLoadCli tag="latest":
    kind load docker-image --name hadron ghcr.io/hadron-project/hadron/hadron-cli:{{tag}}

# Run the Hadron CLI within the kind cluster.
runCli num="0" tag="latest" pullPolicy="IfNotPresent":
    kubectl --context="kind-hadron" run hadron-cli-{{num}} --rm -it \
        --env HADRON_TOKEN=$(kubectl get secret hadron-full-access -o=jsonpath='{.data.token}' | base64 --decode) \
        --env HADRON_URL="http://events.default.svc.cluster.local:7000" \
        --image ghcr.io/hadron-project/hadron/hadron-cli:{{tag}} --image-pull-policy={{pullPolicy}}

# Build the Hadron Operator docker image.
buildOperator mode="debug" tag="latest":
    #!/usr/bin/env bash
    set -euo pipefail
    case '{{mode}}' in
    'debug') opts='';;
    'release') opts='--build-arg=RELEASE_OPT=--release';;
    *) echo 'unrecognized value for `mode`, must be either `debug` or `release`'; exit 1;;
    esac

    docker build ${opts} --target builder -f hadron-operator/Dockerfile .
    docker build ${opts} --target release -f hadron-operator/Dockerfile -t ghcr.io/hadron-project/hadron/hadron-operator:{{tag}} .

# Load the Hadron Operator docker image into kind cluster.
kindLoadOperator tag="latest":
    kind load docker-image --name hadron ghcr.io/hadron-project/hadron/hadron-operator:{{tag}}

# Build the Hadron Stream docker image.
buildStream mode="debug" tag="latest":
    #!/usr/bin/env bash
    set -euo pipefail
    case '{{mode}}' in
    'debug') opts='';;
    'release') opts='--build-arg=RELEASE_OPT=--release';;
    *) echo 'unrecognized value for `mode`, must be either `debug` or `release`'; exit 1;;
    esac

    docker build ${opts} --target builder -f hadron-stream/Dockerfile .
    docker build ${opts} --target release -f hadron-stream/Dockerfile -t ghcr.io/hadron-project/hadron/hadron-stream:{{tag}} .

# Load the Hadron Stream docker image into kind cluster.
kindLoadStream tag="latest":
    kind load docker-image --name hadron ghcr.io/hadron-project/hadron/hadron-stream:{{tag}}

# Perform a upgrade --install of the Hadron chart into the kind cluster.
helmUp tag="latest" pull="IfNotPresent" prom="false":
    helm --kube-context="kind-hadron" upgrade hadron-operator ./charts/hadron-operator -i \
        --set container.image.tag={{tag}},container.image.pullPolicy={{pull}} \
        --set prometheusOperator.enabled={{prom}},prometheusOperator.serviceMonitor.labels.release=monitoring \
        --set prometheusOperator.podMonitor.labels.release=monitoring

# Purge all Hadron data in the local kind cluster.
helmPurge:
    -helm --kube-context="kind-hadron" uninstall hadron-operator
    -kubectl --context="kind-hadron" delete deployments,statefulsets -l app=hadron
    -kubectl --context="kind-hadron" delete persistentvolumeclaims -l app=hadron
    -kubectl --context="kind-hadron" delete leases -l app=hadron
    -kubectl --context="kind-hadron" delete streams,pipelines,tokens --all
    -kubectl --context="kind-hadron" delete crds pipelines.hadron.rs streams.hadron.rs tokens.hadron.rs
    -kubectl --context="kind-hadron" delete secrets -l app=hadron
    -kubectl --context="kind-hadron" delete services -l app=hadron
    -kubectl --context="kind-hadron" delete ingresses -l app=hadron

# Apply example CRs to the cluster.
applyExample:
    kubectl --context="kind-hadron" apply -f ./charts/hadron-operator/examples/full.yaml

# Generate Hadron CRDs.
genCrds:
    cd hadron-core && cargo run --example crd

# Generate Hadron CRDs.
genProtoClient:
    cd hadron-client && cargo run --example genproto

# Setup cert-manager for use in kind development cluster.
helmUpCertManager:
    helm repo add jetstack https://charts.jetstack.io
    helm --kube-context="kind-hadron" upgrade cert-manager jetstack/cert-manager --install \
        --version v1.5.3 --set installCRDs=true

# Create a new packaged chart for the Hadron Operator.
helmPackageOperator:
    helm package charts/hadron-operator -d charts/

# Push the target Hadron Operator chart package, matching the given version, to the OCI registry.
helmPushOperator version:
    helm push charts/hadron-operator-{{version}}.tgz oci://ghcr.io/hadron-project/charts/ && \
        rm charts/hadron-operator-{{version}}.tgz

# Update the artifacthub-repo.yaml for ArtifactHub.
helmUpdateArtifactHubRepo:
    # See https://oras.land/ && https://github.com/oras-project/oras
    oras push ghcr.io/hadron-project/charts/hadron-operator:artifacthub.io \
        --manifest-config /dev/null:application/vnd.cncf.artifacthub.config.v1+yaml \
        charts/hadron-operator/artifacthub-repo.yml:application/vnd.cncf.artifacthub.repository-metadata.layer.v1.yaml

helmUpKubePromStack:
    helm repo add prometheus-community https://prometheus-community.github.io/helm-charts
    helm --kube-context="kind-hadron" upgrade monitoring prometheus-community/kube-prometheus-stack --install \
        --set fullnameOverride=monitoring
