//! `chromewright` CLI entrypoint: MCP server over stdio or loopback HTTP.
//!
//! Defaults to attach mode against `http://127.0.0.1:9222`; launch flags start a local browser.
//! The `serve` subcommand exposes streamable HTTP for shared local MCP sessions.

use chromewright::{BrowserServer, ConnectionOptions, LaunchOptions};
use clap::{Parser, Subcommand};
use log::{debug, info};
use rmcp::{ServiceExt, transport::stdio};
use std::io::{stdin, stdout};
use std::path::PathBuf;

#[cfg(feature = "mcp-server")]
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};

#[cfg(feature = "tui")]
use chromewright::{
    BrowserSession, BrowserSessionPolicy, ManagedHeadlessSession, TuiOptions, run_tui,
};

/// How the process obtains a browser: local launch or DevTools attach.
#[derive(Debug, Clone)]
enum BrowserMode {
    Launch(LaunchOptions),
    Connect(ConnectionOptions),
}

/// Optional transport subcommand; default (no subcommand) is MCP over stdio.
#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Serve streamable HTTP on loopback for shared local MCP sessions.
    Serve {
        /// Port for HTTP transport (default: 3000)
        #[arg(long, short = 'p', default_value_t = 3000)]
        port: u16,

        /// HTTP streamable endpoint path (default: /mcp)
        #[arg(long, default_value = "/mcp")]
        http_path: String,
    },

    /// Interactive terminal browser over a shared Chrome session (requires `--features tui`).
    #[cfg(feature = "tui")]
    Tui {
        /// TOML keymap overlay path (default: `$XDG_CONFIG_HOME/chromewright/tui.toml`)
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        companion_port: u16,
        #[arg(long, default_value = "/mcp")]
        companion_path: String,
    },
}

/// Top-level CLI: attach/launch browser flags plus optional `serve` or `tui`.
///
/// Without a subcommand the process speaks MCP over stdio. Launch flags start a
/// local browser; otherwise defaults attach to `http://127.0.0.1:9222`.
#[derive(Debug, Parser)]
#[command(name = "chromewright")]
#[command(version)]
#[command(about = "Browser automation MCP server", long_about = None)]
struct Cli {
    /// Launch a new browser in headless mode instead of headed launch mode
    #[arg(long, conflicts_with = "ws_endpoint")]
    headless: bool,

    /// Path to custom browser executable for launch mode
    #[arg(long, value_name = "PATH", conflicts_with = "ws_endpoint")]
    executable_path: Option<PathBuf>,

    /// Browser WebSocket URL or stable DevTools HTTP endpoint for remote browser connection
    /// Defaults to http://127.0.0.1:9222 when no launch-mode flags are provided.
    #[arg(
        long,
        value_name = "URL",
        conflicts_with_all = ["headless", "executable_path", "user_data_dir", "debug_port"]
    )]
    ws_endpoint: Option<String>,

    /// Persistent browser profile directory for launch mode
    #[arg(long, value_name = "DIR", conflicts_with = "ws_endpoint")]
    user_data_dir: Option<PathBuf>,

    /// Explicit DevTools debugging port for locally launched browsers
    #[arg(long, value_name = "PORT", conflicts_with = "ws_endpoint")]
    debug_port: Option<u16>,

    /// Reuse or replace Chromewright's owned `--headless tui` browser.
    /// External `--ws-endpoint` browsers are always attach-only.
    #[cfg(feature = "tui")]
    #[arg(
        long,
        value_enum,
        requires = "headless",
        conflicts_with = "ws_endpoint"
    )]
    browser_session: Option<BrowserSessionPolicy>,

    #[command(subcommand)]
    command: Option<Command>,
}

const DEFAULT_WS_ENDPOINT: &str = "http://127.0.0.1:9222";

