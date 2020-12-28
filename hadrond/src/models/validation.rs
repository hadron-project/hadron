// //! Validation code for data models.

// use std::collections::{HashMap, HashSet};

// use anyhow::{bail, ensure, Result};
// use lazy_static::lazy_static;
// use regex::Regex;

// use super::{ChangeSet, DDLSchemaUpdate, DDLStatement, Namespace, Pipeline, PipelineStage, PipelineStageOutput, Stream, DDL};
// use crate::error::AppError;
// use crate::proto::storage;
// use crate::utils;

// const ROOT_EVENT_TOKEN: &str = "root_event";
// const MAX_DESCRIPTION_LEN: usize = 5000;

// const ERR_RE_CHANGESET_NAME: &str = r"invalid changeset name, must match the pattern `^[-_.a-zA-Z0-9]{1,255}$`";
// const ERR_SCHEMA_UPDATE_BASIC: &str = "schema updates must be composed of an initial changeset followed by one or more DDL statements";

// lazy_static! {
//     static ref ERR_DESCRIPTION_LEN: String = format!("descriptions may contain a maximum of {} characters", MAX_DESCRIPTION_LEN);
//     /// Regular expression used to validate changeset names.
//     static ref RE_CHANGESET: Regex = Regex::new(r"^[-/_.a-zA-Z0-9]{1,255}$").expect("failed to compile RE_CHANGESET regex");
//     /// Regular expression used to validate top-level resource names for which hierarchies may be formed.
//     static ref RE_NAME: Regex = Regex::new(r"^[-_.a-zA-Z0-9]{1,100}$").expect("failed to compile RE_NAME regex");
//     /// Regular expression used to validate pipeline stage & output names.
//     static ref RE_BASIC_NAME: Regex = Regex::new(r"^[-_a-zA-Z0-9]{1,100}$").expect("failed to compile RE_BASIC_NAME regex");
// }

// impl DDLStatement {
//     /// Statically validate this object.
//     fn validate(&self) -> Result<()> {
//         match self {
//             Self::ChangeSet(val) => val.validate(),
//             Self::Namespace(val) => val.validate(),
//             Self::Stream(val) => val.validate(),
//             Self::Pipeline(val) => val.validate(),
//         }
//     }
// }

// impl DDLSchemaUpdate {
//     /// Deserialize a DDL schema update from the given str & statically validate content.
//     pub fn decode_and_validate(ddl_str: &str) -> Result<DDLSchemaUpdate> {
//         // We initially pull out a vec of DDL statements.
//         let mut ddl: Vec<DDLStatement> =
//             serde_yaml::from_str_multidoc(ddl_str).map_err(|err| AppError::InvalidInput(format!("error parsing schema update: {}", err)))?;
//         // Ensure we have a working set of statements, which must always consist of an initial
//         // changeset statement followed by one or more DDL statements.
//         if ddl.len() < 2 {
//             bail!(AppError::InvalidInput(ERR_SCHEMA_UPDATE_BASIC.into()));
//         }
//         // Extract the initial changeset statement.
//         let changeset = match ddl.remove(0) {
//             DDLStatement::ChangeSet(changeset) => changeset,
//             _ => bail!(AppError::InvalidInput(ERR_SCHEMA_UPDATE_BASIC.into())),
//         };
//         // Perform static validation on all DDL statements.
//         changeset.validate()?;
//         let statements = ddl.into_iter().try_fold(vec![], |mut acc, elem| {
//             elem.validate()?;
//             acc.push(match elem {
//                 DDLStatement::ChangeSet(_) => bail!(AppError::InvalidInput(ERR_SCHEMA_UPDATE_BASIC.into())),
//                 DDLStatement::Namespace(namespace) => DDL::Namespace(namespace),
//                 DDLStatement::Stream(stream) => DDL::Stream(stream),
//                 DDLStatement::Pipeline(pipeline) => DDL::Pipeline(pipeline),
//             });
//             Ok(acc)
//         })?;
//         Ok(DDLSchemaUpdate { changeset, statements })
//     }
// }

// impl ChangeSet {
//     fn validate(&self) -> Result<()> {
//         ensure!(RE_CHANGESET.is_match(&self.name), AppError::InvalidInput(ERR_RE_CHANGESET_NAME.into()));
//         Ok(())
//     }
// }

