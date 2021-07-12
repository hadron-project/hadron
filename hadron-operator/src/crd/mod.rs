//! Hadron CRDs.
//!
//! References:
//! - https://kubernetes.io/docs/tasks/extend-kubernetes/custom-resources/custom-resource-definitions/
//! - https://kubernetes.io/docs/tasks/extend-kubernetes/custom-resources/custom-resource-definitions/#additional-printer-columns
//! - https://kubernetes.io/docs/reference/kubectl/jsonpath/

pub mod pipeline;
pub mod stream;
pub mod token;

use kube::Resource;

pub use pipeline::*;
pub use stream::*;
pub use token::*;

/// A convenience trait built around the fact that all implementors
/// must have the following attributes.
pub trait RequiredMetadata {
    /// The namespace of this object.
    fn namespace(&self) -> &str;

    /// The name of this object.
    fn name(&self) -> &str;
}

impl RequiredMetadata for Pipeline {
    fn namespace(&self) -> &str {
        self.meta().namespace.as_deref().unwrap_or_default()
    }

    fn name(&self) -> &str {
        self.meta().name.as_deref().unwrap_or_default()
    }
}

impl RequiredMetadata for Stream {
    fn namespace(&self) -> &str {
        self.meta().namespace.as_deref().unwrap_or_default()
    }

    fn name(&self) -> &str {
        self.meta().name.as_deref().unwrap_or_default()
    }
}

impl RequiredMetadata for Token {
    fn namespace(&self) -> &str {
        self.meta().namespace.as_deref().unwrap_or_default()
    }

    fn name(&self) -> &str {
        self.meta().name.as_deref().unwrap_or_default()
    }
}
