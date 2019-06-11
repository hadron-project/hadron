Networking
==========
- Railgun client to server communication takes place over WebSockets, which allows for multiplexed communication channels by default.
- Clients use a single socket for consumption as well as publication.
- Server to server cluster communication takes place over WebSockets as well.
- Protocol buffers are used for all Railgun communication.
- Nodes within a cluster may forward commands between each other as needed.
