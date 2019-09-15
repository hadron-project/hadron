//! Data models for Railgun primitive objects.

use std::collections::BTreeSet;

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
    /// This stream's type.
    pub stream_type: StreamType,
    /// The visibility of this stream.
    pub visibility: StreamVisibility,
}

/// The type variants of a stream.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum StreamType {
    /// A standard durable stream.
    Standard,
    /// A durable stream which enfoces that the ID of each entry be unique.
    UniqueId {
        /// The in-memory index of the stream's IDs.
        ///
        /// This is never sent to disk as part of this model.
        #[serde(skip)]
        index: BTreeSet<String>,
    }
}

/// The visibility variants of a stream.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum StreamVisibility {
    /// The stream is visible at the namespace scope.
    Namespace,
    /// The stream is only accessible as part of a pipeline.
    Private(String),
}

/// An entry of data written to a stream.
#[derive(Clone, Serialize, Deserialize)]
pub struct StreamEntry {
    /// The optional ID of the entry.
    pub id: Option<String>,
    /// The data payload of the entry.
    pub data: Vec<u8>,
}