// impl From<&'_ Namespace> for storage::Namespace {
//     fn from(src: &'_ Namespace) -> Self {
//         Self {
//             name: src.name.clone(),
//             description: src.description.clone(),
//         }
//     }
// }

// impl Namespace {
//     fn validate(&self) -> Result<()> {
//         ensure!(
//             RE_BASIC_NAME.is_match(&self.name),
//             AppError::InvalidInput(format!(
//                 "namespace name `{}` is invalid, must match the pattern `{}`",
//                 &self.name,
//                 RE_BASIC_NAME.as_str()
//             ))
//         );
//         Ok(())
//     }
// }

// impl From<&'_ Stream> for storage::Stream {
//     fn from(src: &'_ Stream) -> Self {
//         Self {
//             name: src.name.clone(),
//             namespace: src.namespace.clone(),
//             description: src.description.clone(),
//         }
//     }
// }

// impl Stream {
//     fn validate(&self) -> Result<()> {
//         ensure!(
//             RE_NAME.is_match(&self.name),
//             AppError::InvalidInput(format!(
//                 "stream name `{}` is invalid, must match the pattern `{}`",
//                 &self.name,
//                 RE_NAME.as_str()
//             ))
//         );
//         utils::validate_name_hierarchy(&self.name)?;
//         if self.description.len() > MAX_DESCRIPTION_LEN {
//             bail!(AppError::InvalidInput(ERR_DESCRIPTION_LEN.clone()));
//         }
//         Ok(())
//     }
// }

// impl From<&'_ Pipeline> for storage::Pipeline {
//     fn from(src: &'_ Pipeline) -> Self {
//         Self {
//             name: src.name.clone(),
//             namespace: src.namespace.clone(),
//             description: src.description.clone(),
//             trigger_stream: src.trigger_stream.clone(),
//             stages: src.stages.iter().map(From::from).collect(),
//         }
//     }
// }

// impl Pipeline {
//     fn validate(&self) -> Result<()> {
//         // Validate name.
//         ensure!(
//             RE_NAME.is_match(&self.name),
//             AppError::InvalidInput(format!(
//                 "pipeline name `{}` is invalid, must match the pattern `{}`",
//                 &self.name,
//                 RE_NAME.as_str()
//             ))
//         );
//         utils::validate_name_hierarchy(&self.name)?;
//         // Validate description.
//         if self.description.len() > MAX_DESCRIPTION_LEN {
//             bail!(AppError::InvalidInput(format!(
//                 "pipeline `{}` description may have maximum length of {} characters",
//                 self.name, MAX_DESCRIPTION_LEN
//             )));
//         }
//         // Validate trigger name as a stream name.
//         ensure!(
//             RE_NAME.is_match(&self.trigger_stream),
//             AppError::InvalidInput(format!(
//                 "pipeline `{}` stream trigger name `{}` is invalid, must match the pattern `{}`",
//                 &self.name,
//                 &self.trigger_stream,
//                 RE_NAME.as_str()
//             ))
//         );
//         utils::validate_name_hierarchy(&self.trigger_stream)?;
//         // Validate stages.
//         if self.stages.is_empty() {
//             bail!(AppError::InvalidInput(format!(
//                 "pipeline `{}` must have at least one stage defined",
//                 self.name
//             )));
//         }
//         // Build a set of all stage names, ensuring the names are valid and no dups.
//         let mut has_entrypoint = false;
//         let stage_to_outputs = self.stages.iter().try_fold(HashMap::new(), |mut acc, stage| {
//             // Validate stage name.
//             ensure!(
//                 RE_BASIC_NAME.is_match(&stage.name),
//                 AppError::InvalidInput(format!(
//                     "pipeline `{}` stage name `{}` is invalid, must match the pattern `{}`",
//                     &self.name,
//                     &stage.name,
//                     RE_BASIC_NAME.as_str()
//                 ))
//             );
//             // Ensure stage name is not duplicated.
//             let is_duplicate = acc.contains_key(stage.name.as_str());
//             ensure!(
//                 !is_duplicate,
//                 AppError::InvalidInput(format!(
//                     "pipeline `{}` stage name `{}` is not unique for this pipeline, all pipeline stages must have unique names",
//                     self.name, &stage.name
//                 ))
//             );
//             // Check to see if this is an entrypoint stage.
//             let is_implicit_entry = stage.after.is_none() && stage.dependencies.is_none();
//             let is_explicity_entry = stage.after.is_none()
//                 && stage
//                     .dependencies
//                     .as_ref()
//                     .map(|deps| deps.len() == 1 && deps[0] == ROOT_EVENT_TOKEN)
//                     .unwrap_or(false);
//             if is_implicit_entry || is_explicity_entry {
//                 has_entrypoint = true;
//             }
//             acc.insert(stage.name.as_str(), &stage.outputs);
//             Ok(acc)
//         })?;
//         ensure!(
//             has_entrypoint,
//             AppError::InvalidInput(format!(
//                 "pipeline `{}` must have at least one entrypoint stage, where the stage has no `after` and no `dependencies` \
//                 entries, or has a single dependency on the `root_event`",
//                 &self.name
//             ))
//         );
//         // Validate the remainder of the contents of each stage.
//         let mut dedup_buf: HashSet<&String> = HashSet::new(); // Single alloc for dedup of string values.
//         for stage in self.stages.iter() {
//             stage.validate(&self, &stage_to_outputs, &mut dedup_buf)?;
//             dedup_buf.clear();
//         }
//         Ok(())
//     }
// }

