<h1 align="center">Hadron</h1>
<div align="center">

[![Build Status](https://github.com/hadron-project/hadron/workflows/CI/badge.svg?branch=main)](https://github.com/hadron-project/hadron/actions)
[![Build Status](https://github.com/hadron-project/hadron/workflows/Pages/badge.svg?branch=main)](https://github.com/hadron-project/hadron/actions)
[![Artifact Hub](https://img.shields.io/endpoint?url=https://artifacthub.io/badge/repository/hadron-operator)](https://artifacthub.io/packages/search?repo=hadron-operator)
</div>
<br/>

Hadron is the Kubernetes native and CloudEvents native distributed event streaming, event orchestration & messaging platform.

Hadron is designed to ingest data in the form of events, and to facilitate working with that data in the form of multi-stage structured workflows.

**💪 Powerful Event Pipelines** - Hadron Pipelines provide structured workflow orchestration featuring parallel task execution, transactional inputs and outputs, cross-service coordination, and many other features.

**⎈ Built for Kubernetes** - Hadron was born into the world of Kubernetes and is designed to leverage the Kubernetes platform to the max.

**📬 CloudEvents** - Hadron uses the CloudEvents model for everything going in and coming out.

**⚙️ Operational Simplicity** - Hadron leverages the Kubernetes platform for horizontal scaling, high availability, and ease of application integration.

**📦 Easy Installation** - Deploy Hadron into your cluster with Helm, the Kubernetes package manager.

**📡 Monitoring Ready** - Hadron ships with Prometheus metrics instrumentation and easy integration points for CloudNative monitoring.

## Getting Started
Head over to the [Hadron Guide](https://hadron-project.github.io/hadron/) to learn more. A few quick links:
- [Quick Start](https://hadron-project.github.io/hadron/overview/quick-start.html) - Hadron installation and quick start.
- [Streams](https://hadron-project.github.io/hadron/overview/streams.html) - Append-only, immutable logs of data with absolute ordering per partition.
- [Pipelines](https://hadron-project.github.io/hadron/overview/pipelines.html) - Workflow orchestration for data on Streams, providing structured concurrency for arbitrarily complex multi-stage workflows.
- [Produces & Consumers](https://hadron-project.github.io/hadron/overview/producers-consumers.html) - Write data to and read data from Hadron.
- [Clients](https://hadron-project.github.io/hadron/reference/clients.html) - All Hadron client libraries.
- [CLI](https://hadron-project.github.io/hadron/reference/cli.html) - The Hadron CLI.

## Examples
Check out a few end-to-end code examples using Hadron:
- [Pipeline TXP](https://github.com/hadron-project/hadron/tree/main/examples/pipeline-transactional-processing) - a demo application using Hadron Pipelines for transactional event processing. This is what Hadron was designed for and shows the power of modeling entire systems as workflow Pipelines.
- [Stream TXP](https://github.com/hadron-project/hadron/tree/main/examples/stream-transactional-processing) - a demo application using Hadron Streams for transactional event processing. For folks coming from the Kafka world, this example is more directly relatable.

---

### License
Hadron's licensing is still being finalized, however the ultimate licensing goals are simple:
- Keep Hadron open source and available for anyone to use.
- Only the team/company behind Hadron is allowed to offer Hadron as a hosted service for profit.

Without that last licensing provision, Hadron would not survive as an open source project.
