//! An actor used for delivering stream messages to consumers.
//!
//! The producer runs only on the Raft leader node. In the future, this may change and producers
//! may be selected to run on different nodes by the leader.
//!
//! This system implements the "at least once" delivery semantics for messages. There are some
//! conditions where messages may be delivered more than once, such as when a leadership change
//! takes place while a message has already been delivered, and the new leader will see the message
//! as outstanding and will redeliver it. Stream consumers should always be idempotent.
//!
//! This actor will coalesce a few streams of data from the storage and network layers and will
//! use that data to determine when messages need to be delivered, and when they need to be
//! redelivered. Nacks will cause messages to be redelivered, and a client with unacknowledged
//! messages which disconnects will also have its messages redelivered. There is no redelivery
//! timeout in the Railgun system.
//!
//! When the network actor's node becomes the leader, it will take all of the client routing and
//! subscription information that it has, and will start the process of controlling message
//! delivery. It will start off with queries to the storage engine for needed payloads of data.
//!
//! stream consumers
//! ================
//!
//! ### inputs
//! - stream of updates from storage layer:
//!     - new consumer groups created or destroyed.
//!     - ack offset updates.
//!     - stream of new messages applied to streams which may need to be delivered to consumers.
//!       NOTE: storage layer will make an informed decision on whether to emit messages for potential
//!       delivery based on if the stream has consumer groups or not, and will only do so when Raft
//!       node is leader.
//!
//! - stream of updates from network actor on:
//!     - new stream subscriptions, new pipeline subscriptions and disconnected clients.
//!     - may only run while node is leader, needs updates from Raft on leader ID. To ensure that
//!       an old leader node's delivery actor isn't attmepting to continue delivering messages,
//!       each network actor in the cluster will check a delivery frame's current term and sender
//!       to ensure only the latest leader is delivering.
//!
//! ### leader / term checking on delivery frames
//! As delivery frames are received by a node's network actor, it will check the frame's current
//! term & leader. If the values match, then allowed. If the frame's term is > node's term, allow.
//! All other conditions, reject. The error used to reject delivery frames will not cause
//! redelivery count increments.
//!
//! ### behaviors
//! - must only run on the leader node.
//! - must keep track of live clients participating in consumer groups.
//! - must deliver messages in a load balanced fashion to members of consumer groups.
//! - must be able to enforce redelivery timeouts per delivery.
//! - must be able to enforce max in flight messages per consumer group.
//!
//! ### needs
//! - needs stream of updates from storage layer.
//! - needs stream of updates from network layer.
//! - needs recipient channnel to be able to query for entries of a stream.
//! - needs recipient channel to be able to send DeliverStreamMsg to clients.
//! - needs recipient channel to be able to send DeliverPipelineMsg to clients.
//! - needs recipient channel to be able to send ClientPayloads to Raft (for Ack of stream & pipeline deliveries).
//! - needs recipient channel to be able to send AckStreamMsgResponse to clients.
//! - needs recipient channel to be able to send AckPipelineMsgResponse to clients.
//!
//! ### algorithm notes
//! #### line rate consumers
//! - when a consumer group's offset is within the MaxInFlight range from the last message index of the corresponding stream, the consumer will go into line rate (it will start off at line rate when the consumer is created with the `latest` starting point, but may not necessarily stay there if it falls behind).
//! - when a consumer is at line rate, new messages streamed in from the storage layer will be immediately delivered to available clients.
//! - outstanding deliveries are always tracked.
//! - when a message can not be immediately delivered, the message will be buffered per group. The buffer's capacity matches the consumer group's `MaxInFlight` messages setting.
//! - as soon as a message is received which would overflow the buffer, that message will be dropped, and the consumer will go into a lagging state.
//!
//! #### lagging consumers
//! - when clients become available to start consuming from a lagging consumer, messages will be delivered from the consumer groups buffer, if available, or a new query will be submitted to the storage layer to fetch more entries (if entries are available on the stream, which can be checked in this actor based on cached data from the storage layer's update stream).
//! - the buffer is used to guard against excessive querying.
//! - once the consumer group is back within MaxInFlight range of the stream's greatest index, the consumer group will go back into line rate, at which point it will be able to start delivering messages directly from the storage layer's update stream (or buffer them as needed).
//!
//! NOTE: for atomic ack+pub handling, ack's will need to return a response so that published index data can be used by clients if needed.
//!
//! NOTE: need to get Nack in place as well. Will probably use a new (Stream/Pipeline)DeliveryResponse type which is oneof Ack or Nack.
//!

use crate::{
    NodeId,
    db::{Pipeline},
};

/// An actor responsible for delivering stream messages to consumers.
pub(crate) struct Delivery {
    /// The ID of the node this actor is running on.
    node_id: NodeId,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// NetworkUpdate /////////////////////////////////////////////////////////////////////////////////

pub(crate) enum NetworkUpdate {
    /// An update to the known value of the Raft leader.
    Leader(Option<NodeId>),
    /// An update indicating a disconnected client connection.
    DisconnectedClient {
        connection_id: String,
    },
    /// An update indicating a new stream subscription.
    SubStream {
        /// The ID of the client's connection.
        connection_id: String,
        /// The namespace which this subscription pertains to.
        namespace: String,
        /// The name of the stream which the subscription pertains to.
        stream: String,
        /// The name of the consumer group which the subscription pertains to.
        consumer_group: String,
    },
    /// An update indicating a new pipeline subscription.
    SubPipeline {
        /// The ID of the client's connection.
        connection_id: String,
        /// The namespace which this subscription pertains to.
        namespace: String,
        /// The name of the pieline which the subscription pertains to.
        pipeline: String,
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// StorageUpdate /////////////////////////////////////////////////////////////////////////////////

pub(crate) enum StorageUpdate {
    Initial {
        pipelines: Vec<Pipeline>,
        stream_consumer_groups: Vec<()>,
    }
}
