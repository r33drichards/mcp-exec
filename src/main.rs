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

    /// Refuse HTTP requests that come from a web browser: any request carrying an
    /// `Origin` or `Sec-Fetch-*` header is answered 403. Use it when a browser runs
    /// on the same host (or in the same network namespace), so that no web page
    /// open there can reach this server.
    #[arg(long)]
    reject_browser_requests: bool,
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
        start_http_server(store, &cli.bind_address, port, cli.reject_browser_requests).await?;
    } else {
        tracing::info!("Starting stdio transport");
        let service = ExecService::new(store).serve(stdio()).await?;
        service.waiting().await?;
    }

    Ok(())
}

/// The name of the first header that marks a request as made by a web browser.
///
/// Browsers attach `Origin` to cross-origin and to all non-GET/HEAD requests, and
/// `Sec-Fetch-*` (fetch metadata) to every request; page script can neither remove
/// nor forge these. MCP clients send none of them.
fn browser_header(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .keys()
        .map(|name| name.as_str())
        .find(|name| *name == "origin" || name.starts_with("sec-fetch-"))
}

async fn reject_browser_requests(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if let Some(header) = browser_header(request.headers()) {
        tracing::warn!(header, "rejected a request made by a web browser");
        return (
            axum::http::StatusCode::FORBIDDEN,
            "Forbidden: requests from web browsers are not accepted",
        )
            .into_response();
    }
    next.run(request).await
}

/// The HTTP application: the MCP streamable HTTP service, optionally behind the
/// browser-request filter.
fn build_app(
    store: AnyLogStore,
    bind_address: &str,
    reject_browsers: bool,
    ct: CancellationToken,
) -> axum::Router {
    let mut config = StreamableHttpServerConfig::default();
    config.sse_keep_alive = Some(std::time::Duration::from_secs(15));
    config.stateful_mode = true;
    config.cancellation_token = ct;

    // rmcp 1.7 enables loopback-only Host validation (DNS-rebinding protection) by
    // default. When the operator binds to a non-loopback address they intend to serve
    // clients that reach the server by hostnames we can't enumerate, so relax the check
    // there; keep the protection for the default loopback bind.
    let bind_is_loopback = bind_address
        .parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false);
    let config = if bind_is_loopback {
        config
    } else {
        tracing::warn!(
            "Binding to non-loopback address {}; disabling Host validation (DNS-rebinding protection)",
            bind_address
        );
        config.disable_allowed_hosts()
    };

    let session_manager = Arc::new(LocalSessionManager::default());

    let http_service = StreamableHttpService::new(
        move || Ok(ExecService::new(store.clone())),
        session_manager,
        config,
    );

    let app = axum::Router::new().fallback_service(http_service);
    if reject_browsers {
        app.layer(axum::middleware::from_fn(reject_browser_requests))
    } else {
        app
    }
}

async fn start_http_server(
    store: AnyLogStore,
    bind_address: &str,
    port: u16,
    reject_browsers: bool,
) -> Result<()> {
    let addr = format!("{}:{}", bind_address, port);
    let ct = CancellationToken::new();

    if reject_browsers {
        tracing::info!("Rejecting requests from web browsers (Origin / Sec-Fetch-* headers)");
    }
    let app = build_app(store, bind_address, reject_browsers, ct.clone());

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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;

    /// Serve the app on an ephemeral loopback port.
    async fn serve(reject_browsers: bool) -> (std::net::SocketAddr, CancellationToken) {
        let ct = CancellationToken::new();
        let store = AnyLogStore::Memory(InMemoryLogStore::new());
        let app = build_app(store, "127.0.0.1", reject_browsers, ct.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (addr, ct)
    }

    /// POST an `initialize` to /mcp with extra header lines; returns the status code.
    async fn post_initialize(addr: std::net::SocketAddr, extra_headers: &str) -> u16 {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\nContent-Length: {}\r\n{}\r\n{}",
            addr.port(),
            INITIALIZE.len(),
            extra_headers,
            INITIALIZE
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        // The status line is all we need; an SSE response body never ends.
        let mut head = Vec::new();
        let mut buf = [0u8; 256];
        while !head.windows(2).any(|w| w == b"\r\n") {
            let n = stream.read(&mut buf).await.unwrap();
            assert!(n > 0, "connection closed before a status line");
            head.extend_from_slice(&buf[..n]);
        }
        let line = String::from_utf8_lossy(&head);
        line.split_whitespace().nth(1).unwrap().parse().unwrap()
    }

    #[test]
    fn browser_headers_are_recognised() {
        let headers = |pairs: &[(&'static str, &'static str)]| {
            let mut map = axum::http::HeaderMap::new();
            for (name, value) in pairs {
                map.insert(*name, value.parse().unwrap());
            }
            map
        };
        assert_eq!(browser_header(&headers(&[("content-type", "application/json")])), None);
        assert_eq!(browser_header(&headers(&[("origin", "https://example.com")])), Some("origin"));
        assert_eq!(browser_header(&headers(&[("origin", "null")])), Some("origin"));
        assert_eq!(browser_header(&headers(&[("sec-fetch-site", "cross-site")])), Some("sec-fetch-site"));
        assert_eq!(browser_header(&headers(&[("sec-fetch-mode", "no-cors")])), Some("sec-fetch-mode"));
        // Not fetch metadata: client hints and the WebSocket headers.
        assert_eq!(browser_header(&headers(&[("sec-ch-ua", "x"), ("sec-websocket-key", "x")])), None);
    }

    #[tokio::test]
    async fn browser_requests_are_refused_when_asked() {
        let (addr, ct) = serve(true).await;
        assert_eq!(post_initialize(addr, "").await, 200);
        assert_eq!(post_initialize(addr, "Origin: https://example.com\r\n").await, 403);
        assert_eq!(post_initialize(addr, "Origin: http://127.0.0.1\r\n").await, 403);
        assert_eq!(post_initialize(addr, "Origin: null\r\n").await, 403);
        assert_eq!(post_initialize(addr, "Sec-Fetch-Site: same-origin\r\n").await, 403);
        assert_eq!(post_initialize(addr, "Sec-Fetch-Mode: cors\r\nSec-Fetch-Dest: empty\r\n").await, 403);
        ct.cancel();
    }

    #[tokio::test]
    async fn browser_requests_pass_by_default() {
        let (addr, ct) = serve(false).await;
        assert_eq!(post_initialize(addr, "").await, 200);
        assert_eq!(post_initialize(addr, "Origin: https://example.com\r\n").await, 200);
        ct.cancel();
    }
}
