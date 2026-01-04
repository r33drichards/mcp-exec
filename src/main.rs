use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use rmcp::{
    transport::{
        stdio,
        streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
    },
    ServiceExt,
};
use tokio_util::sync::CancellationToken;

mod executor;
mod log_store;
mod service;
mod types;

use log_store::{file::FileLogStore, memory::InMemoryLogStore, AnyLogStore};
use service::ExecService;

#[derive(Parser, Debug)]
#[command(author, version, about = "MCP server for shell command execution")]
struct Cli {
    /// Directory path for log storage. If not specified, uses in-memory storage.
    #[arg(long)]
    directory_path: Option<String>,

    /// HTTP port to listen on using streamable HTTP transport (if not specified, uses stdio transport)
    #[arg(long)]
    http_port: Option<u16>,

    /// Address to bind the HTTP server to (default: 127.0.0.1)
    #[arg(long, default_value = "127.0.0.1")]
    bind_address: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cli = Cli::parse();
    tracing::info!(?cli, "Starting MCP exec server");

    let store = match cli.directory_path {
        Some(dir) => {
            tracing::info!("Using file-based log storage at {}", dir);
            AnyLogStore::File(FileLogStore::new(dir))
        }
        None => {
            tracing::info!("Using in-memory log storage");
            AnyLogStore::Memory(InMemoryLogStore::new())
        }
    };

    if let Some(port) = cli.http_port {
        tracing::info!("Starting streamable HTTP transport on {}:{}", cli.bind_address, port);
        start_http_server(store, &cli.bind_address, port).await?;
    } else {
        tracing::info!("Starting stdio transport");
        let service = ExecService::new(store).serve(stdio()).await?;
        service.waiting().await?;
    }

    Ok(())
}

async fn start_http_server(store: AnyLogStore, bind_address: &str, port: u16) -> Result<()> {
    let addr = format!("{}:{}", bind_address, port);
    let ct = CancellationToken::new();

    let config = StreamableHttpServerConfig {
        sse_keep_alive: Some(std::time::Duration::from_secs(15)),
        stateful_mode: true,
        cancellation_token: ct.clone(),
    };

    let session_manager = Arc::new(LocalSessionManager::default());

    let http_service = StreamableHttpService::new(
        move || Ok(ExecService::new(store.clone())),
        session_manager,
        config,
    );

    let app = axum::Router::new().fallback_service(http_service);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Streamable HTTP server listening on {}", addr);

    let ct_shutdown = ct.child_token();
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        ct_shutdown.cancelled().await;
        tracing::info!("HTTP server shutting down");
    });

    // Handle Ctrl+C for graceful shutdown
    let server_handle = tokio::spawn(async move {
        if let Err(e) = server.await {
            tracing::error!("HTTP server error: {:?}", e);
        }
    });

    tokio::signal::ctrl_c().await?;
    tracing::info!("Received Ctrl+C, shutting down");
    ct.cancel();

    // Wait for server to finish
    let _ = server_handle.await;

    Ok(())
}
