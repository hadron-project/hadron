# Create a kind cluster used for local development.
kindCreateCluster:
    kind create cluster --name hadron

# Build the Hadron Operator docker image.
buildHadronOperator buildopts="--build-arg=RELEASE_OPT=--release" tag="latest":
    docker build {{buildopts}} --target builder -f hadron-operator/Dockerfile .
    docker build {{buildopts}} --target release -f hadron-operator/Dockerfile -t ghcr.io/hadron-project/hadron/hadron-operator:{{tag}} .

# Load the Hadron Operator docker image into kind cluster.
kindLoadHadronOperator tag="latest":
    kind load docker-image --name hadron ghcr.io/hadron-project/hadron/hadron-operator:{{tag}}

# Build the Hadron Stream docker image.
buildHadronStream buildopts="--build-arg=RELEASE_OPT=--release" tag="latest":
    docker build {{buildopts}} --target builder -f hadron-stream/Dockerfile .
    docker build {{buildopts}} --target release -f hadron-stream/Dockerfile -t ghcr.io/hadron-project/hadron/hadron-stream:{{tag}} .

# Load the Hadron Stream docker image into kind cluster.
kindLoadHadronStream tag="latest":
    kind load docker-image --name hadron ghcr.io/hadron-project/hadron/hadron-stream:{{tag}}