// impl From<&'_ PipelineStage> for storage::PipelineStage {
//     fn from(src: &'_ PipelineStage) -> Self {
//         Self {
//             name: src.name.clone(),
//             after: src.after.clone().unwrap_or_default(),
//             dependencies: src.dependencies.clone().unwrap_or_default(),
//             outputs: src
//                 .outputs
//                 .as_ref()
//                 .map(|outputs| outputs.iter().map(From::from).collect())
//                 .unwrap_or_default(),
//         }
//     }
// }

// impl PipelineStage {
//     /// Validate the content of this stage.
//     fn validate<'a, 'b>(
//         &'a self, pipeline: &'b Pipeline, stages: &'b HashMap<&'b str, &'b Option<Vec<PipelineStageOutput>>>, dedup_buf: &'b mut HashSet<&'a String>,
//     ) -> Result<()> {
//         // For every `after` constraint, ensure it references an actual stage of this pipeline.
//         // Also ensure there are no dups. This also ensures the name is valid.
//         if let Some(after) = &self.after {
//             for after in after {
//                 ensure!(
//                     stages.contains_key(after.as_str()),
//                     AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` after constraint `{}` references a stage which does not exist in this pipeline",
//                         &pipeline.name, &self.name, &after,
//                     ))
//                 );
//                 let is_unique = dedup_buf.insert(&after);
//                 ensure!(
//                     is_unique,
//                     AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` after constraint `{}` is a duplicate and must be removed",
//                         &pipeline.name, &self.name, &after,
//                     ))
//                 );
//                 ensure!(
//                     after != &self.name,
//                     AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` after constraint `{}` can not reference its own stage",
//                         &pipeline.name, &self.name, &after,
//                     ))
//                 );
//             }
//             dedup_buf.clear();
//         }
//         // For every `dependency`, ensure it references an actual stage+output of this pipeline.
//         // Also ensure there are no dups. This also ensures the names are valid.
//         if let Some(deps) = &self.dependencies {
//             for dep in deps {
//                 ensure!(
//                     dedup_buf.insert(&dep),
//                     AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` dependency `{}` is a duplicate and must be removed",
//                         &pipeline.name, &self.name, &dep
//                     ))
//                 );
//                 if dep == ROOT_EVENT_TOKEN {
//                     continue;
//                 }
//                 let mut split = dep.splitn(2, '.');
//                 let (stage, output) = match (split.next(), split.next()) {
//                     (Some(stage), Some(output)) => (stage, output),
//                     _ => bail!(AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` dependency `{}` is malformed, must follow the pattern of `{{stage}}.{{output}}`",
//                         &pipeline.name, &self.name, &dep
//                     ))),
//                 };
//                 let outputs_opt = stages.get(&stage).ok_or_else(|| {
//                     AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` dependency `{}` references a stage `{}` which does not exist in this pipeline",
//                         &pipeline.name, &self.name, &dep, &stage,
//                     ))
//                 })?;
//                 let outputs = match outputs_opt {
//                     Some(outputs) => outputs,
//                     None => bail!(AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` dependency `{}` references a stage `{}` which has no declared outputs",
//                         &pipeline.name, &self.name, &dep, &stage,
//                     ))),
//                 };
//                 ensure!(
//                     outputs.iter().any(|out| out.name == output),
//                     AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` dependency `{}` references output `{}` which does not exist for stage `{}`",
//                         &pipeline.name, &self.name, &dep, &output, &stage,
//                     ))
//                 );
//                 ensure!(
//                     stage != self.name,
//                     AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` dependency `{}` can not reference its own stage",
//                         &pipeline.name, &self.name, &dep,
//                     ))
//                 );
//             }
//             dedup_buf.clear();
//         }
//         // For every output, ensure the output names & stream names are valid.
//         if let Some(outputs) = &self.outputs {
//             for out in outputs {
//                 ensure!(
//                     dedup_buf.insert(&out.name),
//                     AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` output `{}` has a duplicate name and must be changed or removed",
//                         &pipeline.name, &self.name, &out.name
//                     ))
//                 );
//                 ensure!(
//                     RE_BASIC_NAME.is_match(&out.name),
//                     AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` output `{}` name is invalid, must match the pattern `{}`",
//                         &pipeline.name,
//                         &self.name,
//                         &out.name,
//                         RE_BASIC_NAME.as_str(),
//                     ))
//                 );
//                 ensure!(
//                     RE_NAME.is_match(&out.stream),
//                     AppError::InvalidInput(format!(
//                         "pipeline `{}` stage `{}` output `{}` stream name `{}` is invalid, must match the pattern `{}`",
//                         &pipeline.name,
//                         &self.name,
//                         &out.name,
//                         &out.stream,
//                         RE_NAME.as_str(),
//                     ))
//                 );
//             }
//             dedup_buf.clear();
//         }
//         Ok(())
//     }
// }

