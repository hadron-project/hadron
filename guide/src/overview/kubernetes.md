Hadron & Kubernetes
===================

> Kubernetes, also known as K8s, is an open-source system for automating deployment, scaling, and management of containerized applications.
> <br/><small><i>~ [kubernetes.io](https://kubernetes.io/)</i></small>

Kubernetes has become a cornerstone of the modern cloud ecosystem, and Hadron is purpose built for the Kubernetes platform. Hadron is native to Kubernetes. It was born here and it knows the ins and outs.

Hadron is designed from the ground up to take full advantage of Kubernetes and its rich API for deploying and running applications. Building upon this foundation has enabled Hadron to achieve an operational model with a simple setup, which removes performance bottlenecks, simplifies clustering and consensus, provides predictable and clear scalability, and positions Hadron for seamless and intuitive integration with user applications and infrastructure.

Each of the above points merits deeper discussion, all of which are covered in further detail throughout the reference section of this guide. Here are the highlights.

### Simple Setup
Everything related to Hadron operations is handled by the Operator and is driven via Kubernetes configuration files. Provisioning, auto-scaling, networking, access control, all of this is controlled through a few lines of YAML which can be versioned in source control and reviewed as code.

The Hadron Operator handles the entire lifecycle of Hadron clusters, including:
- upgrading a cluster to a new version of Hadron,
- horizontally scaling a cluster,
- adding credentials and access control to a cluster,

All of this and more is declarative and fully managed, which means less operational burden for users.

### Removing Performance Bottlenecks
Horizontally scaling a Hadron cluster is fully dynamic, clients are able to detect cluster topology changes in real-time as they take place, and very importantly: the path which data takes from the moment of publication to the moment it is persisted to disk is direct and simple. No extra network hops. No overhead. Just an HTTP2 data stream directly from the client to a Hadron partition's internal function which writes data to disk.

### Seamless and Intuitive Integration
Hadron clusters are exposed for application and infrastructure integration using canonical Kubernetes patterns for networking and access. Clients receive a stream of cluster metadata and can react in real-time to topology changes to maximize use of horizontal scaling and high-availability.

If your applications are already running in Kubernetes, then integration couldn't be more simple.
