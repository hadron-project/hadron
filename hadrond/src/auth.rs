use anyhow::{bail, ensure, Result};
use proto::v1::CreateTokenRequest;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::AppError;
pub use crate::models::auth::{User, UserRole};
use crate::utils;

/// The authorization header basic prefix — for user creds.
const BASIC_PREFIX: &str = "basic ";
/// The authorization header bearer prefix — for token creds.
const BEARER_PREFIX: &str = "bearer ";

pub struct UserCredentials(String);

impl UserCredentials {
    /// Extract a user name & PW hash from the given header.
    pub fn from_auth_header(header: &http::HeaderValue) -> Result<Self> {
        let header_str = header
            .to_str()
            .map_err(|_| AppError::InvalidCredentials("must be a valid string value".into()))?;

        // Split the header on the basic auth prefix & ensure the leading segment is empty.
        let mut splits = header_str.splitn(2, BASIC_PREFIX);
        ensure!(
            splits.next() == Some(""),
            AppError::InvalidCredentials("authorization header value must begin with 'basic '".into()),
        );

        // Decode the credentials value.
        let datab64 = match splits.next() {
            Some(datab64) if !datab64.is_empty() => datab64,
            _ => bail!(AppError::InvalidCredentials("no basic auth credentials detected in header".into())),
        };
        let creds = match base64::decode(&datab64) {
            Ok(creds_bytes) => match String::from_utf8(creds_bytes) {
                Ok(creds) => creds,
                Err(_) => bail!(AppError::InvalidCredentials(
                    "decoded basic auth credentials were not a valid string value".into()
                )),
            },
            Err(_) => bail!(AppError::InvalidCredentials("could not base64 decode basic auth credentials".into())),
        };
        Ok(UserCredentials(creds))
    }

    /// Extract the username of the credentials, else err if they are malformed.
    pub fn username(&self) -> Result<&str> {
        Ok(self
            .0
            .splitn(2, ':')
            .next()
            .ok_or_else(|| AppError::InvalidCredentials("basic auth credentials were malformed, could not extract username".into()))?)
    }

    /// Extract the password of the credentials, else err if they are malformed.
    pub fn password(&self) -> Result<&str> {
        let mut segs = self.0.splitn(2, ':');
        segs.next();
        Ok(segs
            .next()
            .ok_or_else(|| AppError::InvalidCredentials("basic auth credentials were malformed, could not extract password".into()))?)
    }
}

/// A token credenitals set, containing the ID of the token and other associated data.
///
/// This is construct by cryptographically verifying a token, and validating its claims.
#[derive(Clone)]
pub struct TokenCredentials {
    /// The internal contents of the extracted JWT presented in the associated header.
    pub claims: Claims,
    /// The original token header value of these credentials.
    pub header: http::HeaderValue,
}

impl TokenCredentials {
    /// Extract a token from the given header value bytes.
    pub fn from_auth_header(header: http::HeaderValue, _config: &Config) -> Result<Self> {
        let header_str = header
            .to_str()
            .map_err(|_| AppError::InvalidCredentials("must be a valid string value".into()))?;

        // Split the header on the bearer prefix & ensure the leading segment is empty.
        let mut splits = header_str.splitn(2, BEARER_PREFIX);
        ensure!(
            splits.next() == Some(""),
            AppError::InvalidCredentials("authorization header value must begin with 'bearer '".into()),
        );

        // Check the final segment and ensure we have a populated value.
        let token = match splits.next() {
            Some(token) if !token.is_empty() => token.to_string(),
            _ => bail!(AppError::InvalidCredentials("no token detected in header".into())),
        };
        tracing::trace!(%token, "auth token detected");
        // let claims: Claims = jsonwebtoken::decode(token.as_ref()) // TODO: finish this up and remove stubbed claims below.
        let claims = Claims {
            id: uuid::Uuid::from_u128(0),
            claims: ClaimsVersion::V1(ClaimsV1::All {}),
        };
        Ok(TokenCredentials { claims, header })
    }
}

/// The data model of a JWT issued by Hadron.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Claims {
    /// The ID of this token.
    pub id: uuid::Uuid,
    /// The version of the claims being used.
    pub claims: ClaimsVersion,
}

