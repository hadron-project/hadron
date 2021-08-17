#[allow(clippy::match_single_binding)] // TODO: remove this when routes are added.
pub mod operator;

pub use operator::operator_server::{Operator, OperatorServer};
pub use operator::*;
