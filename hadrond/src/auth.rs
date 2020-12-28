#![allow(dead_code)] // TODO: remove this.

use anyhow::{anyhow, ensure, Result};
use serde::{Deserialize, Serialize};
use tonic::metadata::{Ascii, MetadataValue};

use crate::config::Config;
use crate::error::AppError;
use crate::utils;

const BEARER_PREFIX: &str = "bearer ";

//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

/// A token credenitals set, containing the ID of the token and other associated data.
///
/// This is construct by cryptographically verifying a token, and validating its claims.
pub struct TokenCredentials {
    /// The ID of this token.
    pub id: u64,
    /// The original value of auth the header.
    pub auth_header: MetadataValue<Ascii>,
}

impl TokenCredentials {
    /// Extract a token from the given header value bytes.
    pub fn from_auth_header(header: MetadataValue<Ascii>, _config: &Config) -> Result<Self> {
        let header_str = header
            .to_str()
            .map_err(|_| anyhow!("invalid credentials provided, must be a valid string value"))?;
        ensure!(
            header_str.starts_with(BEARER_PREFIX),
            "invalid credentials provided, token credentials header must begin with 'bearer '",
        );
        ensure!(header_str.len() > 7, "invalid credentials provided, no token detected");
        let token = &header_str[8..];
        tracing::trace!(%token, "auth token detected");
        // let claims: Claims = jsonwebtoken::decode(token.as_ref()) // TODO: finish this up and remove stubbed claims below.
        Ok(TokenCredentials { id: 0, auth_header: header })
    }
}

/// A permissions claim of a token.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "v")]
pub enum Claims {
    #[serde(rename = "1")]
    V1(ClaimsV1),
}

impl Claims {
    /// Ensure these claims are sufficient for publishing to the given stream.
    pub fn check_stream_pub_auth(&self, ns: &str, stream: &str) -> Result<()> {
        match &self {
            Self::V1(v) => match v {
                ClaimsV1::All => Ok(()),
                ClaimsV1::Namespaced(grants) => {
                    let is_authorized = grants.iter().any(|grant| match grant {
                        NamespaceGrant::Full { namespace } => namespace == ns,
                        NamespaceGrant::Limited { namespace, .. } if namespace != ns => false,
                        NamespaceGrant::Limited { streams, .. } => match streams {
                            Some(streams) => streams.iter().any(|s| s.matcher.has_match(stream) && s.access.can_publish()),
                            None => false,
                        },
                    });
                    ensure!(is_authorized, AppError::Unauthorized);
                    Ok(())
                }
                ClaimsV1::Metrics => Err(AppError::Unauthorized.into()),
            },
        }
    }

