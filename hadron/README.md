hadron
======
The Rust Hadron client.

### design properties
- easy to build connection.
- client does not fail if cluster is not immediately available.
- subscribers are built by registering a function with a compatible signature.
- publishers are built by simply keeping a reference to the client, and then sending messages as needed.
