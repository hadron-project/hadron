# Exchanges & Endpoints
```
Author: Dodd
Status: InDesign
```

Ephemeral messaging exchanges & RPC endpoints. These are two different features, but with significant overlap.

- The schema for both exchanges and endpoints will declare replica sets on which controllers for these objects will live and run.
- Exchange & RPC endpoint subscribers are entirely ephemeral. Information on subscribers is held in memory only.
- If the leader dies or a subscriber connection dies, that information will be available almost immediately as durable connections are used, and clients will simply reconnect.
- Replica set leaders hosting exchanges and RPC endpoints are responsible for making load balancing decisions, and brokering RPC bidirectional communication.