    /// Ensure these claims are sufficient for schema mutations on the target namespace.
    pub fn check_schema_auth(&self, ns: &str) -> Result<()> {
        match &self {
            Self::V1(v) => match v {
                ClaimsV1::All => Ok(()),
                ClaimsV1::Namespaced(grants) => {
                    let is_authorized = grants.iter().any(|grant| match grant {
                        NamespaceGrant::Full { namespace } => namespace == ns,
                        NamespaceGrant::Limited { namespace, .. } if namespace != ns => false,
                        NamespaceGrant::Limited { schema, .. } => *schema,
                    });
                    ensure!(is_authorized, AppError::Unauthorized);
                    Ok(())
                }
                ClaimsV1::Metrics => Err(AppError::Unauthorized.into()),
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClaimsV1 {
    /// A permissions grant on all resources in the system.
    All,
    /// A set of permissions granted on namespace scoped resources.
    Namespaced(Vec<NamespaceGrant>),
    /// A permissions grant on only the cluster metrics system.
    Metrics,
}

/// A permissions grant on a set of resources of a specific namespace.
///
/// Pipeline permissions are evaluated purely in terms of stream permissions. A token may be used
/// as a pipeline stage subscriber if it has sufficient permissions to read from the input streams
/// of the stage and has permissions to write to the output streams of the stage.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NamespaceGrant {
    /// A grant of full permissions on the target namespace.
    Full {
        /// The namespace to which this grant applies.
        namespace: String,
    },
    /// A grant of limited access to specific resources within the target namespace.
    Limited {
        /// The namespace to which this grant applies.
        namespace: String,
        /// Permissions granted on the namespace's ephemeral messaging exchange.
        messaging: Option<Access>,
        /// Permissions granted on RPC endpoints.
        endpoints: Option<Vec<EndpointGrant>>,
        /// Permissions granted on streams.
        streams: Option<Vec<StreamGrant>>,
        /// Permissions to modify the schema of the namespace.
        ///
        /// A token with schema permissions is allowed to create, update & delete streams, pipelines
        /// and other core resources in the associated namespace.
        schema: bool,
    },
}

/// An enumeration of possible access levels.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Access {
    Pub,
    Sub,
    All,
}

impl Access {
    /// Check if this access level represents a sufficient grant to be able to publish.
    pub fn can_publish(&self) -> bool {
        match self {
            Self::All | Self::Pub => true,
            Self::Sub => false,
        }
    }
}

/// A permissions grant on a set of matching endpoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndpointGrant {
    pub matcher: NameMatcher,
    pub access: Access,
}

/// A permissions grant on a set of matching streams.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamGrant {
    pub matcher: NameMatcher,
    pub access: Access,
}

/// A fully qualified matcher over a hierarchical name specification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NameMatcher(pub Vec<NameMatchSegment>);

/// A name match segment variant.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum NameMatchSegment {
    /// A match on a literal segment.
    Literal { literal: String },
    /// A wildcard match on a single segment.
    Wild,
    /// A wildcard match on all remaining segments. Does not match if there are no remaining segments.
    Remaining,
}

