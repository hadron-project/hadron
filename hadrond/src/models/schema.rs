//! Data models related to schema management.

// TODO: all validation need to be audited & tested, as lots has evolved.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use chrono::prelude::*;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
pub use crate::models::proto::storage::*;
use crate::models::traits::Namespaced;
use crate::server::MetadataCache;
use crate::utils;

const ROOT_EVENT_TOKEN: &str = "root_event";
const MAX_DESCRIPTION_LEN: usize = 5000;

const ERR_RE_CHANGESET_NAME: &str = r"invalid changeset name, must match the pattern `^[-_.a-zA-Z0-9]{1,255}$`";
const ERR_SCHEMA_UPDATE_BASIC: &str = "schema updates must be composed of an initial changeset followed by one or more DDL statements";

lazy_static! {
    static ref ERR_DESCRIPTION_LEN: String = format!("descriptions may contain a maximum of {} characters", MAX_DESCRIPTION_LEN);
    /// Regular expression used to validate top-level resource names for which hierarchies may be formed.
    static ref RE_NAME: Regex = Regex::new(r"^[-_.a-zA-Z0-9]{1,100}$").expect("failed to compile RE_NAME regex");
    /// Regular expression used to validate pipeline stage & output names.
    static ref RE_BASIC_NAME: Regex = Regex::new(r"^[-_a-zA-Z0-9]{1,100}$").expect("failed to compile RE_BASIC_NAME regex");
}

/// Deserialize a set of schema statements from the given input, statically validate each
/// statement & then dynamically validate the end results using the given metadata cache.
pub fn decode_and_validate(input: &str, cache: Arc<MetadataCache>) -> Result<Vec<SchemaStatement>> {
    // Decode statements.
    let statements: Vec<SchemaStatement> =
        serde_yaml::from_str_multidoc(&input).map_err(|err| AppError::InvalidInput(format!("error parsing schema update: {}", err)))?;

    // Perform static validation on all schema statements.
    for statement in statements.iter() {
        statement.validate()?;
    }

    // TODO: Perform dynamic validation on content using the given cache.

    Ok(statements)
}

/// All available schema statements in Hadron.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum SchemaStatement {
    /// A namespace definition.
    Namespace(Namespace),
    /// A stream definition.
    Stream(Stream),
    /// A pipeline definition.
    Pipeline(Pipeline),
    /// An RPC endpoint definition.
    Endpoint(Endpoint),
}

impl SchemaStatement {
    /// Statically validate this object.
    fn validate(&self) -> Result<()> {
        match self {
            Self::Namespace(val) => val.validate(),
            Self::Stream(val) => val.validate(),
            Self::Pipeline(val) => val.validate(),
            Self::Endpoint(val) => Ok(()), // TODO: finish this up.
        }
    }
}

impl Namespace {
    fn validate(&self) -> Result<()> {
        ensure!(
            RE_BASIC_NAME.is_match(&self.name),
            AppError::InvalidInput(format!(
                "namespace name `{}` is invalid, must match the pattern `{}`",
                &self.name,
                RE_BASIC_NAME.as_str()
            ))
        );
        Ok(())
    }
}

impl Stream {
    fn validate(&self) -> Result<()> {
        ensure!(
            RE_NAME.is_match(&self.metadata.name),
            AppError::InvalidInput(format!(
                "stream name `{}` is invalid, must match the pattern `{}`",
                &self.metadata.name,
                RE_NAME.as_str()
            ))
        );
        utils::validate_name_hierarchy(&self.metadata.name)?;
        ensure!(
            RE_BASIC_NAME.is_match(&self.metadata.namespace),
            AppError::InvalidInput(format!(
                "namespace `{}` of stream `{}` is invalid, must match the pattern `{}`",
                &self.metadata.namespace,
                &self.metadata.name,
                RE_BASIC_NAME.as_str()
            ))
        );
        if self.metadata.description.len() > MAX_DESCRIPTION_LEN {
            bail!(AppError::InvalidInput(ERR_DESCRIPTION_LEN.clone()));
        }
        ensure!(
            !self.partitions.is_empty(),
            AppError::InvalidInput(format!("stream `{}` is invalid, must have at least 1 partition", &self.metadata.name))
        );
        Ok(())
    }
}

