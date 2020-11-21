//! Data models for Railgun primitive objects.

use std::collections::BTreeSet;

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

//////////////////////////////////////////////////////////////////////////////
// StreamSubscription ////////////////////////////////////////////////////////

/// A model representing a consumer group's progress through consuming a target stream.
///
/// - Always durable.
/// - Names can not be changed.
/// - Names are unique per stream, and multiple subscription requests specifying the same stream
///   and name will simply cause the requstor to be a member of the subscription's consumer group.
/// - IDs are globally unique and are assigned by the cluster.
pub struct StreamSubscription {
    /// The ID of this consumer group.
    pub id: u64,
    /// The namespace which this object belongs to.
    pub namespace: String,
    /// The name of the stream which this consumer group applies to.
    pub stream: String,
    /// The name of this consumer group.
    pub name: String,
    /// Maximum number of messages which may be processed in parallel by this consumer group.
    pub max_in_flight: u8,
    /// Messages will be redelivered if not ack'ed within this amount of time, interpreted as seconds.
    pub redelivery_timout: u64,
    /// The starting position of this consumer group when first created.
    pub start_position: StartPosition,
}

/// A subscription's starting position in its target stream when it is first created.
pub enum StartPosition {
    /// A starting point from the beginning of the associated stream.
    Beginning,
    /// A starting point from the most recent message on the associated stream.
    Latest,
    /// A starting point at the specified offset.
    Offset(u64),
}

/// The offset details of subscription.
///
/// ### algorithm
/// When an ack is recieved for a consumer group:
///
/// - if message index is >= next, take a range from next..msgID, and add values to outstanding set.
/// - if message ID is not >= head, pop the value from the outstanding set.
pub struct SubscriptionOffset {
    /// The ID of the associated consumer group.
    pub id: u64,
    /// The next unprocessed index to be processed by this consumer.
    ///
    /// This value is initialized for a consumer group based on the groups desired starting point
    /// in the stream.
    ///
    /// This value will always be 1 greater than the most recent upper-most index successfully
    /// processed. Values from `next..idx` — where `idx` is the index of an ack'd message ID
    /// >= `next` — will be added to `outstanding`. This provides a consistent and always
    /// up-to-date view of messages which need to be processed.
    ///
    /// When the storage engine receives queries to get payloads of messages which need to be
    /// delivered for a consumer group, the engine will always start from the lowest index in the
    /// `outstanding` set, and will go up to the consumer group's `next` + `max_in_flight` value,
    /// filtering out messages which have already been processed.
    ///
    /// For more details, see the docs on stream consumer groups.
    pub next: u64,
    /// The set of outstanding elements with an index less than `head` which still need to be processed.
    pub outstanding: BTreeSet<u64>,
}

// NOTE: reference impl on offset algorithm tracking for consumers.
// use std::collections::BTreeSet;

// fn main() {
//     let mut head = 4u64;
//     let mut outstanding = BTreeSet::new();
//     outstanding.insert(0u64);
//     outstanding.insert(1u64);
//     outstanding.insert(2u64);
//     outstanding.insert(3u64);

//     // process_ack(10, &mut head, &mut outstanding); // head=10, outstanding={0..=9}
//     // process_ack(2, &mut head, &mut outstanding); // head=4, outstanding={0,1,3}
//     process_ack(5, &mut head, &mut outstanding);

//     println!("New head: {}; new oustanding: {:?}", head, outstanding);
// }

// fn process_ack(index: u64, head: &mut u64, outstanding: &mut BTreeSet<u64>) {
//     if index > *head {
//         ((*head+1)..index).for_each(|val| {
//             outstanding.insert(val);
//         });
//         *head = index;
//     } else {
//         outstanding.remove(&index);
//     }
// }
