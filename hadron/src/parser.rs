#![allow(clippy::upper_case_acronyms)] // EOI from pest.

use anyhow::{bail, ensure, Context, Result};
use pest::iterators::{Pair, Pairs};
use pest::{Parser, Position, RuleType, Token};
use pest_derive::Parser;

use hadron::{NameMatcher, NamespaceGrant, PubSubAccess};

/// A parser for namespaced grants provided on the CLI.
#[derive(Parser)]
#[grammar = "../../parsers/cli-grants.pest"]
pub struct GrantsParser;

impl GrantsParser {
    /// Parse the given slice of strings as namespace grants.
    pub fn parse_grants(grants: &[String]) -> Result<Vec<NamespaceGrant>> {
        grants.iter().try_fold(vec![], |mut acc, grant| {
            acc.push(GrantsParser::parse_grant(grant)?);
            Ok(acc)
        })
    }

    /// Parse the given str as a namespace grant.
    pub fn parse_grant(grant: &str) -> Result<NamespaceGrant> {
        let mut grant = GrantsParser::parse(Rule::grant, grant)
            .context("error parsing grant")?
            .next()
            .context("no grant spec found")?
            .into_inner();

        // Extract the namespace of the grant.
        let namespace = GrantsParser::parse_namespace(&mut grant)?;

        // Extract the grants to be applied for the namespace.
        let ns_grants = grant
            .next()
            .context("ns_grant contents not found in grant string")?
            .into_inner()
            .try_fold(vec![], |mut grants, ns_grant| -> Result<Vec<NsGrant>> {
                grants.push(GrantsParser::parse_ns_grant(ns_grant)?);
                Ok(grants)
            })?;

        // Build the grant object for use with the API.
        let mut final_grant = ns_grants.into_iter().fold(NamespaceGrant::default(), |mut acc, grant| {
            match grant {
                NsGrant::All => acc.all = true,
                NsGrant::Schema => acc.schema = true,
                NsGrant::Endpoint(matcher, access) => acc.endpoints.push(NameMatcher {
                    matcher,
                    access: access as i32,
                }),
                NsGrant::Exchange(matcher, access) => acc.exchanges.push(NameMatcher {
                    matcher,
                    access: access as i32,
                }),
                NsGrant::Stream(matcher, access) => acc.streams.push(NameMatcher {
                    matcher,
                    access: access as i32,
                }),
            }
            acc
        });
        final_grant.namespace = namespace;
        Ok(final_grant)
    }

    /// Extract the name of the namespace which the grant applies to.
    fn parse_namespace(grant: &mut Pairs<Rule>) -> Result<String> {
        let ns_rule = grant.next().context("namespace not found in grant")?;
        ensure!(
            matches!(ns_rule.as_rule(), Rule::namespace_name),
            "unexpected rule type while extracting grant namespace"
        );
        Ok(ns_rule.as_str().to_owned())
    }

    /// Parse a namespace permission grant.
    fn parse_ns_grant(ns_grant: Pair<Rule>) -> Result<NsGrant> {
        match ns_grant.as_rule() {
            Rule::ns_grant_all => Ok(NsGrant::All),
            Rule::ns_grant_schema => Ok(NsGrant::Schema),
            Rule::ns_grant_endpoint => {
                let (matcher, access) = GrantsParser::parse_ns_grant_matcher_and_access(ns_grant.into_inner())?;
                Ok(NsGrant::Endpoint(matcher, access))
            }
            Rule::ns_grant_exchange => {
                let (matcher, access) = GrantsParser::parse_ns_grant_matcher_and_access(ns_grant.into_inner())?;
                Ok(NsGrant::Exchange(matcher, access))
            }
            Rule::ns_grant_stream => {
                let (matcher, access) = GrantsParser::parse_ns_grant_matcher_and_access(ns_grant.into_inner())?;
                Ok(NsGrant::Stream(matcher, access))
            }
            rule => bail!("unexpected rule type found while parsing ns_grant '{:?}'", rule),
        }
    }

    /// Parse the object matcher and access level of an object grant.
    fn parse_ns_grant_matcher_and_access(mut ns_object_grant: Pairs<Rule>) -> Result<(String, PubSubAccess)> {
        let matcher = ns_object_grant
            .next()
            .context("name matcher not found in object grant")?
            .as_str()
            .to_owned();
        let access = ns_object_grant.next().context("pub/sub access level not found in object grant")?.as_str();
        match access {
            "pub;" => Ok((matcher, PubSubAccess::Pub)),
            "sub;" => Ok((matcher, PubSubAccess::Sub)),
            "all;" => Ok((matcher, PubSubAccess::All)),
            bad => bail!("unrecognized pub/sub access level specified: '{}'", bad),
        }
    }
}

enum NsGrant {
    All,
    Schema,
    Endpoint(String, PubSubAccess),
    Exchange(String, PubSubAccess),
    Stream(String, PubSubAccess),
}
