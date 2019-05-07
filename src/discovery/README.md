discovery
=========
The discovery actor is responsible for working with a backend discovery protocol to discover peer nodes.

The responsibility of the discovery actor is as follows:
- Instantiate the configured discovery backend to perform peer discovery.
- Maintain a set of all currently observed peers.
- Publish changes to any registered subscribers.

### backends
Discovery backends are thier own actor types. The responsibility of the discovery backend actors is as follows:
- Use their specific discovery protocols to resolve IP addresses of peer nodes.
- Immediately push discovered IPs to the `Discovery` actor, which keeps track of peer discovery state, diffing discovery changesets and the like.
- Backends should not attempt to keep track of peer IPs, their roles is only to report on the peer IPs which they discover.

#### dns
The DNS discovery backend uses DNS to discover peer nodes.
