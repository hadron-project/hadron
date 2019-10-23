use futures::sync::mpsc::UnboundedReceiver;

use crate::{
    producer::Producer,
};

/// A stream of updates from the storage engine.
pub type StorageUpdatesStream = UnboundedReceiver<StorageUpdate>;

/// A producer of updates from the backing storage engine.
///
/// This producer is intended to be agnostic over any backing storage engine. As such, it only
/// uses storage agnostic data types.
pub(super) type StorageProducer = Producer<StorageUpdate>;

#[derive(Clone)]
pub enum StorageUpdate {
    /// An update which is sent only when the storage engine initializes.
    Initial {
        consumers: Vec<()>,
    }
}
