# Durable Task Pool
```
Author: Dodd
Status: InDesign
```

A general purpose queue of durable tasks.

- First In, First Out queue (FIFO).
- Unordered, non-batch processing of messages.
- Delays & timers supported.
- Messages are durable on disk until ack'd.
- Perhaps this should just be a new stream type. At a minimum, this would have a lot of overlap with a normal stream.
