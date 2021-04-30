//! Data schema models.

pub mod auth;
pub mod events;
pub mod pipeline;
mod proto;
pub mod schema;
pub mod stream;
mod traits;

pub mod prelude {
    pub use super::traits::*;
}
