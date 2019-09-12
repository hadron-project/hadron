Building A Client
=================
TODO: in progress

- clients are responsible for driving the heartbeat system via WebSocket ping/pong frames.
- Server will simply ensure that too much time doesn't pass before receiving another heartbeat. If too much time passes — based on server config & client connection overwrites — the client will be reckoned as dead.
- Client will be configured with a heartbeat rate and number of allowed consecutive missed heartbeat responses. Once that threshold is violated, the client must kick into the reconnect protocol.
