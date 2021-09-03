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
    /// one or more following segments.
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
}