impl Pipeline {
    fn validate(&self) -> Result<()> {
        // Validate name.
        ensure!(
            RE_NAME.is_match(&self.metadata.name),
            AppError::InvalidInput(format!(
                "pipeline name `{}` is invalid, must match the pattern `{}`",
                &self.metadata.name,
                RE_NAME.as_str()
            ))
        );
        utils::validate_name_hierarchy(&self.metadata.name)?;
        // Validate description.
        if self.metadata.description.len() > MAX_DESCRIPTION_LEN {
            bail!(AppError::InvalidInput(format!(
                "pipeline `{}` description may have maximum length of {} characters",
                self.metadata.name, MAX_DESCRIPTION_LEN
            )));
        }
        // Validate trigger name as a stream name.
        ensure!(
            RE_NAME.is_match(&self.input_stream),
            AppError::InvalidInput(format!(
                "pipeline `{}` stream input name `{}` is invalid, must match the pattern `{}`",
                &self.metadata.name,
                &self.input_stream,
                RE_NAME.as_str()
            ))
        );
        utils::validate_name_hierarchy(&self.input_stream)?;
        // Validate stages.
        if self.stages.is_empty() {
            bail!(AppError::InvalidInput(format!(
                "pipeline `{}` must have at least one stage defined",
                self.metadata.name
            )));
        }
        // Build a set of all stage names, ensuring the names are valid and no dups.
        let mut has_entrypoint = false;
        let stage_to_outputs = self.stages.iter().try_fold(HashMap::new(), |mut acc, stage| {
            // Validate stage name.
            ensure!(
                RE_BASIC_NAME.is_match(&stage.name),
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage name `{}` is invalid, must match the pattern `{}`",
                    &self.metadata.name,
                    &stage.name,
                    RE_BASIC_NAME.as_str()
                ))
            );
            // Ensure stage name is not duplicated.
            let is_duplicate = acc.contains_key(stage.name.as_str());
            ensure!(
                !is_duplicate,
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage name `{}` is not unique for this pipeline, all pipeline stages must have unique names",
                    self.metadata.name, &stage.name
                ))
            );
            // Check to see if this is an entrypoint stage.
            let is_implicit_entry = stage.after.is_empty() && stage.dependencies.is_empty();
            let is_explicity_entry = stage.after.is_empty() && stage.dependencies.iter().any(|dep| dep == ROOT_EVENT_TOKEN);
            if is_implicit_entry || is_explicity_entry {
                has_entrypoint = true;
            }
            acc.insert(stage.name.as_str(), &stage.outputs);
            Ok(acc)
        })?;
        ensure!(
            has_entrypoint,
            AppError::InvalidInput(format!(
                "pipeline `{}` must have at least one entrypoint stage, where the stage has no `after` and no `dependencies` \
                entries, or has a single dependency on the `root_event`",
                &self.metadata.name
            ))
        );
        // Validate the remainder of the contents of each stage.
        let mut dedup_buf: HashSet<&String> = HashSet::new(); // Single alloc for dedup of string values.
        for stage in self.stages.iter() {
            stage.validate(&self, &stage_to_outputs, &mut dedup_buf)?;
            dedup_buf.clear();
        }
        Ok(())
    }
}

