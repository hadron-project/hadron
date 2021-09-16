Hadron & Kubernetes
===================

> Kubernetes, also known as K8s, is an open-source system for automating deployment, scaling, and management of containerized applications.
> <br/><small><i>~ [kubernetes.io](https://kubernetes.io/)</i></small>

Kubernetes has become a cornerstone of the modern cloud ecosystem, and Hadron is purpose built for the Kubernetes platform. Hadron is native to Kubernetes. It was born here and it knows the ins and outs.

Hadron is designed from the ground up to take full advantage of Kubernetes and its rich API for deploying and running applications. Building upon this foundation has enabled Hadron to achieve an operational model with a simple setup, which removes performance bottlenecks, simplifies clustering and consensus, provides predictable and clear scalability, and positions Hadron for seamless and intuitive integration with user applications and infrastructure.

Each of the above points merits deeper discussion, all of which are covered in further detail throughout the reference section of this guide. Here are the highlights.

### Simple Setup
Most usage of Hadron will start with the Hadron Helm chart. The chart installs the Hadron Operator along with various other resource. Everything related to Hadron operations is handled by the Operator and is driven via Kubernetes configuration files. Provisioning, auto-scaling, networking, access control, all of this is controlled through a few lines of YAML which can be versioned in source control and reviewed as code.

The entire lifecycle of Hadron clusters is handled by the Operator. Upgrading a cluster to a new version of Hadron, horizontally scaling a cluster, adding credentials and access control, all of this and more is declarative and fully managed, which means no operational headaches for users.

### Removing Performance Bottlenecks
Kubernetes offers deployment models which allow Hadron to completely remove the need for its own distributed consensus algorithms in many cases, especially in the write path of publishing data to streams. Without this overhead, Hadron is able to optimize write throughput, replication, and other performance criteria in ways which would not otherwise be feasible.

These wins come with no downsides. Horizontally scaling a cluster is still fully dynamic, clients are still able to detect cluster topology changes in real-time as they take place, but most importantly: the path which data takes from the moment of publication to the moment it is persisted to disk is direct and simple. No extra network hops. No overhead. Just an HTTP2 data stream directly from the client to the Hadron internal function which writes data to disk. All with absolute ordering per partition.

### Clustering and Consensus
Hadron clusters are dynamic, but never contend over leadership. Even as cluster topology changes, there is never any overhead on consensus. Without this overhead, Hadron is able to achieve an excellent performance profile.

### Seamless and Intuitive Integration
Hadron clusters are exposed for application and infrastructure integration using canonical Kubernetes patterns for networking and access. Clients receive a stream of cluster metadata and can react in real-time to topology changes to maximize use of horizontal scaling and high-availability.

If your applications are already running in Kubernetes, then integration couldn't be more simple.
