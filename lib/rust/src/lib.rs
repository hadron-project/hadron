#![allow(dead_code)] // TODO: remove
#![allow(unused_imports)] // TODO: remove

mod client;
mod common;
mod futures;
mod handler;

pub use crate::client::{Client, ClientCreds, PipelineSubscription, SubscriberConfig, Subscription, SubscriptionStartingPoint};
pub use crate::handler::{PipelineHandler, StreamHandler};
pub use async_trait::async_trait;
pub use proto::v1::{NameMatcher, NamespaceGrant, PipelineSubDelivery, PubSubAccess, StreamSubDelivery};
