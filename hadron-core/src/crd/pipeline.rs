//! Pipeline CRD.
//!
//! The code here is used to generate the actual CRD used in K8s. See examples/crd.rs.

use std::collections::BTreeSet;

use kube::CustomResource;
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::crd::RequiredMetadata;

pub type Pipeline = PipelineCRD; // Mostly to resolve a Rust Analyzer issue.

lazy_static::lazy_static! {
    /// The regex used to validate RFC 1123 label names going into K8s.
    static ref NAME_1123_LABEL_RE: Regex = Regex::new("^[a-z0-9]([-a-z0-9]*[a-z0-9])?$").expect("error initializing NAME_1123_LABEL_RE regex");
}

/// The annotation which allows for a pipeline to be updated destructively.
const ANNOTATION_ALLOW_DESTRUCTIVE: &str = "hadron.rs/allow-destructive-update";
/// Max length of a RFC 1123 label name allowed.
const NAME_1123_LABEL_LEN: usize = 63;
/// Error message for a RFC 1123 label name.
const NAME_1123_LABEL_MSG: &str =
    "must be a RFC 1123 label consisting of lower case alphanumeric characters or '-', and must start and end with an alphanumeric character";

/// CRD spec for the Pipeline resource.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, CustomResource, JsonSchema)]
#[kube(
    struct = "PipelineCRD",
    status = "PipelineStatus",
    group = "hadron.rs",
    version = "v1beta1",
    kind = "Pipeline",
    namespaced,
    derive = "PartialEq",
    apiextensions = "v1",
    shortname = "pipeline",
    printcolumn = r#"{"name":"Source Stream","type":"string","jsonPath":".spec.sourceStream"}"#,
    printcolumn = r#"{"name":"Triggers","type":"string","jsonPath":".spec.triggers"}"#,
    printcolumn = r#"{"name":"Max Parallel","type":"number","jsonPath":".spec.maxParallel"}"#
)]
pub struct PipelineSpec {
    /// The name of the stream which will trigger this pipeline.
    #[serde(rename = "sourceStream")]
    pub source_stream: String,
    /// Event type matchers which will trigger this pipeline.
    ///
    /// - An empty list will match any event.
    /// - Hierarchies may be matched using the `.` to match different segments.
    /// - Wildcards `*` and `>` may be used. The `*` will match any segment, and `>` will match
    /// one or more following segments. Wildcards will only be treated as wildcards when they
    /// appear as the only character of a segment, else they will be treated as a literal of the
    /// segment.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// The stages of this pipeline.
    pub stages: Vec<PipelineStage>,
    /// The maximum number of pipeline instances which may be executed in parallel per partition.
    #[serde(rename = "maxParallel")]
    pub max_parallel: u32,
    /// The starting point of the source stream from which to begin this pipeline.
    #[serde(rename = "startPoint")]
    pub start_point: PipelineStartPoint,
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

/// The starting point of the source stream from which to begin this pipeline.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct PipelineStartPoint {
    /// The start point location.
    pub location: PipelineStartPointLocation,
    /// The offset of the source stream from which to start this pipeline.
    #[serde(default)]
    pub offset: Option<u64>,
}

/// The starting point of the source stream from which to begin this pipeline.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PipelineStartPointLocation {
    /// The beginning of the source stream.
    Beginning,
    /// The most recent offset of the source stream.
    Latest,
    /// A specific offset of the source stream.
    Offset,
}

/// CRD status object.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct PipelineStatus {}