impl PipelineStage {
    /// Validate the content of this stage.
    fn validate<'a, 'b>(
        &'a self, pipeline: &'b Pipeline, stages: &'b HashMap<&'b str, &'b Vec<PipelineStageOutput>>, dedup_buf: &'b mut HashSet<&'a String>,
    ) -> Result<()> {
        // For every `after` constraint, ensure it references an actual stage of this pipeline.
        // Also ensure there are no dups. This also ensures the name is valid.
        for after in self.after.iter() {
            ensure!(
                stages.contains_key(after.as_str()),
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` after constraint `{}` references a stage which does not exist in this pipeline",
                    &pipeline.metadata.name, &self.name, &after,
                ))
            );
            let is_unique = dedup_buf.insert(&after);
            ensure!(
                is_unique,
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` after constraint `{}` is a duplicate and must be removed",
                    &pipeline.metadata.name, &self.name, &after,
                ))
            );
            ensure!(
                after != &self.name,
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` after constraint `{}` can not reference its own stage",
                    &pipeline.metadata.name, &self.name, &after,
                ))
            );
        }
        dedup_buf.clear();
        // For every `dependency`, ensure it references an actual stage+output of this pipeline.
        // Also ensure there are no dups. This also ensures the names are valid.
        for dep in self.dependencies.iter() {
            ensure!(
                dedup_buf.insert(&dep),
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` dependency `{}` is a duplicate and must be removed",
                    &pipeline.metadata.name, &self.name, &dep
                ))
            );
            if dep == ROOT_EVENT_TOKEN {
                continue;
            }
            let mut split = dep.splitn(2, '.');
            let (stage, output) = match (split.next(), split.next()) {
                (Some(stage), Some(output)) => (stage, output),
                _ => bail!(AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` dependency `{}` is malformed, must follow the pattern of `{{stage}}.{{output}}`",
                    &pipeline.metadata.name, &self.name, &dep
                ))),
            };
            let outputs = stages.get(&stage).ok_or_else(|| {
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` dependency `{}` references a stage `{}` which does not exist in this pipeline",
                    &pipeline.metadata.name, &self.name, &dep, &stage,
                ))
            })?;
            ensure!(
                !outputs.is_empty(),
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` dependency `{}` references a stage `{}` which has no declared outputs",
                    &pipeline.metadata.name, &self.name, &dep, &stage,
                )),
            );
            ensure!(
                outputs.iter().any(|out| out.name == output),
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` dependency `{}` references output `{}` which does not exist for stage `{}`",
                    &pipeline.metadata.name, &self.name, &dep, &output, &stage,
                ))
            );
            ensure!(
                stage != self.name,
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` dependency `{}` can not reference its own stage",
                    &pipeline.metadata.name, &self.name, &dep,
                ))
            );
        }
        dedup_buf.clear();
        // For every output, ensure the output names & stream names are valid.
        for out in self.outputs.iter() {
            ensure!(
                dedup_buf.insert(&out.name),
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` output `{}` has a duplicate name and must be changed or removed",
                    &pipeline.metadata.name, &self.name, &out.name
                ))
            );
            ensure!(
                RE_BASIC_NAME.is_match(&out.name),
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` output `{}` name is invalid, must match the pattern `{}`",
                    &pipeline.metadata.name,
                    &self.name,
                    &out.name,
                    RE_BASIC_NAME.as_str(),
                ))
            );
            ensure!(
                RE_NAME.is_match(&out.stream),
                AppError::InvalidInput(format!(
                    "pipeline `{}` stage `{}` output `{}` stream name `{}` is invalid, must match the pattern `{}`",
                    &pipeline.metadata.name,
                    &self.name,
                    &out.name,
                    &out.stream,
                    RE_NAME.as_str(),
                ))
            );
        }
        dedup_buf.clear();
        Ok(())
    }
}

// // #[cfg(test)]
// // mod test {
// //     use super::*;

// //     const DDL_SPEC: &str = r###"
// // ---
// // kind: changeset
// // name: test/0

// // ---
// // kind: namespace
// // name: example

// // ---
// // kind: stream
// // namespace: example
// // name: services

// // ---
// // kind: stream
// // namespace: example
// // name: deployer

// // ---
// // kind: stream
// // namespace: example
// // name: billing

// // ---
// // kind: stream
// // namespace: example
// // name: monitoring

// // ---
// // kind: pipeline
// // name: instance-creation
// // namespace: example
// // triggerStream: services
// // stages:
// //   - name: deploy-service
// //     outputs:
// //       - name: deploy-complete
// //         stream: deployer
// //         namespace: example

// //   - name: update-billing
// //     dependencies:
// //       - root_event
// //       - deploy-service.deploy-complete
// //     outputs:
// //       - name: billing-updated
// //         stream: billing
// //         namespace: example
// //   - name: update-monitoring
// //     dependencies:
// //       - root_event
// //       - deploy-service.deploy-complete
// //     outputs:
// //       - name: monitoring-updated
// //         stream: monitoring
// //         namespace: example

// //   - name: cleanup
// //     dependencies:
// //       - root_event
// //       - deploy-service.deploy-complete
// //       - update-billing.billing-updated
// //       - update-monitoring.monitoring-updated
// //     outputs:
// //       - name: new-service-deployed
// //         stream: services
// //         namespace: example
// // "###;

// //     fn default_pipeline_stages() -> Vec<PipelineStage> {
// //         vec![
// //             PipelineStage {
// //                 name: "deploy-service".into(),
// //                 after: None,
// //                 dependencies: None,
// //                 outputs: Some(vec![PipelineStageOutput {
// //                     name: "deploy-complete".into(),
// //                     stream: "deployer".into(),
// //                     namespace: "example".into(),
// //                 }]),
// //             },
// //             PipelineStage {
// //                 name: "update-billing".into(),
// //                 after: None,
// //                 dependencies: Some(vec!["root_event".into(), "deploy-service.deploy-complete".into()]),
// //                 outputs: Some(vec![PipelineStageOutput {
// //                     name: "billing-updated".into(),
// //                     stream: "billing".into(),
// //                     namespace: "example".into(),
// //                 }]),
// //             },
// //             PipelineStage {
// //                 name: "update-monitoring".into(),
// //                 after: None,
// //                 dependencies: Some(vec!["root_event".into(), "deploy-service.deploy-complete".into()]),
// //                 outputs: Some(vec![PipelineStageOutput {
// //                     name: "monitoring-updated".into(),
// //                     stream: "monitoring".into(),
// //                     namespace: "example".into(),
// //                 }]),
// //             },
// //             PipelineStage {
// //                 name: "cleanup".into(),
// //                 after: None,
// //                 dependencies: Some(vec![
// //                     "root_event".into(),
// //                     "deploy-service.deploy-complete".into(),
// //                     "update-billing.billing-updated".into(),
// //                     "update-monitoring.monitoring-updated".into(),
// //                 ]),
// //                 outputs: Some(vec![PipelineStageOutput {
// //                     name: "new-service-deployed".into(),
// //                     stream: "services".into(),
// //                     namespace: "example".into(),
// //                 }]),
// //             },
// //         ]
// //     }

// //     fn default_pipeline() -> Pipeline {
// //         Pipeline {
// //             name: "instance-creation".into(),
// //             namespace: "example".into(),
// //             trigger_stream: "services".into(),
// //             description: "".into(),
// //             stages: default_pipeline_stages(),
// //         }
// //     }

// //     #[test]
// //     fn test_deserialize_ddl() -> Result<()> {
// //         let expected = DDLSchemaUpdate {
// //             changeset: ChangeSet { name: "test/0".into() },
// //             statements: vec![
// //                 DDL::Namespace(Namespace {
// //                     name: "example".into(),
// //                     description: "".into(),
// //                 }),
// //                 DDL::Stream(Stream {
// //                     name: "services".into(),
// //                     namespace: "example".into(),
// //                     description: "".into(),
// //                 }),
// //                 DDL::Stream(Stream {
// //                     name: "deployer".into(),
// //                     namespace: "example".into(),
// //                     description: "".into(),
// //                 }),
// //                 DDL::Stream(Stream {
// //                     name: "billing".into(),
// //                     namespace: "example".into(),
// //                     description: "".into(),
// //                 }),
// //                 DDL::Stream(Stream {
// //                     name: "monitoring".into(),
// //                     namespace: "example".into(),
// //                     description: "".into(),
// //                 }),
// //                 DDL::Pipeline(default_pipeline()),
// //             ],
// //         };
// //         let data = DDLSchemaUpdate::decode_and_validate(DDL_SPEC)?;
// //         assert_eq!(expected, data, "processed DDL did not match expected DDL");
// //         Ok(())
// //     }

// //     macro_rules! test_validate_ddl {
// //         (test: $test:ident, expected: $expected:expr, ddl: $ddl:expr $(,)*) => {
// //             #[test]
// //             fn $test() {
// //                 let expected: Option<anyhow::Error> = $expected.map(Into::into);
// //                 let res = $ddl().validate().err().map(|err| err.to_string());
// //                 assert_eq!(res, expected.map(|err| err.to_string()))
// //             }
// //         };
// //     }

// //     type NoneErr = Option<anyhow::Error>;

// //     /////////////////////////////////
// //     // Tests for stream validation //

// //     // TODO: update error message from name hierarchy validation to allow for inclusion of context info
// //     // TODO: must not have cycles between stages w/ after & dependencies

// //     test_validate_ddl!(test: test_stream_name_max_len,
// //         expected: NoneErr::None,
// //         ddl: || Stream { name: "a".repeat(100), namespace: "a".into(), description: "".into() });

// //     test_validate_ddl!(test: test_stream_name_too_long,
// //         expected: Some(AppError::InvalidInput("stream name `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
// //         ddl: || Stream { name: "a".repeat(101), namespace: "a".into(), description: "".into() });

// //     test_validate_ddl!(test: test_stream_name_empty,
// //         expected: Some(AppError::InvalidInput("stream name `` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
// //         ddl: || Stream { name: "".into(), namespace: "a".into(), description: "".into() });

// //     test_validate_ddl!(test: test_stream_name_bad_hierarchy_0,
// //         expected: Some(AppError::InvalidInput("name hierarchy `..` is invalid, segments delimited by `.` may not be empty".into())),
// //         ddl: || Stream { name: "..".into(), namespace: "a".into(), description: "".into() });
// //     test_validate_ddl!(test: test_stream_name_bad_hierarchy_1,
// //         expected: Some(AppError::InvalidInput("name hierarchy `asdf..` is invalid, segments delimited by `.` may not be empty".into())),
// //         ddl: || Stream { name: "asdf..".into(), namespace: "a".into(), description: "".into() });
// //     test_validate_ddl!(test: test_stream_name_bad_hierarchy_2,
// //         expected: Some(AppError::InvalidInput("name hierarchy `asdf..asdf` is invalid, segments delimited by `.` may not be empty".into())),
// //         ddl: || Stream { name: "asdf..asdf".into(), namespace: "a".into(), description: "".into() });

// //     ///////////////////////////////////
// //     // Tests for pipeline validation //

// //     test_validate_ddl!(test: test_pipeline_name_empty,
// //     expected: Some(AppError::InvalidInput("pipeline name `` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.metadata.name = "".into();
// //         pipeline
// //     });

// //     test_validate_ddl!(test: test_pipeline_name_max_len, expected: NoneErr::None,
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.metadata.name = "a".repeat(100);
// //         pipeline
// //     });

// //     test_validate_ddl!(test: test_pipeline_name_too_long,
// //     expected: Some(AppError::InvalidInput("pipeline name `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.metadata.name = "a".repeat(101);
// //         pipeline
// //     });

// //     test_validate_ddl!(test: test_pipeline_name_bad_hierarchy_0,
// //     expected: Some(AppError::InvalidInput("name hierarchy `..` is invalid, segments delimited by `.` may not be empty".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.metadata.name = "..".into();
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_pipeline_name_bad_hierarchy_1,
// //     expected: Some(AppError::InvalidInput("name hierarchy `asdf..` is invalid, segments delimited by `.` may not be empty".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.metadata.name = "asdf..".into();
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_pipeline_name_bad_hierarchy_2,
// //     expected: Some(AppError::InvalidInput("name hierarchy `asdf..asdf` is invalid, segments delimited by `.` may not be empty".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.metadata.name = "asdf..asdf".into();
// //         pipeline
// //     });

// //     test_validate_ddl!(test: test_pipeline_invalid_trigger_name,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stream trigger name `!!!` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.trigger_stream = "!!!".into();
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_pipeline_invalid_trigger_hierarchy,
// //     expected: Some(AppError::InvalidInput("name hierarchy `...` is invalid, segments delimited by `.` may not be empty".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.trigger_stream = "...".into();
// //         pipeline
// //     });

// //     test_validate_ddl!(test: test_pipeline_no_stages,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` must have at least one stage defined".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages.clear();
// //         pipeline
// //     });

// //     test_validate_ddl!(test: test_pipeline_bad_stage_name,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage name `!!!` is invalid, must match the pattern `^[-_a-zA-Z0-9]{1,100}$`".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages[0].name = "!!!".into();
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_pipeline_duplicate_stage_name,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage name `update-billing` is not unique for this pipeline, all pipeline stages must have unique names".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages[0].name = pipeline.stages[1].name.clone();
// //         pipeline
// //     });

// //     test_validate_ddl!(test: test_pipeline_has_no_entry_points,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` must have at least one entrypoint stage, where the stage has no `after` and no `dependencies` entries, or has a single dependency on the `root_event`".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages[0].after = Some(vec![pipeline.stages[1].name.clone()]);
// //         pipeline
// //     });

// //     /////////////////////////////////////////
// //     // Tests for pipeline stage validation //

// //     test_validate_ddl!(test: test_stage_after_clause_references_non_extant_stage,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` after constraint `non-extant-stage` references a stage which does not exist in this pipeline".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages[1].after = Some(vec!["non-extant-stage".into()]);
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_stage_has_duplicate_after_clause,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` after constraint `update-monitoring` is a duplicate and must be removed".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages[1].after = Some(vec![pipeline.stages[2].name.clone(), pipeline.stages[2].name.clone()]);
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_stage_after_clause_is_self_ref,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` after constraint `update-billing` can not reference its own stage".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages[1].after = Some(vec![pipeline.stages[1].name.clone()]);
// //         pipeline
// //     });

// //     test_validate_ddl!(test: test_stage_dep_is_dup,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` dependency `root_event` is a duplicate and must be removed".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages.get_mut(1).map(|stages| stages.dependencies.as_mut().map(|deps| {
// //             let _ = deps.push(deps[0].clone());
// //         }));
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_stage_dep_is_malformed,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` dependency `stage-ref-no-ouput` is malformed, must follow the pattern of `{stage}.{output}`".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages.get_mut(1).map(|stages| stages.dependencies.as_mut().map(|deps| {
// //             deps[0] = "stage-ref-no-ouput".into();
// //         }));
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_stage_dep_ref_to_non_extant_stage,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` dependency `fake-stage.fake-output` references a stage `fake-stage` which does not exist in this pipeline".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages.get_mut(1).map(|stages| stages.dependencies.as_mut().map(|deps| {
// //             deps[0] = "fake-stage.fake-output".into();
// //         }));
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_stage_dep_ref_to_stage_with_no_outputs,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` dependency `no-output.fake-output` references a stage `no-output` which has no declared outputs".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages.push(PipelineStage {
// //             name: "no-output".into(),
// //             after: None,
// //             dependencies: None,
// //             outputs: None,
// //         });
// //         pipeline.stages.get_mut(1).map(|stages| stages.dependencies.as_mut().map(|deps| {
// //             deps[0] = "no-output.fake-output".into();
// //         }));
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_stage_dep_ref_to_stage_with_unknown_output,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `cleanup` dependency `update-monitoring.monitoring-fake-output` references output `monitoring-fake-output` which does not exist for stage `update-monitoring`".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages.get_mut(3).map(|stages| stages.dependencies.as_mut().map(|deps| {
// //             deps.push("update-monitoring.monitoring-fake-output".into());
// //         }));
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_stage_dep_ref_to_self,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` dependency `update-billing.billing-updated` can not reference its own stage".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages.get_mut(1).map(|stages| stages.dependencies.as_mut().map(|deps| {
// //             deps.push("update-billing.billing-updated".into()); // Self ref.
// //         }));
// //         pipeline
// //     });

// //     test_validate_ddl!(test: test_stage_output_is_duplicate,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `deploy-service` output `deploy-complete` has a duplicate name and must be changed or removed".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages.get_mut(0).map(|stages| stages.outputs.as_mut().map(|outputs| {
// //             outputs.push(PipelineStageOutput {
// //                 name: "deploy-complete".into(),
// //                 stream: "deployer".into(),
// //                 namespace: "example".into(),
// //             });
// //         }));
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_stage_output_name_invalid,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `deploy-service` output `!!!` name is invalid, must match the pattern `^[-_a-zA-Z0-9]{1,100}$`".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages.get_mut(0).map(|stages| stages.outputs.as_mut().map(|outputs| {
// //             outputs.push(PipelineStageOutput {
// //                 name: "!!!".into(),
// //                 stream: "deployer".into(),
// //                 namespace: "example".into(),
// //             });
// //         }));
// //         pipeline
// //     });
// //     test_validate_ddl!(test: test_stage_output_stream_name_invalid,
// //     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `deploy-service` output `work-complete` stream name `!!!` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
// //     ddl: || {
// //         let mut pipeline = default_pipeline();
// //         pipeline.stages.get_mut(0).map(|stages| stages.outputs.as_mut().map(|outputs| {
// //             outputs.push(PipelineStageOutput {
// //                 name: "work-complete".into(),
// //                 stream: "!!!".into(),
// //                 namespace: "example".into(),
// //             });
// //         }));
// //         pipeline
// //     });
// // }