fn wants_launch_mode(cli: &Cli) -> bool {
    cli.headless
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
        headless: cli.headless,
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

#[cfg(feature = "tui")]
fn create_browser_session(mode: &BrowserMode) -> Result<BrowserSession, String> {
    match mode {
        BrowserMode::Launch(options) => {
            BrowserSession::launch(options.clone()).map_err(|e| e.to_string())
        }
        BrowserMode::Connect(options) => {
            BrowserSession::connect(options.clone()).map_err(|e| e.to_string())
        }
    }
}

#[cfg(feature = "tui")]
fn managed_headless_tui_session(cli: &Cli) -> Result<ManagedHeadlessSession, String> {
    if cli.user_data_dir.is_some() {
        return Err(
            "--headless tui manages a private runtime profile; --user-data-dir is not supported in this mode"
                .into(),
        );
    }
    ManagedHeadlessSession::open(
        &LaunchOptions {
            headless: true,
            chrome_path: cli.executable_path.clone(),
            debug_port: cli.debug_port,
            ..Default::default()
        },
        cli.browser_session.unwrap_or_default(),
    )
}

#[cfg(feature = "tui")]
fn validate_tui_session_policy(cli: &Cli) -> Result<(), String> {
    if cli.browser_session.is_some() && !matches!(&cli.command, Some(Command::Tui { .. })) {
        return Err("--browser-session is only valid with --headless tui".into());
    }
    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    #[cfg(feature = "tui")]
    validate_tui_session_policy(&cli)?;
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

    match cli.command.clone() {
        None => {
            info!("Transport: stdio");
            info!("Ready to accept MCP connections via stdio");
            let (_read, _write) = (stdin(), stdout());
            let service = create_browser_server(&browser_mode)
                .map_err(|e| format!("Failed to create browser server: {}", e))?;
            let server = service.serve(stdio()).await?;

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
        #[cfg(feature = "tui")]
        Some(Command::Tui {
            config,
            companion_port,
            companion_path,
        }) => {
            info!("Transport: terminal UI");
            if let Some(ref path) = config {
                info!("TUI config: {}", path.display());
            } else {
                info!("TUI config: XDG default (if present)");
            }
            let options = TuiOptions {
                config: config.clone(),
                companion_port,
                companion_path,
            };
            if cli.headless {
                info!(
                    "Headless TUI browser session: {:?}",
                    cli.browser_session.unwrap_or_default()
                );
                let mut managed = managed_headless_tui_session(&cli).map_err(|e| {
                    format!("Failed to create managed headless browser session: {e}")
                })?;
                let session = managed.take_session().map_err(|e| {
                    format!("Failed to transfer managed headless browser session: {e}")
                })?;
                let tui_result =
                    run_tui(session, options).map_err(|e| format!("TUI exited with error: {e}"));
                let shutdown_result = managed
                    .shutdown()
                    .map_err(|e| format!("managed headless browser shutdown failed: {e}"));
                match (tui_result, shutdown_result) {
                    (Ok(()), Ok(())) => {}
                    // Preserve the TUI failure as the primary cause while
                    // still surfacing a cleanup failure to the CLI.
                    (Err(tui_error), Ok(())) => return Err(tui_error.into()),
                    (Ok(()), Err(shutdown_error)) => return Err(shutdown_error.into()),
                    (Err(tui_error), Err(shutdown_error)) => {
                        return Err(format!("{tui_error}; {shutdown_error}").into());
                    }
                }
            } else {
                let session = create_browser_session(&browser_mode)
                    .map_err(|e| format!("Failed to create browser session: {e}"))?;
                run_tui(std::sync::Arc::new(session), options)
                    .map_err(|e| format!("TUI exited with error: {e}"))?;
            }
            return Ok(());
        }
        Some(Command::Serve { port, http_path }) => {
            info!("Transport: HTTP streamable");
            info!("Port: {}", port);
            info!("HTTP path: {}", http_path);

            let bind_addr = format!("127.0.0.1:{}", port);
            let browser_mode = browser_mode.clone();

            let service_factory = move || {
                create_browser_server(&browser_mode)
                    .map_err(|e| std::io::Error::other(e.to_string()))
            };

            let http_service = StreamableHttpService::new(
                service_factory,
                LocalSessionManager::default().into(),
                Default::default(),
            );

            let router = axum::Router::new().nest_service(&http_path, http_service);

            info!(
                "Ready to accept MCP connections at http://{}{}",
                bind_addr, http_path
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
    fn test_cli_defaults_to_stdio_without_subcommand() {
        let cli = Cli::try_parse_from(["chromewright"]).expect("CLI should parse");

        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_serve_subcommand_defaults_to_streamable_http() {
        let cli = Cli::try_parse_from(["chromewright", "serve"]).expect("CLI should parse");

        match cli.command {
            Some(Command::Serve { port, http_path }) => {
                assert_eq!(port, 3000);
                assert_eq!(http_path, "/mcp");
            }
            None => panic!("expected serve subcommand"),
            #[cfg(feature = "tui")]
            Some(Command::Tui { .. }) => panic!("expected serve subcommand"),
        }
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
                assert!(
                    !options.headless,
                    "launch mode should default to headed when no --headless flag is passed"
                );
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
    fn test_headless_flag_without_ws_endpoint_uses_launch_mode() {
        let cli = Cli::try_parse_from(["chromewright", "--headless"]).expect("CLI should parse");

        match browser_mode_from_cli(&cli) {
            BrowserMode::Launch(options) => {
                assert!(options.headless);
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
            "--headless",
        ])
        .expect_err("CLI should reject conflicting browser modes");

        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }

    #[cfg(feature = "tui")]
    #[test]
    fn test_cli_tui_subcommand_parses_config() {
        let cli = Cli::try_parse_from([
            "chromewright",
            "tui",
            "--config",
            "/tmp/chromewright-tui.toml",
        ])
        .expect("CLI should parse tui");

        match cli.command {
            Some(Command::Tui { config, .. }) => {
                assert_eq!(config, Some(PathBuf::from("/tmp/chromewright-tui.toml")));
            }
            other => panic!("expected tui subcommand, got {other:?}"),
        }
    }

    #[cfg(feature = "tui")]
    #[test]
    fn test_headless_tui_defaults_to_managed_reuse_policy() {
        let cli = Cli::try_parse_from(["chromewright", "--headless", "tui"])
            .expect("CLI should parse managed headless TUI");
        assert_eq!(
            cli.browser_session.unwrap_or_default(),
            BrowserSessionPolicy::Reuse
        );
        assert!(matches!(cli.command, Some(Command::Tui { .. })));
    }

    #[cfg(feature = "tui")]
    #[test]
    fn test_browser_session_requires_headless_and_rejects_external_endpoint() {
        let no_headless =
            Cli::try_parse_from(["chromewright", "--browser-session", "restart", "tui"])
                .expect_err("browser session policy is managed-headless-only");
        assert_eq!(no_headless.kind(), ErrorKind::MissingRequiredArgument);

        let external = Cli::try_parse_from([
            "chromewright",
            "--ws-endpoint",
            "http://127.0.0.1:9222",
            "--browser-session",
            "restart",
            "tui",
        ])
        .expect_err("external browsers must remain attach-only");
        assert_eq!(external.kind(), ErrorKind::ArgumentConflict);
    }

    #[cfg(feature = "tui")]
    #[test]
    fn test_browser_session_is_rejected_without_tui_subcommand() {
        let cli = Cli::try_parse_from(["chromewright", "--headless", "--browser-session", "reuse"])
            .expect("clap should preserve the explicit option for runtime validation");
        assert!(cli.browser_session.is_some());
        assert!(validate_tui_session_policy(&cli).is_err());
    }

    #[cfg(feature = "tui")]
    #[test]
    fn test_cli_tui_does_not_alter_serve_defaults() {
        let cli = Cli::try_parse_from(["chromewright", "serve"]).expect("serve");
        match cli.command {
            Some(Command::Serve { port, http_path }) => {
                assert_eq!(port, 3000);
                assert_eq!(http_path, "/mcp");
            }
            _ => panic!("expected serve"),
        }
    }
}
