use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::transport::io::stdio;
use rmcp::ServiceExt;

mod client;
mod error;
mod server;
mod tools;

pub use client::IloldClient;
pub use error::McpClientError;
pub use server::dispatch;

#[derive(Debug, Clone)]
pub struct Config {
    pub server_url: String,
}

pub async fn run(cfg: Config) -> Result<()> {
    let client = Arc::new(IloldClient::new(cfg.server_url));
    client
        .health_check()
        .await
        .context("ilold backend health-check failed (is `ilold serve` running?)")?;

    let handler = server::EvmMcpServer::new(client);
    let service = handler.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
