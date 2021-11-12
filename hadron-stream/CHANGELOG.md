# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Added
- Prometheus metrics instrumentation covering:
    - K8s resource watcher errors (counter).
    - Stream current offset (counter).
    - Stream subscriber count (gauge).
    - Stream subscriber group last offset processed (counter).
    - Per Pipeline last offset processed (counter).
    - Per Pipeline active instances (gauge).
    - Per Pipeline number of stage subscribers (gauge).
    - Process metrics.

## [0.1.0-beta.1] - 2021-11-10
### Added
- Support for Stream data retention policies per [#99](https://github.com/hadron-project/hadron/issues/99).
