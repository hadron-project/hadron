use serde::{Serialize, Deserialize};

use crate::proto::client::api;

const HIERARCHY_TOKEN: &str = ".";
const UNAUTHORIZED_STREAM_ACCESS: &str = "The client token being used does not specify sufficient permissions for the request stream operation.";

//////////////////////////////////////////////////////////////////////////////////////////////////
// JWT Data Models ///////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag="v")]
pub enum Claims {
    V1(ClaimsV1),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimsV1 {
    /// A boolean indicating if the token has full permissions on the cluster.
    pub all: bool,
    /// The set of permissions for this token.
    pub grants: Vec<Grant>,
}

/// A set of permissions granted on a specific namespace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grant {
    /// The namespace to which this grant applies.
    pub namespace: String,
    /// A boolean indicating if this token has permission to create resources in the associated namespace.
    pub can_create: bool,
    /// The token's access level to the namespace's ephemeral messaging.
    pub messaging: MessagingAccess,
    /// The permissions granted on the endpoints of the associated namespace.
    pub endpoints: Vec<EndpointGrant>,
    /// The permissions granted on the streams of the associated namespace.
    pub streams: Vec<StreamGrant>,
}

/// An enumeration of possible ephemeral messaging access levels.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MessagingAccess {
    None,
    Read,
    Write,
    All,
}

/// A permissions grant on a set of matching endpoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndpointGrant {
    pub matcher: NameMatcher,
    pub access: EndpointAccess,
}

/// The access level of an endpoint grant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EndpointAccess {
    Read,
    Write,
    All,
}

/// A permissions grant on a set of matching streams.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamGrant {
    pub matcher: NameMatcher,
    pub access: StreamAccess,
}

/// The access level of an stream grant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StreamAccess {
    Read,
    Write,
    All,
}

/// A fully qualified matcher over a hierarchical name specification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NameMatcher(pub Vec<NameMatchSegment>);

/// A name match segment variant.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag="t")]
pub enum NameMatchSegment {
    /// A match on a literal segment.
    Literal {literal: String},
    /// A wildcard match on a single segment.
    Wild,
    /// A wildcard match on all remaining segments. Does not match if there are no remaining segments.
    Remaining,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// User Data Models //////////////////////////////////////////////////////////////////////////////

/// A user of the Railgun system.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct User {
    /// The user's name.
    pub name: String,
    /// The user's role.
    pub role: UserRole,
}

/// A user's role within Railgun.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum UserRole {
    /// Full control over the Railgun cluster and all resources.
    Root,
    /// Full control over the resources of specific namespaces and access to system metrics.
    Admin {
        /// The namespaces on which this user has authorization.
        namespaces: Vec<String>,
    },
    /// Read-only permissions on resources of specifed namespaces and/or the cluster's metrics.
    Viewer {
        /// A boolean indicating if this user is authorized to view system metrics.
        metrics: bool,
        /// The namespaces on which this user has authorization.
        namespaces: Vec<String>,
    },
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Claims impl ///////////////////////////////////////////////////////////////////////////////////

impl Claims {
    /// Check if the given
    pub fn check_stream_pub_auth(&self, req: &api::PubStreamRequest) -> Result<(), api::ClientError> {
        match &self {
            Claims::V1(v) => {
                if v.all {
                    return Ok(());
                }
                let has_match = v.grants.iter().filter(|e| &e.namespace == &req.namespace).any(|e| e.streams.iter().any(|s| s.matcher.has_match(&req.name)));
                if has_match {
                    Ok(()) // TODO: ensure role is all or write.
                } else {
                    Err(api::ClientError{message: UNAUTHORIZED_STREAM_ACCESS.to_string(), code: api::ErrorCode::Unauthorized as i32})
                }
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// NameMatcher impl //////////////////////////////////////////////////////////////////////////////

impl NameMatcher {
    /// Test the given object name against this matcher instance.
    pub fn has_match(&self, name: &str) -> bool {
        let mut did_hit_remaining_token = false;
        let matcher_len = self.0.len();
        let has_match = name.split(HIERARCHY_TOKEN).enumerate().try_for_each(|(idx, seg)| -> Result<(), ()> {
            if did_hit_remaining_token {
                return Ok(());
            }
            let matcher = match self.0.get(idx) {
                Some(matcher) => matcher,
                None => return Err(()),
            };
            match matcher {
                NameMatchSegment::Literal{literal} => if literal == seg { Ok(()) } else { Err(()) },
                NameMatchSegment::Wild => Ok(()),
                NameMatchSegment::Remaining => {
                    did_hit_remaining_token = true;
                    Ok(())
                },
            }
        })
        .is_ok();
        if has_match && (matcher_len > name.split(HIERARCHY_TOKEN).count()) && !did_hit_remaining_token {
            false
        } else {
            has_match
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Unit Tests ////////////////////////////////////////////////////////////////////////////////////

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
            }
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
