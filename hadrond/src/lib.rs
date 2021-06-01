#![allow(unused_imports)] // TODO: remove this.
#![allow(unused_variables)] // TODO: remove this.
#![allow(unused_mut)] // TODO: remove this.
#![allow(dead_code)] // TODO: remove this.

// TODO: finish up changes to helm chart so that we can launch this guy.

mod app;
mod auth;
mod config;
// Exposed for CRD generation.
pub mod crd;
mod database;
mod error;
mod futures;
mod k8s;
mod models;
mod server;
mod utils;

// Public exports for binaries.
pub use crate::{app::AppServer, config::Config};