# Perform a upgrade --install of the Hadron chart into the kind cluster.
helmUp:
    helm --kube-context="kind-hadron" upgrade hadron-operator ./k8s/helm -i \
        --set container.image.pullPolicy=Never \
        --set container.env.jwt.encodingKey.value='LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQpNSUlFb3dJQkFBS0NBUUVBeXpOMGFsYy9OMHZ3MVcyV1VkdVRVRDRzZEZMUGtESnJXNU13TVNTU0toQmRDbmd4ClFQWk5CK3JJWkozWjhFUUtKYy80eEdSMU90SXdKTzdIN1JPTWxtT3RtUEZ0K0I5clpnVFF0TnJsZi9KamRTVmcKd3J5dmxYTEdueEhRTUt4NzhvTFFKWmlSNkpqaTRBbFVPT3VhY3RmNWFzTndNc3lWbGZZQjFNYlAvMWh0cThzMAp4ZVdHV1NxazZGa0M1bEJMY2VyNFE5YjdFaDVPVjlhS1VhLzc4akJ0WDJEdVJJcThvVHJVaXJsT1gvbjJRcVJhClNFZ0NYRC9zUEpzeFM3YTAwRW1aSExWbDVZQWZzNTZJbTR1UXFITXNqaGUvT0hjYjJzYktBUi93VjNJNnpjdUoKM0pXY2lUZDhvUnN4VEJjcVYybGJCbXVYUCtFWTYzQ3NQbjhBM3dJREFRQUJBb0lCQUg5YnZMZDUvNUUwODdtUgplZ25NU2NTZGMxZGxIOXNNL3VUaWwrMzFNZmRUVWoxaG45MUxnblJYMzBuUTlldjFlVGJCNXZXUTBUa1F6RVFaCnRRME9sNnNheG85NW9xZEZhaGNESlp4MUppclBUUzc3UW01THZQRTZndXJvUzBoMmt5a29mRFZVTHY5Tjg4VGYKay80cERTQzE1UW5aZk5VYURSZDBuU2t6Z3VKUytGTU5pK2xQZzFrbW5FTGRxRG4wOVhlUE5hc3RBVTlxWEhNLwpwV1NYWkRsOFZJOHBBWFZUMG1sK0o4S0xjd1U0QjNFTUc3dUFoK3dSdWVQMzVkTVhjMjZOaXZrTGx6ZFBIQXZjClpqMG9aNjJOdEk1MEJZTTF1NjgyVjdIVlg0L0lJU2YrVC9xTlhMM01vTTJVS0l2Tnc3WTJDcUdDcWx2R3Ayem0KcDMwK0tzRUNnWUVBNk5EUzlodGdsdm1UOEFjK3dJQnMwZEFWYVQ1VzBiLzRHWHdoYmNvNURIaHZjRXlrTnZDdgpYMnhPZDFHbHJjQnVTT0ZiMTcySlJVYkc4RnVUQlFlU1RJWnJYT0NYd28rTzRtalRKV2ZNNXZyKzJ0SVVibitLCjBYcDJpemRmcHRmd25vTVJMTlNUL2wrcmVNc3Uxd1dpVklIc0I1ZG5HRVVTenBIOEtNa2FyQWNDZ1lFQTMyK3AKY3d6RkJrRWpQKzBocTBkRWxDZGdkU2VqSW1OTmZ2ejgxRTZsUml5eGtjZTlZcWNjOUhoYWtiaEwyOEJva3BMKwp5ZkUwQ1BjWkt3S05BckN6cVRFMDlJV1JGWjV5alY4dkY0NS9DYXNnbDl3ZVVHY3ZLSGRpck1hSFVKSWhadkUxCnR0aTA5dDNISi82VHZZVDNCUWFEeHdKWG5JZmYvc3FML3B4RmZta0NnWUFWMlBMVEVZS2c2RTdQcVg1a0Jpa3cKRHp6VElYeDRObkdMd3JCSVl6K3pRZUlEUWMzdk1lcmpJNERCaGJIc2VqQmZPbmFwNmlsbGpOekNWWFdZZFR0dgpYdlhZUTJNNmFNcFp5TWgzckNQNFVQbDFnMTFUZVRpSHE5eFArQ1JMdmR2Z3BDQUtldkJnWUkzd3lmQmRVVFFJCmhpQ21IYmtZOS9KcDNCMHpucHVZSlFLQmdETEVvV0RsMXVLcjdFRjdOUHBBOEVFbGpWSXhWbXphMldId1E5Wk4KcEZvRWo3aExCU29rUkh6NzE4Qmllc0lNdnZZRzltT1dMYmVYeFowek1DMGJUMGN6U2hBQlJVei9PcElLdloyQgpvRFhuTHpteUp5VW52TnhaL0E2NzhVVUtYeEtQM2grVXI5R2o1THVlcVowWFdEVkpIS29jU3lIaThhOW9BRlV5CjFzdDVBb0dCQUxIVVA2bkI3MjA5Tklrc2s4Uy9RSUhwQXFxR2FzdHVTRjU2TDB0bnN4UTRwU1NMUGF3RElmN2oKSjhrUGt0WE5yZ0gvaGhDc0IxK2FyRXNsZDNjd0JGRGRndXpCK2dVNHphMTdsRGd0Y01iZDZhNkkwSzZTSkd6QQowTXJMU2ptMmJRSG0rcU9veC8wU3ArNFQwWUROeHh2Mk1ZamdDSnpZMU5yOXUvOEVoSnlzCi0tLS0tRU5EIFJTQSBQUklWQVRFIEtFWS0tLS0t' \
        --set container.env.jwt.decodingKey.value='LS0tLS1CRUdJTiBQVUJMSUMgS0VZLS0tLS0KTUlJQklqQU5CZ2txaGtpRzl3MEJBUUVGQUFPQ0FROEFNSUlCQ2dLQ0FRRUF5ek4wYWxjL04wdncxVzJXVWR1VApVRDRzZEZMUGtESnJXNU13TVNTU0toQmRDbmd4UVBaTkIrcklaSjNaOEVRS0pjLzR4R1IxT3RJd0pPN0g3Uk9NCmxtT3RtUEZ0K0I5clpnVFF0TnJsZi9KamRTVmd3cnl2bFhMR254SFFNS3g3OG9MUUpaaVI2SmppNEFsVU9PdWEKY3RmNWFzTndNc3lWbGZZQjFNYlAvMWh0cThzMHhlV0dXU3FrNkZrQzVsQkxjZXI0UTliN0VoNU9WOWFLVWEvNwo4akJ0WDJEdVJJcThvVHJVaXJsT1gvbjJRcVJhU0VnQ1hEL3NQSnN4UzdhMDBFbVpITFZsNVlBZnM1NkltNHVRCnFITXNqaGUvT0hjYjJzYktBUi93VjNJNnpjdUozSldjaVRkOG9Sc3hUQmNxVjJsYkJtdVhQK0VZNjNDc1BuOEEKM3dJREFRQUIKLS0tLS1FTkQgUFVCTElDIEtFWS0tLS0t'

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
    kubectl --context="kind-hadron" apply -f k8s/helm/examples/full.yaml

# Generate Hadron CRDs.
genCrds:
    cd hadron-core && cargo run --example crd
