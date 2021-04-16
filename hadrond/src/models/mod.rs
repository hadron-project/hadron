//! Data schema models.

use serde::{Deserialize, Serialize};

pub mod events;
pub mod placement;
mod proto;
pub mod schema;
mod traits;

pub mod prelude {
    pub use super::traits::*;
}
