//! Stream & pipeline delivery controls.
//!
//! For delivering stream & pipeline messages to clients which have active consumer subscriptions,
//! this actor functions a bit like a request/response client, sending requests to clients
//! to have them process messages as part of their consumer subscriptions, and then upon
//! receiving a response will send the ack payload to the Raft node for durability. Once the ack
//! has been applied to the state machine, it will respond to the client with an AckStreamResponse
//! or AckPipelineResponse as needed.
//!
//! Fundamentally, messages can only be delivered to live clients, so this subsystem does not need
//! to be aware of all consumer subscriptions which have ever been created, it only needs to be
//! aware of live consumers.
//!
//! When the network actor's node becomes the leader, it will take all of the client routing and
//! subscription information that it has, and will start the process of controlling message
//! delivery. It will start off by submitting queries to the storage engine
//!
//! The storage layer will be initialized with an unbounded stream sender of consumer group
//! updates. When the storage engine first initializes, it will emit a payload of all current
//! consumer group data on disk, and then will emit updates on new consumer groups, updates to ack
//! offsets per group, and will receive messages for immediate delivery.
//!
//!
//!
//! stream consumers
//! ================
//!
//! ### inputs
//! - stream of updates from storage layer:
//!     - new consumer groups created or destroyed.
//!     - ack offset updates.
//!     - stream of new messages applied to streams which may need to be delivered to consumers. NOTE: storage layer will make an informed decision on whether to emit messages for potential delivery based on if the stream has consumer groups or not, and will only do so when Raft node is leader.
//!
//! - stream of updates from network actor on connected clients and their subscriptions.
//!     - new client connections with subscriptions and disconnected clients.
//!
//! - may only run on the Raft leader node, so will receive Raft state metrics.
//!     - this will allow it to spin up or down as needd.
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