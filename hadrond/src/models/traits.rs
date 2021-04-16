//! Model related traits.

use crate::models::schema;

/// A type which is associated with a namespace, and has a name.
pub trait Namespaced {
    /// The namespace of the object.
    fn namespace(&self) -> &str;

    /// The name of the object.
    fn name(&self) -> &str;

    /// The description of the object.
    fn description(&self) -> &str;

    /// The namespaced name of the object, represented as `{namespace}/{name}`.
    fn namespaced_name(&self) -> String {
        format!("{}/{}", self.namespace(), self.name())
    }
}

impl Namespaced for schema::Stream {
    fn namespace(&self) -> &str {
        &self.metadata.namespace
    }
    fn name(&self) -> &str {
        &self.metadata.name
    }
    fn description(&self) -> &str {
        &self.metadata.description
    }
}

impl Namespaced for schema::Pipeline {
    fn namespace(&self) -> &str {
        &self.metadata.namespace
    }
    fn name(&self) -> &str {
        &self.metadata.name
    }
    fn description(&self) -> &str {
        &self.metadata.description
    }
}

impl Namespaced for schema::Endpoint {
    fn namespace(&self) -> &str {
        &self.metadata.namespace
    }
    fn name(&self) -> &str {
        &self.metadata.name
    }
    fn description(&self) -> &str {
        &self.metadata.description
    }
}