// impl From<&'_ PipelineStageOutput> for storage::PipelineStageOutput {
//     fn from(src: &'_ PipelineStageOutput) -> Self {
//         Self {
//             name: src.name.clone(),
//             stream: src.stream.clone(),
//             namespace: src.namespace.clone(),
//         }
//     }
// }

// #[cfg(test)]
// mod test {
//     use super::*;

//     const DDL_SPEC: &str = r###"
// ---
// kind: changeset
// name: test/0

// ---
// kind: namespace
// name: example

// ---
// kind: stream
// namespace: example
// name: services

// ---
// kind: stream
// namespace: example
// name: deployer

// ---
// kind: stream
// namespace: example
// name: billing

// ---
// kind: stream
// namespace: example
// name: monitoring

// ---
// kind: pipeline
// name: instance-creation
// namespace: example
// triggerStream: services
// stages:
//   - name: deploy-service
//     outputs:
//       - name: deploy-complete
//         stream: deployer
//         namespace: example

//   - name: update-billing
//     dependencies:
//       - root_event
//       - deploy-service.deploy-complete
//     outputs:
//       - name: billing-updated
//         stream: billing
//         namespace: example
//   - name: update-monitoring
//     dependencies:
//       - root_event
//       - deploy-service.deploy-complete
//     outputs:
//       - name: monitoring-updated
//         stream: monitoring
//         namespace: example

//   - name: cleanup
//     dependencies:
//       - root_event
//       - deploy-service.deploy-complete
//       - update-billing.billing-updated
//       - update-monitoring.monitoring-updated
//     outputs:
//       - name: new-service-deployed
//         stream: services
//         namespace: example
// "###;

