//! Data schema models.

pub mod auth;
pub mod events;
mod proto;
pub mod schema;
mod traits;

pub mod prelude {
    pub use super::traits::*;
}
