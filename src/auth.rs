use serde::{Serialize, Deserialize};

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
    pub matcher: String,
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
    pub matcher: String,
    pub access: StreamAccess,
}

/// The access level of an stream grant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StreamAccess {
    Read,
    Write,
    All,
}
