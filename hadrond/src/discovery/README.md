discovery
=========
The discovery actor is responsible for working with a backend discovery protocol to discover peer nodes.

The responsibility of the discovery actor is as follows:
- instantiate the configured discovery backend to perform peer discovery
- maintain a set of all currently observed peers
- publish changes to any registered subscribers

### backends
Discovery backends are thier own types. The responsibilities of discovery backends are as follows:
- use their specific discovery protocols to resolve IP addresses of peer nodes
- immediately push discovered IPs to the `Discovery` actor, which keeps track of peer discovery state, diffing discovery changesets and the like.
- backends should not attempt to keep track of peer IPs, their roles is only to report on the peer IPs which they discover

#### dns
The DNS discovery backend uses DNS SRV to discover peer nodes.
