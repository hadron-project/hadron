<h1 align="center">Hadron</h1>
<div align="center">

[![Build Status](https://github.com/hadron-project/hadron/workflows/CI/badge.svg?branch=main)](https://github.com/hadron-project/hadron/actions)
[![Build Status](https://github.com/hadron-project/hadron/workflows/Pages/badge.svg?branch=main)](https://github.com/hadron-project/hadron/actions)
[![Artifact Hub](https://img.shields.io/endpoint?url=https://artifacthub.io/badge/repository/hadron-operator)](https://artifacthub.io/packages/search?repo=hadron-operator)
</div>
<br/>

Hadron is the Kubernetes native and CloudEvents native distributed event streaming, event orchestration & messaging platform.

Hadron is designed to ingest data in the form of events, and to facilitate working with that data in the form of multi-stage structured workflows.

**⎈ Built for Kubernetes** - Hadron was born into the world of Kubernetes and is designed to leverage the Kubernetes platform to the max.

**📬 CloudEvents** - Hadron uses the CloudEvents model for everything going in and coming out.

**⚙️ Operational Simplicity** - Hadron leverages the Kubernetes platform for horizontal scaling, high availability, and ease of application integration.

**📦 Easy Installation** - Deploy Hadron into your cluster with Helm, the Kubernetes package manager.

### CertManager Integration
This chart ships with cert-manager integration for managing various TLS certs used by Hadron. It is the recommended way to manage TLS/cryptography needs in the Kubernetes context and is enabled by default. This can be controlled with the `.Values.certManager.enabled` key.

### Prometheus Operator & kube-prometheus-stack Integration
This chart ships with Prometheus Operator / kube-prometheus-stack integration for automatic monitoring via Prometheus. When `prometheusOperator.enabled=true`, this chart will generate a `ServiceMonitor` CRD instance (from the Prometheus Operator chart) which handles everything needed for automatic monitoring. **Note well:** ensure that `prometheusOperator.serviceMonitor.labels` & `prometheusOperator.podMonitor.labels` are supplied so that the `ServiceMonitor` & `PodMonitor` created by this chart will be detected by your Prometheus deployment. [See the docs here](https://github.com/prometheus-operator/prometheus-operator/blob/v0.52.1/Documentation/user-guides/getting-started.md#include-servicemonitors).
