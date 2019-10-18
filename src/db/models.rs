//! Data models for Railgun primitive objects.

use lazy_static::lazy_static;
use regex::Regex;
use serde::{Serialize, Deserialize};
use sled;

lazy_static! {
    /// The regex pattern for stream names.
    pub(super) static ref STREAM_NAME_PATTERN: Regex = Regex::new(r"[-_.a-zA-Z0-9]{1,100}").expect("Expected stream name pattern to compile.");
}

/// A multi-stage data workflow composed of endpoints and streams.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct Pipeline {
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
    // /// A durable stream which enfoces that the ID of each entry be unique.
    // UniqueId {
    //     /// The in-memory index of the stream's IDs.
    //     ///
    //     /// This is never sent to disk as part of this model.
    //     #[serde(skip)]
    //     index: BTreeSet<String>,
    // }
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
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct StreamEntry {
    /// The index of the stream entry.
    pub index: u64,
    /// The data payload of the entry.
    pub data: Vec<u8>,
}

/// A wrapper type used for tracking a stream's last index & DB tree.
pub struct StreamWrapper {
    pub stream: Stream,
    /// The index to use for the next entry to be written to this stream.
    pub next_index: u64,
    pub data: sled::Tree,
    pub meta: sled::Tree,
}
