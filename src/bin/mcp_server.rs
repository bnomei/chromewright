//! Chromewright MCP Server
//!
//! This binary provides a Model Context Protocol (MCP) server for browser automation.
//! It exposes browser automation tools that can be used by AI assistants and other MCP clients.

use chromewright::{BrowserServer, ConnectionOptions, LaunchOptions};
use clap::{Parser, ValueEnum};
use log::{debug, info};
use rmcp::{ServiceExt, transport::stdio};
use std::io::{stdin, stdout};
use std::path::PathBuf;

#[cfg(feature = "mcp-server")]
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Transport {
    /// Standard input/output transport
    Stdio,
    /// HTTP streamable transport (default)
    Http,
}

#[derive(Debug, Clone)]
enum BrowserMode {
    Launch(LaunchOptions),
    Connect(ConnectionOptions),
}

#[derive(Debug, Parser)]
#[command(name = "chromewright")]
#[command(version)]
#[command(about = "Browser automation MCP server", long_about = None)]
struct Cli {
    /// Launch a new browser in headed mode instead of attaching to the default Chrome session
    #[arg(long, short = 'H', conflicts_with = "ws_endpoint")]
    headed: bool,

    /// Path to custom browser executable for launch mode
    #[arg(long, value_name = "PATH", conflicts_with = "ws_endpoint")]
    executable_path: Option<PathBuf>,

    /// Browser WebSocket URL or stable DevTools HTTP endpoint for remote browser connection
    /// Defaults to http://127.0.0.1:9222 when no launch-mode flags are provided.
    #[arg(
        long,
        value_name = "URL",
        conflicts_with_all = ["headed", "executable_path", "user_data_dir", "debug_port"]
    )]
    ws_endpoint: Option<String>,

    /// Persistent browser profile directory for launch mode
    #[arg(long, value_name = "DIR", conflicts_with = "ws_endpoint")]
    user_data_dir: Option<PathBuf>,

    /// Explicit DevTools debugging port for locally launched browsers
    #[arg(long, value_name = "PORT", conflicts_with = "ws_endpoint")]
    debug_port: Option<u16>,

    /// Transport type to use
    #[arg(long, short = 't', value_enum, default_value = "http")]
    transport: Transport,

    /// Port for HTTP transport (default: 3000)
    #[arg(long, short = 'p', default_value = "3000")]
    port: u16,

    /// HTTP streamable endpoint path (default: /mcp)
    #[arg(long, default_value = "/mcp")]
    http_path: String,
}

const DEFAULT_WS_ENDPOINT: &str = "http://127.0.0.1:9222";

fn wants_launch_mode(cli: &Cli) -> bool {
    cli.headed
        || cli.executable_path.is_some()
        || cli.user_data_dir.is_some()
        || cli.debug_port.is_some()
}

fn browser_mode_from_cli(cli: &Cli) -> BrowserMode {
    if let Some(ws_endpoint) = &cli.ws_endpoint {
        return BrowserMode::Connect(ConnectionOptions::new(ws_endpoint.clone()));
    }

    if !wants_launch_mode(cli) {
        return BrowserMode::Connect(ConnectionOptions::new(DEFAULT_WS_ENDPOINT));
    }

    BrowserMode::Launch(LaunchOptions {
        headless: !cli.headed,
        chrome_path: cli.executable_path.clone(),
        user_data_dir: cli.user_data_dir.clone(),
        debug_port: cli.debug_port,
        ..Default::default()
    })
}