impl Claims {
    /// Ensure these claims are sufficient for publishing to the given stream.
    pub fn check_stream_pub_auth(&self, ns: &str, stream: &str) -> Result<()> {
        match &self.claims {
            ClaimsVersion::V1(v) => match v {
                ClaimsV1::All {} => Ok(()),
                ClaimsV1::Namespaced { grants } => {
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
                ClaimsV1::Metrics {} => Err(AppError::Unauthorized.into()),
            },
        }
    }

    /// Ensure these claims are sufficient for subscribing to the given stream.
    pub fn check_stream_sub_auth(&self, ns: &str, stream: &str) -> Result<()> {
        match &self.claims {
            ClaimsVersion::V1(v) => match v {
                ClaimsV1::All {} => Ok(()),
                ClaimsV1::Namespaced { grants } => {
                    let is_authorized = grants.iter().any(|grant| match grant {
                        NamespaceGrant::Full { namespace } => namespace == ns,
                        NamespaceGrant::Limited { namespace, .. } if namespace != ns => false,
                        NamespaceGrant::Limited { streams, .. } => match streams {
                            Some(streams) => streams.iter().any(|s| s.matcher.has_match(stream) && s.access.can_subscribe()),
                            None => false,
                        },
                    });
                    ensure!(is_authorized, AppError::Unauthorized);
                    Ok(())
                }
                ClaimsV1::Metrics {} => Err(AppError::Unauthorized.into()),
            },
        }
    }

    /// Ensure these claims are sufficient for schema mutations on the target namespace.
    pub fn check_schema_auth(&self, ns: &str) -> Result<()> {
        match &self.claims {
            ClaimsVersion::V1(v) => match v {
                ClaimsV1::All {} => Ok(()),
                ClaimsV1::Namespaced { grants } => {
                    let is_authorized = grants.iter().any(|grant| match grant {
                        NamespaceGrant::Full { namespace } => namespace == ns,
                        NamespaceGrant::Limited { namespace, .. } if namespace != ns => false,
                        NamespaceGrant::Limited { schema, .. } => *schema,
                    });
                    ensure!(is_authorized, AppError::Unauthorized);
                    Ok(())
                }
                ClaimsV1::Metrics {} => Err(AppError::Unauthorized.into()),
            },
        }
    }

    /// Create a new token claims object from the given client request to create a token.
    pub fn from_create_token_request(req: CreateTokenRequest) -> Result<Self> {
        let id = uuid::Uuid::new_v4();
        let claimsv1 = match (req.all, req.metrics) {
            (true, _) => ClaimsV1::All {},
            (false, true) => ClaimsV1::Metrics {},
            _ => ClaimsV1::Namespaced {
                grants: req
                    .namespaced
                    .into_iter()
                    .try_fold(vec![], |mut acc, grant| -> Result<Vec<NamespaceGrant>> {
                        let nsgrant = if grant.all {
                            NamespaceGrant::Full { namespace: grant.namespace }
                        } else {
                            NamespaceGrant::Limited {
                                namespace: grant.namespace,
                                schema: grant.schema,
                                exchanges: grant
                                    .exchanges
                                    .into_iter()
                                    .try_fold(vec![], |mut acc, val| -> Result<Vec<NameMatcherGrant>> {
                                        acc.push(NameMatcherGrant {
                                            access: PubSubAccess::must_from_i32(val.access)?,
                                            matcher: val.matcher.parse()?,
                                        });
                                        Ok(acc)
                                    })
                                    .map(|val| if val.is_empty() { None } else { Some(val) })?,
                                endpoints: grant
                                    .endpoints
                                    .into_iter()
                                    .try_fold(vec![], |mut acc, val| -> Result<Vec<NameMatcherGrant>> {
                                        acc.push(NameMatcherGrant {
                                            access: PubSubAccess::must_from_i32(val.access)?,
                                            matcher: val.matcher.parse()?,
                                        });
                                        Ok(acc)
                                    })
                                    .map(|val| if val.is_empty() { None } else { Some(val) })?,
                                streams: grant
                                    .streams
                                    .into_iter()
                                    .try_fold(vec![], |mut acc, val| -> Result<Vec<NameMatcherGrant>> {
                                        acc.push(NameMatcherGrant {
                                            access: PubSubAccess::must_from_i32(val.access)?,
                                            matcher: val.matcher.parse()?,
                                        });
                                        Ok(acc)
                                    })
                                    .map(|val| if val.is_empty() { None } else { Some(val) })?,
                            }
                        };
                        acc.push(nsgrant);
                        Ok(acc)
                    })?,
            },
        };
        Ok(Claims {
            id,
            claims: ClaimsVersion::V1(claimsv1),
        })
    }
}

/// The claims version being used.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "v")]
pub enum ClaimsVersion {
    #[serde(rename = "1")]
    V1(ClaimsV1),
}

/// The V1 claims model.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClaimsV1 {
    /// A permissions grant on all resources in the system.
    All {},
    /// A set of permissions granted on namespace scoped resources.
    Namespaced { grants: Vec<NamespaceGrant> },
    /// A permissions grant on only the cluster metrics system.
    Metrics {},
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
        /// Permissions granted on ephemeral messaging exchanges.
        exchanges: Option<Vec<NameMatcherGrant>>,
        /// Permissions granted on RPC endpoints.
        endpoints: Option<Vec<NameMatcherGrant>>,
        /// Permissions granted on streams.
        streams: Option<Vec<NameMatcherGrant>>,
        /// Permissions to modify the schema of the namespace.
        ///
        /// A token with schema permissions is allowed to create, update & delete streams, pipelines
        /// and other core resources in the associated namespace.
        schema: bool,
    },
}