//     fn default_pipeline_stages() -> Vec<PipelineStage> {
//         vec![
//             PipelineStage {
//                 name: "deploy-service".into(),
//                 after: None,
//                 dependencies: None,
//                 outputs: Some(vec![PipelineStageOutput {
//                     name: "deploy-complete".into(),
//                     stream: "deployer".into(),
//                     namespace: "example".into(),
//                 }]),
//             },
//             PipelineStage {
//                 name: "update-billing".into(),
//                 after: None,
//                 dependencies: Some(vec!["root_event".into(), "deploy-service.deploy-complete".into()]),
//                 outputs: Some(vec![PipelineStageOutput {
//                     name: "billing-updated".into(),
//                     stream: "billing".into(),
//                     namespace: "example".into(),
//                 }]),
//             },
//             PipelineStage {
//                 name: "update-monitoring".into(),
//                 after: None,
//                 dependencies: Some(vec!["root_event".into(), "deploy-service.deploy-complete".into()]),
//                 outputs: Some(vec![PipelineStageOutput {
//                     name: "monitoring-updated".into(),
//                     stream: "monitoring".into(),
//                     namespace: "example".into(),
//                 }]),
//             },
//             PipelineStage {
//                 name: "cleanup".into(),
//                 after: None,
//                 dependencies: Some(vec![
//                     "root_event".into(),
//                     "deploy-service.deploy-complete".into(),
//                     "update-billing.billing-updated".into(),
//                     "update-monitoring.monitoring-updated".into(),
//                 ]),
//                 outputs: Some(vec![PipelineStageOutput {
//                     name: "new-service-deployed".into(),
//                     stream: "services".into(),
//                     namespace: "example".into(),
//                 }]),
//             },
//         ]
//     }

//     fn default_pipeline() -> Pipeline {
//         Pipeline {
//             name: "instance-creation".into(),
//             namespace: "example".into(),
//             trigger_stream: "services".into(),
//             description: "".into(),
//             stages: default_pipeline_stages(),
//         }
//     }

//     #[test]
//     fn test_deserialize_ddl() -> Result<()> {
//         let expected = DDLSchemaUpdate {
//             changeset: ChangeSet { name: "test/0".into() },
//             statements: vec![
//                 DDL::Namespace(Namespace {
//                     name: "example".into(),
//                     description: "".into(),
//                 }),
//                 DDL::Stream(Stream {
//                     name: "services".into(),
//                     namespace: "example".into(),
//                     description: "".into(),
//                 }),
//                 DDL::Stream(Stream {
//                     name: "deployer".into(),
//                     namespace: "example".into(),
//                     description: "".into(),
//                 }),
//                 DDL::Stream(Stream {
//                     name: "billing".into(),
//                     namespace: "example".into(),
//                     description: "".into(),
//                 }),
//                 DDL::Stream(Stream {
//                     name: "monitoring".into(),
//                     namespace: "example".into(),
//                     description: "".into(),
//                 }),
//                 DDL::Pipeline(default_pipeline()),
//             ],
//         };
//         let data = DDLSchemaUpdate::decode_and_validate(DDL_SPEC)?;
//         assert_eq!(expected, data, "processed DDL did not match expected DDL");
//         Ok(())
//     }

//     macro_rules! test_validate_ddl {
//         (test: $test:ident, expected: $expected:expr, ddl: $ddl:expr $(,)*) => {
//             #[test]
//             fn $test() {
//                 let expected: Option<anyhow::Error> = $expected.map(Into::into);
//                 let res = $ddl().validate().err().map(|err| err.to_string());
//                 assert_eq!(res, expected.map(|err| err.to_string()))
//             }
//         };
//     }

//     type NoneErr = Option<anyhow::Error>;

//     /////////////////////////////////
//     // Tests for stream validation //

//     // TODO: update error message from name hierarchy validation to allow for inclusion of context info
//     // TODO: must not have cycles between stages w/ after & dependencies

//     test_validate_ddl!(test: test_stream_name_max_len,
//         expected: NoneErr::None,
//         ddl: || Stream { name: "a".repeat(100), namespace: "a".into(), description: "".into() });

//     test_validate_ddl!(test: test_stream_name_too_long,
//         expected: Some(AppError::InvalidInput("stream name `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
//         ddl: || Stream { name: "a".repeat(101), namespace: "a".into(), description: "".into() });

//     test_validate_ddl!(test: test_stream_name_empty,
//         expected: Some(AppError::InvalidInput("stream name `` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
//         ddl: || Stream { name: "".into(), namespace: "a".into(), description: "".into() });

