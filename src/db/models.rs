//! Data models for Railgun primitive objects.

use serde::{Serialize, Deserialize};

/// A multi-stage data workflow composed of endpoints and streams.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Pipeline {
    /// The namespace which this object belongs to.
    pub namespace: String,
    /// The name of this object.
    pub name: String,
}

/// An append-only, immutable log of data.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Stream {
    /// The namespace which this object belongs to.
    pub namespace: String,
    /// The name of this object.
    pub name: String,
}
