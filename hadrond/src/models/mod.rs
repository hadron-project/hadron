//! Data schema models.

#![allow(dead_code)] // TODO: remove this.

pub mod placement;
pub mod schema;
mod schema_impls;
mod traits;

pub mod prelude {
    pub use super::traits::*;
}
