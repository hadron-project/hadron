use std::sync::Arc;

use anyhow::Result;

use hadron::{App, Config};

#[tokio::main]
async fn main() -> Result<()> {
    // Setup tracing/logging system.
    tracing_subscriber::fmt::init();

    tracing::info!("booting the hadron collider ... er, app");
    let config = Arc::new(Config::new());
    let app_handle = App::new(config).await?.spawn();
    let _res = app_handle.await; // TODO: handle this.

    Ok(())
}