impl PipelineCRD {
    /// Check if the given event type matches any of this pipeline's triggers.
    ///
    /// Bear in mind that this method does not attempt to validate the given trigger patterns,
    /// as such functionality is upheld by the Validating Admissions Webhook (VAW). That said,
    /// the structure is as followes:
    /// - any number of segments separated by the `.` character to deliniate a segment. The `.` may
    /// not appear at the beginning or end of the pattern.
    /// - segments may not be empty, and may contain any valid alpha-numeric characters, or one of
    /// the wildcards `*` or `>`. Wildcards may only appear as an independent segment, and may
    /// not have any accompanying text.
    /// - the wildcard `>` may only appear as the very last segment of a pattern as it matches
    /// one or more following segments in the target text.
    ///
    /// ## Rules
    /// - An empty pipeline `triggers` value will match any event type.
    /// - An empty event type being matched is still subject to matching rules. An empty `triggers`
    ///   value or a single wildcard will match it.
    /// - For all other cases, each pipeline trigger will attempt to match the given event type.
    ///   Wildcards are used for hierarchical matching.
    pub fn event_type_matches_triggers<T: AsRef<str>>(triggers: &[T], event_type: &str) -> bool {
        // Empty value matches any event type.
        if triggers.is_empty() {
            return true;
        }

        triggers.iter().any(|pattern| {
            // An empty pattern matches any event type, though this is not allowed during VAW validation.
            let pattern = pattern.as_ref();
            if pattern.is_empty() {
                return true;
            }

            // If the event type has more segments than the pattern, and the pattern does not end
            // with `>`, then it is not a match.
            let pat_segs_count = pattern.split('.').count();
            let event_segs_count = event_type.split('.').count();
            if event_segs_count > pat_segs_count && !pattern.ends_with('>') {
                return false;
            }

            // Default to a true match, and negate at the first contradiction.
            // If there are more event segments than pattern segments, the contradiction is handled above.
            let mut event_segs = event_type.split('.');
            for pat in pattern.split('.') {
                let event_seg_opt = event_segs.next();
                match event_seg_opt {
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

    /// Validate this object, ensuring that it conforms to application requirements.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // Validate source stream name. K8s already validates stream names when they are crated,
        // and we don't have any additional validation to add. Skip for now.

        // Validate triggers, and dedup. Given CloudEvents 1.0 spec (https://github.com/cloudevents/spec/blob/v1.0.1/spec.md#type)
        // there is really no invalid trigger that could be presented. The only thing we can really
        // do here is dedup the set of triggers.
        let mut triggers = BTreeSet::new();
        for trigger in self.spec.triggers.iter() {
            if !triggers.insert(trigger) {
                errors.push(format!(
                    "trigger '{}' of pipeline '{}' is a duplicate and must be removed",
                    trigger,
                    self.name()
                ));
            }
        }

        // Validate stages name basics and build graph nodes.
        use petgraph::{graphmap::GraphMap, Directed};
        let mut graph = GraphMap::<_, (), Directed>::new();
        for stage in self.spec.stages.iter() {
            if !NAME_1123_LABEL_RE.is_match(stage.name.as_str()) {
                errors.push(format!(
                    "stage '{}' of pipeline {} {}",
                    stage.name.as_str(),
                    self.name(),
                    NAME_1123_LABEL_MSG
                ));
            }
            if stage.name.len() > NAME_1123_LABEL_LEN {
                errors.push(format!(
                    "stage name '{}' of pipeline {} may not exceed {} characters",
                    stage.name.as_str(),
                    self.name(),
                    NAME_1123_LABEL_LEN,
                ));
            }
            graph.add_node(stage.name.as_str());
        }

        // Validate graph edges & ensure graph is acyclic.
        let mut has_entrypoint = false;
        for stage in self.spec.stages.iter() {
            if stage.dependencies.is_empty() && stage.after.is_empty() {
                has_entrypoint = true;
            }
            for dep in stage.dependencies.iter() {
                if !graph.contains_node(dep.as_str()) {
                    errors.push(format!(
                        "stage '{}' depends upon stage '{}' which does not exist in pipeline {}",
                        stage.name,
                        dep,
                        self.name()
                    ));
                }
                graph.add_edge(dep.as_str(), stage.name.as_str(), ());
            }
            for after in stage.after.iter() {
                if !graph.contains_node(after.as_str()) {
                    errors.push(format!(
                        "stage '{}' must execute after stage '{}' which does not exist in pipeline {}",
                        stage.name,
                        after,
                        self.name()
                    ));
                }
                graph.add_edge(after.as_str(), stage.name.as_str(), ());
            }
        }
        if let Err(cycle_err) = petgraph::algo::toposort(&graph, None) {
            errors.push(format!(
                "stage '{}' of pipeline {} creates a cycle and pipelines must be acyclic",
                cycle_err.node_id(),
                self.name()
            ));
        }
        if !has_entrypoint {
            errors.push(format!(
                "pipeline {} has no entrypoint stage with no `dependencies` and no `after` stages",
                self.name()
            ));
        }

        // Ensure max_parallel is > 0.
        if self.spec.max_parallel == 0 {
            errors.push(format!("max_parallel of pipeline {} must be > 0", self.name()));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate this object, ensuring that it is compatible with the given "old" version of this object.
    pub fn validate_compatibility(&self, old: &Self) -> Result<(), Vec<String>> {
        let mut new_stages = BTreeSet::new();
        for stage in self.spec.stages.iter() {
            new_stages.insert(stage.name.as_str());
        }
        let mut deleted = vec![];
        for stage in old.spec.stages.iter() {
            if !new_stages.contains(stage.name.as_str()) {
                deleted.push(stage.name.as_str());
            }
        }
        let allow_destructive = self
            .metadata
            .annotations
            .as_ref()
            .map(|ann| ann.contains_key(ANNOTATION_ALLOW_DESTRUCTIVE))
            .unwrap_or(false);
        if !deleted.is_empty() && !allow_destructive {
            Err(vec![format!(
                "pipeline {} would incur data loss if stages {:?} are removed, set annotation {} to allow this change",
                self.name(),
                deleted,
                ANNOTATION_ALLOW_DESTRUCTIVE
            )])
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    macro_rules! matcher_test {
        ($name:ident, $triggers:expr, $event:literal, $expect:literal) => {
            #[test]
            fn $name() {
                let output = Pipeline::event_type_matches_triggers::<&str>($triggers, $event);
                assert!(
                    $expect == output,
                    "expected output `{}` did not match actual output `{}`",
                    $expect,
                    output,
                );
            }
        };
    }

    matcher_test!(empty_triggers_match_any_0, &[], "", true);
    matcher_test!(empty_triggers_match_any_1, &[], "domain.event.v1", true);

    matcher_test!(
        no_match_with_event_type_too_long_and_no_matchall,
        &["domain.event"],
        "domain.event.v1",
        false
    );

    matcher_test!(match_with_event_type_too_long_and_matchall_0, &["domain.>"], "domain.event", true);
    matcher_test!(match_with_event_type_too_long_and_matchall_1, &["domain.>"], "domain.event.v1", true);

    matcher_test!(match_with_wildcard_matchone_0, &["*"], "domain", true);
    matcher_test!(match_with_wildcard_matchone_1, &["domain.*"], "domain.event", true);
    matcher_test!(match_with_wildcard_matchone_2, &["domain.*.*"], "domain.event.v1", true);
    matcher_test!(match_with_wildcard_matchone_3, &["*.event.*"], "domain.event.v1", true);
    matcher_test!(match_with_wildcard_matchone_4, &["*.*.v1"], "domain.event.v1", true);

    matcher_test!(no_match_with_wildcard_matchone_and_event_type_too_long, &["*"], "domain.event", false);

    macro_rules! rfc_1123_labe_test {
        ($name:ident, $pat:literal, $expect:literal) => {
            #[test]
            fn $name() {
                let output = NAME_1123_LABEL_RE.is_match($pat);
                assert!(
                    $expect == output,
                    "match for pattern {} expected to be {} but got {}",
                    $pat,
                    $expect,
                    output,
                );
            }
        };
    }

    rfc_1123_labe_test!(basic_match, "my-stage", true);
    rfc_1123_labe_test!(basic_mismatch, "my_stage", false);
}
