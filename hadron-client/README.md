hadron-client
=============
The Hadron Rust client library.

### protobuf
To generate update protobuf code for this client library, execute the `genproto.rs` example of this crate. Using build.rs would break end users of the client as we do not ship the protobuf code along with the client source.