//     test_validate_ddl!(test: test_stream_name_bad_hierarchy_0,
//         expected: Some(AppError::InvalidInput("name hierarchy `..` is invalid, segments delimited by `.` may not be empty".into())),
//         ddl: || Stream { name: "..".into(), namespace: "a".into(), description: "".into() });
//     test_validate_ddl!(test: test_stream_name_bad_hierarchy_1,
//         expected: Some(AppError::InvalidInput("name hierarchy `asdf..` is invalid, segments delimited by `.` may not be empty".into())),
//         ddl: || Stream { name: "asdf..".into(), namespace: "a".into(), description: "".into() });
//     test_validate_ddl!(test: test_stream_name_bad_hierarchy_2,
//         expected: Some(AppError::InvalidInput("name hierarchy `asdf..asdf` is invalid, segments delimited by `.` may not be empty".into())),
//         ddl: || Stream { name: "asdf..asdf".into(), namespace: "a".into(), description: "".into() });

//     ///////////////////////////////////
//     // Tests for pipeline validation //

//     test_validate_ddl!(test: test_pipeline_name_empty,
//     expected: Some(AppError::InvalidInput("pipeline name `` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.name = "".into();
//         pipeline
//     });

//     test_validate_ddl!(test: test_pipeline_name_max_len, expected: NoneErr::None,
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.name = "a".repeat(100);
//         pipeline
//     });

//     test_validate_ddl!(test: test_pipeline_name_too_long,
//     expected: Some(AppError::InvalidInput("pipeline name `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.name = "a".repeat(101);
//         pipeline
//     });

//     test_validate_ddl!(test: test_pipeline_name_bad_hierarchy_0,
//     expected: Some(AppError::InvalidInput("name hierarchy `..` is invalid, segments delimited by `.` may not be empty".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.name = "..".into();
//         pipeline
//     });
//     test_validate_ddl!(test: test_pipeline_name_bad_hierarchy_1,
//     expected: Some(AppError::InvalidInput("name hierarchy `asdf..` is invalid, segments delimited by `.` may not be empty".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.name = "asdf..".into();
//         pipeline
//     });
//     test_validate_ddl!(test: test_pipeline_name_bad_hierarchy_2,
//     expected: Some(AppError::InvalidInput("name hierarchy `asdf..asdf` is invalid, segments delimited by `.` may not be empty".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.name = "asdf..asdf".into();
//         pipeline
//     });

//     test_validate_ddl!(test: test_pipeline_invalid_trigger_name,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stream trigger name `!!!` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.trigger_stream = "!!!".into();
//         pipeline
//     });
//     test_validate_ddl!(test: test_pipeline_invalid_trigger_hierarchy,
//     expected: Some(AppError::InvalidInput("name hierarchy `...` is invalid, segments delimited by `.` may not be empty".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.trigger_stream = "...".into();
//         pipeline
//     });

//     test_validate_ddl!(test: test_pipeline_no_stages,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` must have at least one stage defined".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages.clear();
//         pipeline
//     });

//     test_validate_ddl!(test: test_pipeline_bad_stage_name,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage name `!!!` is invalid, must match the pattern `^[-_a-zA-Z0-9]{1,100}$`".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages[0].name = "!!!".into();
//         pipeline
//     });
//     test_validate_ddl!(test: test_pipeline_duplicate_stage_name,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage name `update-billing` is not unique for this pipeline, all pipeline stages must have unique names".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages[0].name = pipeline.stages[1].name.clone();
//         pipeline
//     });

//     test_validate_ddl!(test: test_pipeline_has_no_entry_points,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` must have at least one entrypoint stage, where the stage has no `after` and no `dependencies` entries, or has a single dependency on the `root_event`".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages[0].after = Some(vec![pipeline.stages[1].name.clone()]);
//         pipeline
//     });

//     /////////////////////////////////////////
//     // Tests for pipeline stage validation //

//     test_validate_ddl!(test: test_stage_after_clause_references_non_extant_stage,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` after constraint `non-extant-stage` references a stage which does not exist in this pipeline".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages[1].after = Some(vec!["non-extant-stage".into()]);
//         pipeline
//     });
//     test_validate_ddl!(test: test_stage_has_duplicate_after_clause,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` after constraint `update-monitoring` is a duplicate and must be removed".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages[1].after = Some(vec![pipeline.stages[2].name.clone(), pipeline.stages[2].name.clone()]);
//         pipeline
//     });
//     test_validate_ddl!(test: test_stage_after_clause_is_self_ref,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` after constraint `update-billing` can not reference its own stage".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages[1].after = Some(vec![pipeline.stages[1].name.clone()]);
//         pipeline
//     });

