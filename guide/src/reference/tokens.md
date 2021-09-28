Tokens
======
Hadron uses a simple authentication and authorization model which works seamlessly with Kubernetes.

All authentication in Hadron is performed via Tokens. Tokens are Hadron CRDs, which are recognized and processed by the Operator. Token CRs result in a JWT being created with the Operator's private RSA key. The generated JWT is then written to a Kuberetes Secret in the same namespace as the Token, and will bear the same name as the Token.

```yaml
apiVersion: hadron.rs/v1beta1
kind: Token
metadata:
  ## The name of this Token.
  ##
  ## The generated Kubernetes Secret will have the same name.
  name: :string
  ## The Kubernetes namespace of this Token.
  ##
  ## The generated Kubernetes Secret will live in the same namespace.
  namespace: :string
spec:
  ## Grant full access to all resources of the cluster.
  ##
  ## If this value is true, then all other values are
  ## ignored when establishing authorization.
  all: :bool

  ## Pub/Sub access for Streams by name.
  ##
  ## Permissions granted on a Stream extend to any Pipelines
  ## associated with that Stream.
  streams:
    ## The names of all Streams which this Token can publish to.
    pub: [:string]
    ## The names of all Streams which this Token can subscribe to.
    sub: [:string]

  ## Pub/Sub access for Exchanges by name.
  exchanges:
    ## The names of all Exchanges which this Token can publish to.
    pub: [:string]
    ## The names of all Exchanges which this Token can subscribe to.
    sub: [:string]

  ## Pub/Sub access for Endpoints by name.
  endpoints:
    ## The names of all Endpoints which this Token can publish to.
    pub: [:string]
    ## The names of all Endpoints which this Token can subscribe to.
    sub: [:string]
```
