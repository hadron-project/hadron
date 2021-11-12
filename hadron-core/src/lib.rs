pub mod auth;
pub mod crd;
pub mod error;
pub mod prom;

pub use error::AppError;

/// Comma-separated list of canonical label selectors which match the
/// Hadron Operator's labelling scheme.
pub const HADRON_OPERATOR_LABEL_SELECTORS: &str = "app=hadron,hadron.rs/controlled-by=hadron-operator";
