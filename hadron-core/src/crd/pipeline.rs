//! Pipeline CRD.
//!
//! The code here is used to generate the actual CRD used in K8s. See examples/crd.rs.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type Pipeline = PipelineCRD; // Mostly to resolve a Rust Analyzer issue.

/// CRD spec for the Pipeline resource.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, CustomResource, JsonSchema)]
#[kube(
    struct = "PipelineCRD",
    status = "PipelineStatus",
    group = "hadron.rs",
    version = "v1",
    kind = "Pipeline",
    namespaced,
    derive = "PartialEq",
    apiextensions = "v1",
    shortname = "pipeline",
    printcolumn = r#"{"name":"Cluster","type":"string","jsonPath":".spec.cluster"}"#,
    printcolumn = r#"{"name":"Source Stream","type":"string","jsonPath":".spec.sourceStream"}"#,
    printcolumn = r#"{"name":"Triggers","type":"string","jsonPath":".spec.triggers"}"#,
    printcolumn = r#"{"name":"Max Parallel","type":"number","jsonPath":".spec.maxParallel"}"#
)]
pub struct PipelineSpec {
    /// The cluster to which this token belongs.
    pub cluster: String,
    /// The name of the stream which will trigger this pipeline.
    #[serde(rename = "sourceStream")]
    pub source_stream: String,
    /// Event type matchers which will trigger this pipeline.
    ///
    /// Values are expressed as a comma-separated list. An empty string will match any event type.
    #[serde(default)]
    pub triggers: String,
    /// The stages of this pipeline.
    pub stages: Vec<PipelineStage>,
    /// The maximum number of pipeline instances which may be executed in parallel per partition.
    #[serde(rename = "maxParallel")]
    pub max_parallel: u32,
}

/// The definition of a Pipeline stage.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct PipelineStage {
    /// The name of this pipeline stage, which is unique per pipeline.
    pub name: String,
    /// All stages which must complete before this stage may be started.
    #[serde(default)]
    pub after: Vec<String>,
    /// All inputs (previous stages) which this stage depends upon in order to be started.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// CRD status object.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct PipelineStatus {}

impl PipelineCRD {
    /// Check if the given event type matches any of this pipeline's triggers.
    ///
    /// ## Rules
    /// - An empty pipeline `triggers` value will match any event type.
    /// - An empty event type being matched is still subject to matching rules. An empty `triggers`
    ///   value or a single wildcard will match it.
    /// - For all other cases, each pipeline trigger will attempt to match the given event type.
    ///   Wildcards are used for hierarchical matching.
    pub fn event_type_matches_triggers(triggers: &str, event_type: &str) -> bool {
        // Empty value matches any event type.
        if triggers.is_empty() {
            return true;
        }

        triggers.split(',').any(|pattern| {
            // An empty pattern matches any event type, though this is not allowed during VAW validation.
            if pattern.is_empty() {
                return true;
            }

            // Default to a true match, and negate at the first contradiction.
            let mut event_segs = event_type.split('.');
            for pat in pattern.split('.') {
                let seg_opt = event_segs.next();
                match seg_opt {
                    Some(seg) => match (pat, seg) {
                        ("*", _seg) => continue,              // Matches this segment, check next.
                        (">", _seg) => break,                 // Matches anything else.
                        (pat, seg) if pat == seg => continue, // Literal match, check next.
                        _ => return false,                    // Any other case is a contradiction.
                    },
                    None => break, // No more event segments to check, and no contradiction was found.
                }
            }
            true
        })
    }
}
