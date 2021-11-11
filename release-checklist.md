Release Checklist
=================
Checklist items are grouped by component.

## Operator
Updates to the Hadron Operator binary will always require changes to the Operator's chart.

- [ ] Update `hadron-operator/CHANGELOG.md`.
- [ ] Update `hadron-operator/Cargo.toml` version with correct semver bump per changes.
- [ ] Update chart's image tag in `charts/hadron-operator/values.yaml` to match new Operator version.
- [ ] Update chart's `version` and `appVersion` in `charts/hadron-operator/Chart.yaml`.
- [ ] Update changelog annotations in `charts/hadron-operator/Chart.yaml` as needed.
- [ ] Commit changes, tag branch with `operator-vX.Y.Z` and `operator-chart-vX.Y.Z` tags, CI will cut the new releases. CI will also update the Operator's container `latest` tag.

## Operator Chart
These steps are for cases where the Operator chart needed updates, but the Operator itself was not updated.

- [ ] Update chart's `version` and `appVersion` in `charts/hadron-operator/Chart.yaml`.
- [ ] Update changelog annotations in `charts/hadron-operator/Chart.yaml` as needed.
- [ ] Commit changes, tag branch with `operator-chart-vX.Y.Z`, CI will cut the new releases.

## Stream
Updates to the Hadron Stream binary are isolated and only require an independent release.

- [ ] Update `hadron-stream/CHANGELOG.md`.
- [ ] Update `hadron-stream/Cargo.toml` version with correct semver bump per changes.
- [ ] Commit changes, tag branch with `stream-vX.Y.Z`, CI will cut the new releases. CI will also update the Hadron Stream container `latest` tag.

## Core
Updates to the Hadron Core library impact both the Hadron Stream & Hadron Operator packages, however the Core library itself not currently independendently released.

- [ ] Update `hadron-core/CHANGELOG.md`.
- [ ] Update `hadron-core/Cargo.toml` version with correct semver bump per changes.

## CLI
Updates to the Hadron CLI binary are isolated and only require an independent release.

- [ ] Update `hadron-cli/CHANGELOG.md`.
- [ ] Update `hadron-cli/Cargo.toml` version with correct semver bump per changes.
- [ ] Commit changes, tag branch with `cli-vX.Y.Z`, CI will cut the new releases. CI will also update the Hadron CLI container `latest` tag.

## Rust Client
Updates to the Hadron Rust client library are isolated and only require an independent release.

- [ ] Update `hadron-client/CHANGELOG.md`.
- [ ] Update `hadron-client/Cargo.toml` version with correct semver bump per changes.
- [ ] Commit changes, tag branch with `rust-client-vX.Y.Z`, CI will cut the new releases.

## Rust Example Apps
Updates to the Rust example apps (examples/*-transactional-processing/) are isolated and only require independent releases.

- [ ] Update `hadron-client/Cargo.toml` version with correct semver bump per changes.
- [ ] Update the app's `example/**/CHANGELOG.md`.
- [ ] Commit changes, tag branch with `example-{appName}-vX.Y.Z`, CI will cut the new releases. CI will also update the example container's `latest` tag.
