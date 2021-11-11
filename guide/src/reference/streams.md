Streams
=======
Streams are append-only, immutable logs of data with absolute ordering per partition.

Streams are deployed as independent StatefulSets within a Kubernetes cluster, based purely on a Stream CRD stored in Kubernetes.

Each pod of a Stream's StatefulSet acts as a leader of its own partition. Kubernetes guarantees the identity, stable storage and stable network address of each pod. Replication is based on a deterministic algorithm and does not require consensus due to these powerful identity and stability properties.

```yaml
apiVersion: hadron.rs/v1beta1
kind: Stream
metadata:
  ## The name of this Stream.
  ##
  ## The generated Kubernetes StatefulSet of this Stream will have the same name.
  name: :string
  ## The Kubernetes namespace of this Stream.
  ##
  ## The generated Kubernetes Secret will live in the same namespace.
  namespace: :string
spec:
  ## The number of partitions this Stream should have.
  ##
  ## This directly corresponds to the number of replicas of the
  ## backing StatefulSet of this Stream. Scaling this value up or down will
  ## cause the backing StatefulSet to be scaled identically.
  partitions: :integer

  ## When true, the Stream processes will use `debug` level logging.
  debug: :bool

  ## The data retention policy for data residing on this Stream.
  retentionPolicy:
    ## The retention strategy to use.
    ##
    ## Allowed values:
    ## - "retain": this strategy preserves the data on the Stream indefinitely.
    ## - "time": this strategy preserves the data on the Stream for the amount of time
    ##   specified in `.spec.retentionPolicy.retentionSeconds`.
    strategy: :string (default "time")
    ## The number of seconds which data is to be retained on the Stream before it is deleted.
    ##
    ## This field is optional and is only evaluated when using the "time" strategy.
    retentionSeconds: :integer (default 604800) # Default 7 days.

  ## The full image name, including tag, to use for the backing StatefulSet pods.
  image: :string

  ## The PVC volume size for each pod of the backing StatefulSet.
  pvcVolumeSize: :string

  ## The PVC storage class to use for each pod of the backing StatefulSet.
  pvcStorageClass: :string

  ## The PVC access modes to use for each pod of the backing StatefulSet.
  pvcAccessModes: [:string]
```