//     test_validate_ddl!(test: test_stage_dep_is_dup,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` dependency `root_event` is a duplicate and must be removed".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages.get_mut(1).map(|stages| stages.dependencies.as_mut().map(|deps| {
//             let _ = deps.push(deps[0].clone());
//         }));
//         pipeline
//     });
//     test_validate_ddl!(test: test_stage_dep_is_malformed,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` dependency `stage-ref-no-ouput` is malformed, must follow the pattern of `{stage}.{output}`".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages.get_mut(1).map(|stages| stages.dependencies.as_mut().map(|deps| {
//             deps[0] = "stage-ref-no-ouput".into();
//         }));
//         pipeline
//     });
//     test_validate_ddl!(test: test_stage_dep_ref_to_non_extant_stage,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` dependency `fake-stage.fake-output` references a stage `fake-stage` which does not exist in this pipeline".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages.get_mut(1).map(|stages| stages.dependencies.as_mut().map(|deps| {
//             deps[0] = "fake-stage.fake-output".into();
//         }));
//         pipeline
//     });
//     test_validate_ddl!(test: test_stage_dep_ref_to_stage_with_no_outputs,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` dependency `no-output.fake-output` references a stage `no-output` which has no declared outputs".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages.push(PipelineStage {
//             name: "no-output".into(),
//             after: None,
//             dependencies: None,
//             outputs: None,
//         });
//         pipeline.stages.get_mut(1).map(|stages| stages.dependencies.as_mut().map(|deps| {
//             deps[0] = "no-output.fake-output".into();
//         }));
//         pipeline
//     });
//     test_validate_ddl!(test: test_stage_dep_ref_to_stage_with_unknown_output,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `cleanup` dependency `update-monitoring.monitoring-fake-output` references output `monitoring-fake-output` which does not exist for stage `update-monitoring`".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages.get_mut(3).map(|stages| stages.dependencies.as_mut().map(|deps| {
//             deps.push("update-monitoring.monitoring-fake-output".into());
//         }));
//         pipeline
//     });
//     test_validate_ddl!(test: test_stage_dep_ref_to_self,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `update-billing` dependency `update-billing.billing-updated` can not reference its own stage".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages.get_mut(1).map(|stages| stages.dependencies.as_mut().map(|deps| {
//             deps.push("update-billing.billing-updated".into()); // Self ref.
//         }));
//         pipeline
//     });

//     test_validate_ddl!(test: test_stage_output_is_duplicate,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `deploy-service` output `deploy-complete` has a duplicate name and must be changed or removed".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages.get_mut(0).map(|stages| stages.outputs.as_mut().map(|outputs| {
//             outputs.push(PipelineStageOutput {
//                 name: "deploy-complete".into(),
//                 stream: "deployer".into(),
//                 namespace: "example".into(),
//             });
//         }));
//         pipeline
//     });
//     test_validate_ddl!(test: test_stage_output_name_invalid,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `deploy-service` output `!!!` name is invalid, must match the pattern `^[-_a-zA-Z0-9]{1,100}$`".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages.get_mut(0).map(|stages| stages.outputs.as_mut().map(|outputs| {
//             outputs.push(PipelineStageOutput {
//                 name: "!!!".into(),
//                 stream: "deployer".into(),
//                 namespace: "example".into(),
//             });
//         }));
//         pipeline
//     });
//     test_validate_ddl!(test: test_stage_output_stream_name_invalid,
//     expected: Some(AppError::InvalidInput("pipeline `instance-creation` stage `deploy-service` output `work-complete` stream name `!!!` is invalid, must match the pattern `^[-_.a-zA-Z0-9]{1,100}$`".into())),
//     ddl: || {
//         let mut pipeline = default_pipeline();
//         pipeline.stages.get_mut(0).map(|stages| stages.outputs.as_mut().map(|outputs| {
//             outputs.push(PipelineStageOutput {
//                 name: "work-complete".into(),
//                 stream: "!!!".into(),
//                 namespace: "example".into(),
//             });
//         }));
//         pipeline
//     });
// }
