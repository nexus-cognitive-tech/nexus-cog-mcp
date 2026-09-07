//! `nexus-cog-mcp-server` — Model Context Protocol server binary.
//!
//! Runs the [`Server`] from [`nexus_cog_mcp`] over stdio (default)
//! or streamable HTTP. The transport is selected by the
//! `NEXUS_COG_MCP_TRANSPORT` env var (`stdio` | `http`).
//!
//! DB / workspace resolution (used by the shared [`CmdCtx`]):
//!
//! 1. `NEXUS_COG_DB` — absolute or relative path to the SQLite file.
//!    This is what `nexus-cog mcp --db <path>` sets.
//! 2. `NEXUS_COG_MCP_DEFAULT_WORKSPACE` — directory where the DB is
//!    placed at `<workspace>/.nexus-cog/palace.db`.
//! 3. Fallback `/tmp/nexus-cog-mcp/_default`.

#[cfg(feature = "http")]
use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::transport::stdio;
use rmcp::ServiceExt;

use nexus_cog_mcp::Server;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    init_tracing();

    let transport = std::env::var("NEXUS_COG_MCP_TRANSPORT").unwrap_or_else(|_| "stdio".into());

    // Eagerly open the cortex so DB/environment errors are reported
    // at startup instead of inside the first tool call.
    let server = Server::default()
        .initialize()
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to initialize nexus-cog MCP context")?;

    match transport.as_str() {
        "http" => run_http(server).await.context("http transport failed")?,
        _ => run_stdio(server).await.context("stdio transport failed")?,
    }
    Ok(())
}

async fn run_stdio(server: Server) -> Result<()> {
    let _service = server.serve(stdio()).await?;
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(feature = "http")]
async fn run_http(server: Server) -> Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager,
        tower::StreamableHttpService,
        StreamableHttpServerConfig,
    };
    let cfg = StreamableHttpServerConfig::default();
    let session_manager = Arc::new(LocalSessionManager::default());
    let service = StreamableHttpService::new(
        move || Ok::<_, std::io::Error>(server.clone()),
        session_manager,
        cfg,
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, router).await?;
    Ok(())
}

#[cfg(not(feature = "http"))]
async fn run_http(_server: Server) -> Result<()> {
    anyhow::bail!("`http` transport requested but nexus-cog-mcp was built without the `http` feature")
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::builder()
        .with_default_directive(
            tracing_subscriber::filter::LevelFilter::from_level(tracing::Level::WARN).into(),
        )
        .from_env_lossy();
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