/// An enumeration of possible pub/sub access levels.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[repr(i32)]
pub enum PubSubAccess {
    Pub,
    Sub,
    All,
}

impl PubSubAccess {
    /// Check if this access level represents a sufficient grant to be able to publish.
    pub fn can_publish(&self) -> bool {
        match self {
            Self::All | Self::Pub => true,
            Self::Sub => false,
        }
    }

    /// Check if this access level represents a sufficient grant to be able to subscribe.
    pub fn can_subscribe(&self) -> bool {
        match self {
            Self::All | Self::Sub => true,
            Self::Pub => false,
        }
    }

    /// Convert the given i32 into an instance, else 400 error.
    pub fn must_from_i32(val: i32) -> Result<Self> {
        match val {
            0 => Ok(Self::Pub),
            1 => Ok(Self::Sub),
            2 => Ok(Self::All),
            _ => Err(AppError::InvalidInput(format!("unrecognized pub/sub access level given '{}'", val)).into()),
        }
    }
}

/// A permissions grant on a set of matching endpoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NameMatcherGrant {
    pub matcher: NameMatcher,
    pub access: PubSubAccess,
}

/// A fully qualified matcher over a hierarchical name specification.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NameMatcher(pub Vec<NameMatchSegment>);

impl std::str::FromStr for NameMatcher {
    type Err = anyhow::Error;

    fn from_str(val: &str) -> Result<Self> {
        let segments = val.split('.').try_fold(vec![], |mut acc, seg| -> Result<Vec<NameMatchSegment>> {
            acc.push(match seg {
                "*" => NameMatchSegment::Wild,
                ">" => NameMatchSegment::Remaining,
                _ => NameMatchSegment::Literal { literal: seg.to_owned() },
            });
            Ok(acc)
        })?;
        // Validate that there is no intermediate `>` in the matcher, as it must always be at the end.
        if let Some((idx, _)) = segments.iter().enumerate().find(|(_, val)| matches!(val, NameMatchSegment::Remaining)) {
            if idx < segments.len() - 1 {
                return Err(AppError::InvalidInput("the name matcher wildcard `>` may only appear at the end of a matcher string".into()).into());
            }
        }
        Ok(Self(segments))
    }
}

/// A name match segment variant.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum NameMatchSegment {
    /// A match on a literal segment.
    Literal { literal: String },
    /// A wildcard match on a single segment, represented as `*` in a matcher string.
    Wild,
    /// A wildcard match on all remaining segments, represented as `>` in a matcher string.
    ///
    /// Does not match if there are no remaining segments.
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
