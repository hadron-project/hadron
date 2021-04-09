//! Data schema models.

use serde::{Deserialize, Serialize};

pub mod placement;
pub mod schema;
mod schema_impls;
mod traits;

pub mod prelude {
    pub use super::traits::*;
}

/// An object which is assigned an ID and stored in the DB along with that ID.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WithId<T> {
    /// The ID of this object.
    pub id: u64,
    /// The wrapped data model.
    #[serde(flatten)]
    pub model: T,
}

impl<T> WithId<T> {
    /// Create a new model instance.
    pub fn new(id: u64, model: T) -> Self {
        Self { id, model }
    }
}