impl NameMatcher {
    /// Test the given object name against this matcher instance.
    pub fn has_match(&self, name: &str) -> bool {
        let mut did_hit_remaining_token = false;
        let mut hierarchy = name.split(utils::HIERARCHY_TOKEN);
        let mut hierarchy_len: usize = 0;
        let has_match = hierarchy
            .by_ref()
            .enumerate()
            .try_for_each(|(idx, seg)| -> Result<(), ()> {
                println!("checking segment for matcher: {}", seg);
                hierarchy_len += 1;
                if did_hit_remaining_token {
                    return Ok(());
                }
                let matcher = match self.0.get(idx) {
                    Some(matcher) => matcher,
                    None => return Err(()),
                };
                match matcher {
                    NameMatchSegment::Literal { literal } => {
                        if literal == seg {
                            Ok(())
                        } else {
                            Err(())
                        }
                    }
                    NameMatchSegment::Wild => Ok(()),
                    NameMatchSegment::Remaining => {
                        did_hit_remaining_token = true;
                        Ok(())
                    }
                }
            })
            .is_ok();
        println!("got a match: {}", has_match);
        // If we have a match, but the matcher is more specific & we did not hit the `remaining`
        // token, then this is not a match.
        if has_match && (self.0.len() > hierarchy_len) && !did_hit_remaining_token {
            println!("returning false due to mismatch of matcher len");
            false
        } else {
            // Else, we have a match.
            println!("returning from matcher with has_match");
            has_match
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

/// A system user.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct User {
    /// The user's name.
    pub name: String,
    /// The user's role.
    pub role: UserRole,
}

/// A system user's role.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum UserRole {
    /// Full control over the cluster and all resources.
    ///
    /// There is only ever one root user — called `root` — and its permissions are irrevocable.
    /// It is recommended that the `root` user only be used to create an initial set of admin
    /// users which should be used from that point onward.
    Root,
    /// Full control over the cluster and all resources.
    Admin,
    /// A user with view-only permissions on cluster resources.
    Viewer,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod test {
    use super::*;

    mod name_matcher {
        use super::*;

        macro_rules! test_has_match {
            ({test=>$test:ident, name=>$name:literal, expected=>$expected:literal, matcher=>$matcher:expr}) => {
                #[test]
                fn $test() {
                    let res = $matcher.has_match($name);
                    assert_eq!(res, $expected)
                }
            };
        }

        //////////////////////////////////////////////////////////////////////
        // Tests for Literals ////////////////////////////////////////////////

        test_has_match!({test=>matches_single_lit_single_seg, name=>"service0", expected=>true, matcher=>NameMatcher(vec![
            NameMatchSegment::Literal{literal: String::from("service0")},
        ])});

        test_has_match!({test=>fails_single_lit_single_seg, name=>"service0task1", expected=>false, matcher=>NameMatcher(vec![
            NameMatchSegment::Literal{literal: String::from("service0")},
        ])});

        test_has_match!({test=>fails_single_lit_multi_seg, name=>"service0.task1", expected=>false, matcher=>NameMatcher(vec![
            NameMatchSegment::Literal{literal: String::from("service0")},
        ])});

        test_has_match!({test=>fails_multi_lit_single_seg, name=>"service0", expected=>false, matcher=>NameMatcher(vec![
            NameMatchSegment::Literal{literal: String::from("service0")},
            NameMatchSegment::Literal{literal: String::from("task1")},
        ])});

        //////////////////////////////////////////////////////////////////////
        // Tests for Wildcard Segments ///////////////////////////////////////

        test_has_match!({test=>matches_single_wild_single_seg, name=>"service0", expected=>true, matcher=>NameMatcher(vec![
            NameMatchSegment::Wild,
        ])});

        test_has_match!({test=>matches_wild_lit_multi_seg, name=>"service0.task1", expected=>true, matcher=>NameMatcher(vec![
            NameMatchSegment::Wild, NameMatchSegment::Literal{literal: String::from("task1")},
        ])});

        test_has_match!({test=>matches_lit_wild_lit_multi_seg, name=>"service0.task1.sub2", expected=>true, matcher=>NameMatcher(vec![
            NameMatchSegment::Literal{literal: String::from("service0")}, NameMatchSegment::Wild, NameMatchSegment::Literal{literal: String::from("sub2")},
        ])});

        test_has_match!({test=>fails_lit_wild_lit_multi_seg, name=>"service0.task1.sub2", expected=>false, matcher=>NameMatcher(vec![
            NameMatchSegment::Literal{literal: String::from("service0")}, NameMatchSegment::Wild, NameMatchSegment::Literal{literal: String::from("sub1")},
        ])});

        test_has_match!({test=>fails_single_wild_multi_seg, name=>"service0.task1", expected=>false, matcher=>NameMatcher(vec![
            NameMatchSegment::Wild,
        ])});

        //////////////////////////////////////////////////////////////////////
        // Tests for Remaining Token /////////////////////////////////////////

        test_has_match!({test=>matches_remaining_single_seg, name=>"service0", expected=>true, matcher=>NameMatcher(vec![
            NameMatchSegment::Remaining,
        ])});

        test_has_match!({test=>matches_remaining_multi_seg, name=>"service0.task1.sub2", expected=>true, matcher=>NameMatcher(vec![
            NameMatchSegment::Remaining,
        ])});

        test_has_match!({test=>matches_lit_wild_remaining_multi_seg, name=>"service0.task1.sub2", expected=>true, matcher=>NameMatcher(vec![
            NameMatchSegment::Literal{literal: String::from("service0")}, NameMatchSegment::Wild, NameMatchSegment::Remaining,
        ])});

        test_has_match!({test=>fails_lit_wild_remaining_multi_seg, name=>"service0.task1", expected=>false, matcher=>NameMatcher(vec![
            NameMatchSegment::Literal{literal: String::from("service0")}, NameMatchSegment::Wild, NameMatchSegment::Remaining,
        ])});
    }
}
