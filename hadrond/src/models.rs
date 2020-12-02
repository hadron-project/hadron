//! Data Definition Language (DDL) models.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// All Data Definition Language variants availabe in Hadron.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind")]
pub enum DDL {
    #[serde(alias = "stream")]
    Stream(Stream),
    #[serde(alias = "pipeline")]
    Pipeline(Pipeline),
}

impl DDL {
    /// Deserialize a series of DDL objects from the given str.
    pub fn from_str(ddl: &str) -> Result<Vec<Self>> {
        let data: Vec<Self> = serde_yaml::from_str_multidoc(ddl).context("error deserializing ddl")?;
        Ok(data)
    }
}

/// A durable log of events.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Stream {
    /// The name of this stream.
    pub name: String,
    /// The current revision number of this object.
    pub revision: u64,
}

/// A multi-stage data workflow.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Pipeline {
    /// The name of this pipeline.
    pub name: String,
    /// The name of the stream which can trigger this pipeline.
    #[serde(rename = "triggerStream")]
    pub trigger_stream: String,
    /// The current revision number of this object.
    pub revision: u64,
    /// The stages of this pipeline.
    pub stages: Vec<PipelineStage>,
}

/// A single pipeline stage.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct PipelineStage {
    /// The name of this pipeline stage, which is unique per pipeline.
    pub name: String,
    /// All stages which must complete before this stage may be started.
    pub after: Option<Vec<String>>,
    /// All inputs which this stage depends upon in order to be started.
    pub dependencies: Option<Vec<String>>,
    /// All outputs which this stage must produce in order to complete successfully.
    pub outputs: Option<Vec<PipelineStageOutput>>,
}

/// A pipeline stage output definition.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct PipelineStageOutput {
    /// The name of this pipeline stage output, which is unique per pipeline stage.
    pub name: String,
    /// The name of the stream to which this output event is to be published.
    pub stream: String,
}

#[cfg(test)]
mod test {
    use super::*;

    const DDL_SPEC: &str = r###"---
kind: stream
name: services
revision: 0

---
kind: stream
name: deployer
revision: 0

---
kind: stream
name: billing
revision: 0

---
kind: stream
name: monitoring
revision: 0

---
kind: pipeline
name: instance-creation
triggerStream: services
revision: 0
stages:
  - name: deploy-service
    outputs:
      - name: deploy-complete
        stream: deployer

  - name: update-billing
    dependencies:
      - root_event
      - deploy-service.deploy-complete
    outputs:
      - name: billing-updated
        stream: billing
  - name: update-monitoring
    dependencies:
      - root_event
      - deploy-service.deploy-complete
    outputs:
      - name: monitoring-updated
        stream: monitoring

  - name: cleanup
    dependencies:
      - root_event
      - deploy-service.deploy-complete
      - update-billing.billing-updated
      - update-monitoring.monitoring-updated
    outputs:
      - name: new-service-deployed
        stream: services
"###;

    #[test]
    fn test_deserialize_ddl() -> Result<()> {
        let expected = vec![
            DDL::Stream(Stream {
                name: "services".into(),
                revision: 0,
            }),
            DDL::Stream(Stream {
                name: "deployer".into(),
                revision: 0,
            }),
            DDL::Stream(Stream {
                name: "billing".into(),
                revision: 0,
            }),
            DDL::Stream(Stream {
                name: "monitoring".into(),
                revision: 0,
            }),
            DDL::Pipeline(Pipeline {
                name: "instance-creation".into(),
                trigger_stream: "services".into(),
                revision: 0,
                stages: vec![
                    PipelineStage {
                        name: "deploy-service".into(),
                        after: None,
                        dependencies: None,
                        outputs: Some(vec![PipelineStageOutput {
                            name: "deploy-complete".into(),
                            stream: "deployer".into(),
                        }]),
                    },
                    PipelineStage {
                        name: "update-billing".into(),
                        after: None,
                        dependencies: Some(vec!["root_event".into(), "deploy-service.deploy-complete".into()]),
                        outputs: Some(vec![PipelineStageOutput {
                            name: "billing-updated".into(),
                            stream: "billing".into(),
                        }]),
                    },
                    PipelineStage {
                        name: "update-monitoring".into(),
                        after: None,
                        dependencies: Some(vec!["root_event".into(), "deploy-service.deploy-complete".into()]),
                        outputs: Some(vec![PipelineStageOutput {
                            name: "monitoring-updated".into(),
                            stream: "monitoring".into(),
                        }]),
                    },
                    PipelineStage {
                        name: "cleanup".into(),
                        after: None,
                        dependencies: Some(vec![
                            "root_event".into(),
                            "deploy-service.deploy-complete".into(),
                            "update-billing.billing-updated".into(),
                            "update-monitoring.monitoring-updated".into(),
                        ]),
                        outputs: Some(vec![PipelineStageOutput {
                            name: "new-service-deployed".into(),
                            stream: "services".into(),
                        }]),
                    },
                ],
            }),
        ];
        let data = DDL::from_str(DDL_SPEC)?;
        assert_eq!(expected, data, "processed DDL did not match expected DDL");
        Ok(())
    }
}
