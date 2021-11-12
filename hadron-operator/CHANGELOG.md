# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Added
- Stream StatefulSet config for metrics ports & updated Stream StatefulSet pod spec to expose metrics port on the container.
- Added Prometheus metrics covering:
    - Number of leadership changes.
    - Leadership state (1.0 == leader, anything else is follower).
    - Error counter for K8s resource watchers.
    - Process metrics.

## [0.1.0-beta.1] - 2021-11-10
### Added
- Support for Stream data retention policies per [#99](https://github.com/hadron-project/hadron/issues/99).
