Hadron
======
Hadron is the Kubernetes native and CloudEvents native distributed event streaming, event orchestration & messaging platform.

### CertManager Integration
This chart ships with cert-manager integration for managing various TLS certs used by Hadron. It is the recommended way to manage TLS/cryptography needs in the Kubernetes context and is enabled by default. This can be controlled with the `.Values.certManager.enabled` key.