fn create_browser_server(mode: &BrowserMode) -> Result<BrowserServer, String> {
    match mode {
        BrowserMode::Launch(options) => BrowserServer::with_options(options.clone()),
        BrowserMode::Connect(options) => BrowserServer::connect(options.clone()),
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let browser_mode = browser_mode_from_cli(&cli);

    info!("chromewright MCP server v{}", env!("CARGO_PKG_VERSION"));
    match &browser_mode {
        BrowserMode::Launch(options) => {
            info!(
                "Browser mode: {}",
                if options.headless {
                    "headless"
                } else {
                    "headed"
                }
            );

            if let Some(ref path) = options.chrome_path {
                info!("Browser executable: {}", path.display());
            }

            if let Some(ref dir) = options.user_data_dir {
                info!("User data directory: {}", dir.display());
            }

            if let Some(port) = options.debug_port {
                info!("DevTools port: {}", port);
            } else {
                info!("DevTools port: auto");
            }
        }
        BrowserMode::Connect(options) => {
            info!("Browser mode: connect");
            info!("Browser endpoint: {}", options.ws_url);
        }
    }

    // Route to appropriate transport
    match cli.transport {
        Transport::Stdio => {
            info!("Transport: stdio");
            info!("Ready to accept MCP connections via stdio");
            let (_read, _write) = (stdin(), stdout());
            let service = create_browser_server(&browser_mode)
                .map_err(|e| format!("Failed to create browser server: {}", e))?;
            let server = service.serve(stdio()).await?;

            // Set up signal handler for graceful shutdown
            #[cfg(unix)]
            {
                let mut sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
                let mut sigint =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

                tokio::select! {
                    quit_reason = server.waiting() => {
                        debug!("Server quit with reason: {:?}", quit_reason);
                    }
                    _ = sigterm.recv() => {
                        info!("Received SIGTERM, shutting down gracefully...");
                    }
                    _ = sigint.recv() => {
                        info!("Received SIGINT (Ctrl+C), shutting down gracefully...");
                    }
                }
            }

            #[cfg(windows)]
            {
                let mut ctrl_c = tokio::signal::windows::ctrl_c()?;
                let mut ctrl_break = tokio::signal::windows::ctrl_break()?;

                tokio::select! {
                    quit_reason = server.waiting() => {
                        debug!("Server quit with reason: {:?}", quit_reason);
                    }
                    _ = ctrl_c.recv() => {
                        info!("Received Ctrl+C, shutting down gracefully...");
                    }
                    _ = ctrl_break.recv() => {
                        info!("Received Ctrl+Break, shutting down gracefully...");
                    }
                }
            }

            #[cfg(not(any(unix, windows)))]
            {
                let quit_reason = server.waiting().await;
                debug!("Server quit with reason: {:?}", quit_reason);
            }
        }
        Transport::Http => {
            info!("Transport: HTTP streamable");
            info!("Port: {}", cli.port);
            info!("HTTP path: {}", cli.http_path);

            let bind_addr = format!("127.0.0.1:{}", cli.port);
            let browser_mode = browser_mode.clone();

            // Create service factory closure
            let service_factory = move || {
                create_browser_server(&browser_mode)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
            };

            let http_service = StreamableHttpService::new(
                service_factory,
                LocalSessionManager::default().into(),
                Default::default(),
            );

            let router = axum::Router::new().nest_service(&cli.http_path, http_service);

            info!(
                "Ready to accept MCP connections at http://{}{}",
                bind_addr, cli.http_path
            );

            let listener = tokio::net::TcpListener::bind(bind_addr).await?;
            axum::serve(listener, router).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn test_cli_defaults_to_streamable_http_transport() {
        let cli = Cli::try_parse_from(["chromewright"]).expect("CLI should parse");

        assert!(matches!(cli.transport, Transport::Http));
    }

    #[test]
    fn test_browser_mode_defaults_to_devtools_http_attach() {
        let cli = Cli::try_parse_from(["chromewright"]).expect("CLI should parse");

        match browser_mode_from_cli(&cli) {
            BrowserMode::Connect(options) => {
                assert_eq!(options.ws_url, DEFAULT_WS_ENDPOINT);
            }
            BrowserMode::Launch(_) => panic!("expected default attach mode"),
        }
    }

    #[test]
    fn test_browser_mode_uses_local_launch_flags() {
        let cli = Cli::try_parse_from([
            "chromewright",
            "--headed",
            "--executable-path",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "--user-data-dir",
            "/tmp/chromewright-profile",
            "--debug-port",
            "9333",
        ])
        .expect("CLI should parse");

        match browser_mode_from_cli(&cli) {
            BrowserMode::Launch(options) => {
                assert!(!options.headless);
                assert_eq!(
                    options.chrome_path,
                    Some(PathBuf::from(
                        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
                    ))
                );
                assert_eq!(
                    options.user_data_dir,
                    Some(PathBuf::from("/tmp/chromewright-profile"))
                );
                assert_eq!(options.debug_port, Some(9333));
            }
            BrowserMode::Connect(_) => panic!("expected local launch mode"),
        }
    }

    #[test]
    fn test_headed_launch_without_ws_endpoint_uses_launch_mode() {
        let cli = Cli::try_parse_from(["chromewright", "--headed"]).expect("CLI should parse");

        match browser_mode_from_cli(&cli) {
            BrowserMode::Launch(options) => {
                assert!(!options.headless);
            }
            BrowserMode::Connect(_) => panic!("expected local launch mode"),
        }
    }

    #[test]
    fn test_browser_mode_can_connect_to_existing_websocket() {
        let cli = Cli::try_parse_from([
            "chromewright",
            "--ws-endpoint",
            "ws://127.0.0.1:9222/devtools/browser/test",
        ])
        .expect("CLI should parse");

        match browser_mode_from_cli(&cli) {
            BrowserMode::Connect(options) => {
                assert_eq!(options.ws_url, "ws://127.0.0.1:9222/devtools/browser/test");
            }
            BrowserMode::Launch(_) => panic!("expected remote connect mode"),
        }
    }

    #[test]
    fn test_browser_mode_can_connect_to_devtools_http_origin() {
        let cli = Cli::try_parse_from(["chromewright", "--ws-endpoint", "http://127.0.0.1:9222"])
            .expect("CLI should parse");

        match browser_mode_from_cli(&cli) {
            BrowserMode::Connect(options) => {
                assert_eq!(options.ws_url, "http://127.0.0.1:9222");
            }
            BrowserMode::Launch(_) => panic!("expected remote connect mode"),
        }
    }

    #[test]
    fn test_ws_endpoint_conflicts_with_local_launch_flags() {
        let err = Cli::try_parse_from([
            "chromewright",
            "--ws-endpoint",
            "ws://127.0.0.1:9222/devtools/browser/test",
            "--headed",
        ])
        .expect_err("CLI should reject conflicting browser modes");

        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }
}
