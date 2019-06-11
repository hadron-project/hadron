consumers
=========
NOTE: this is still in flux. It may be completely different moving forward.

- The consumption process is handled by the writer delegate for the purpose of tracking offsets. The system will use cursors over the stream's keyspace so that batches of messages may be read from disk, but the offsets will only be committed as messages are ack'd by the target consumer.
- The target client/node combination to which a message from the cursor must be routed to is determined by the the router. The message is sent to the router along with metadata on the stream it came from. The router will then make load balancing decisions and other message routing decisions.
- The offsets of consumers is broadcast to all writer nodes responsible for the specific streams replication. Durable consumer offsets are written to disk, while ephemeral consumer offsets are only held in memory.
- Messages on a cursor which are nack'd by a consumer will be held in memory for immediate reference and decision making, this info is broadcast to all writing peers responsible for the target stream. A timeout will be spawned on the writer for when it should be redelivered. A writer which is selected to be a new writer delegate must spawn timeouts for any currently nack'd messages on cursors.
- Consuming from an ID-checked stream behaves the same way as other persistent streams.
