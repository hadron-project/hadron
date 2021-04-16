/// A system user.
#[derive(::serde::Serialize, ::serde::Deserialize)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct User {
    /// The user's name.
    #[prost(string, required, tag="1")]
    pub name: ::prost::alloc::string::String,
    /// The user's role.
    #[prost(enumeration="UserRole", required, tag="2")]
    pub role: i32,
    /// A bcrypt hash of the user's password.
    #[prost(string, required, tag="3")]
    pub pwhash: ::prost::alloc::string::String,
}
/// A system user's role.
#[derive(::serde::Serialize, ::serde::Deserialize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum UserRole {
    /// Full control over the cluster and all resources.
    ///
    /// There is only ever one root user — called `root` — and its permissions are irrevocable.
    /// It is recommended that the `root` user only be used to create an initial set of admin
    /// users which should be used from that point onward.
    Root = 0,
    /// Full control over the cluster and all resources.
    Admin = 1,
    /// A user with view-only permissions on cluster resources.
    Viewer = 2,
}
