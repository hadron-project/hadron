peer discovery
==============
This document describes how to configure the peer discovery system in Railgun.

### kubernetes
When deploying Railgun in a Kubernetes environment, DNS setup couldn't be more simple. Typically, everything will already be configured on the Kubernetes platform level, and containers will have their DNS configured properly. The only step which will need to be taken is to populate the `RG_DISCOVERY_DNS_NAME` environment variable with the appropriate DNS name so that Railgun cluster peers can be discovered and connected to.

Railgun should be deployed in Kubernetes using a stateful set. If a service is generated for the cluster, it may be any of the standard service types, including a headless service. The `RG_DISCOVERY_DNS_NAME` should be set as follows to ensure all peer IPs are properly resolved: `*.${statefulSetName}`. Typically this is all that will be needed for the DNS peer discovery system to work in Kubernetes.
