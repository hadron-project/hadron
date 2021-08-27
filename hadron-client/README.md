hadron-client
=============
The Hadron Rust client library.

This library can be used independently in Rust applications, or it can be used via C FFI in any other language in order to provide a uniform base across all supported languages.

### design properties
- easy to build connection.
- client does not fail if cluster is not immediately available.
- subscribers are built by registering a function with a compatible signature.
- publishers are built by simply keeping a reference to the client, and then sending messages as needed.
