use crate::error::{BrowserError, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

const DEVTOOLS_VERSION_PATH: &str = "/json/version";
const DEVTOOLS_VERSION_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const CHROME_BROWSER_IDLE_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Options for launching a new browser instance
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Whether to run browser in headless mode (default: true)
    pub headless: bool,

    /// Custom Chrome/Chromium binary path
    pub chrome_path: Option<PathBuf>,

    /// Browser window width (default: 1280)
    pub window_width: u32,

    /// Browser window height (default: 720)
    pub window_height: u32,

    /// User data directory for browser profile
    pub user_data_dir: Option<PathBuf>,

    /// DevTools debugging port for the launched browser.
    /// If not provided, the library chooses an available local port.
    pub debug_port: Option<u16>,

    /// Enable sandbox mode (default: true)
    pub sandbox: bool,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            headless: true,
            chrome_path: None,
            window_width: 1280,
            window_height: 720,
            user_data_dir: None,
            debug_port: None,
            sandbox: true,
        }
    }
}

impl LaunchOptions {
    /// Create new LaunchOptions with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder method: set headless mode
    pub fn headless(mut self, headless: bool) -> Self {
        self.headless = headless;
        self
    }

    /// Builder method: set Chrome binary path
    pub fn chrome_path(mut self, path: PathBuf) -> Self {
        self.chrome_path = Some(path);
        self
    }

    /// Builder method: set window dimensions
    pub fn window_size(mut self, width: u32, height: u32) -> Self {
        self.window_width = width;
        self.window_height = height;
        self
    }

    /// Builder method: set user data directory
    pub fn user_data_dir(mut self, dir: PathBuf) -> Self {
        self.user_data_dir = Some(dir);
        self
    }

    /// Builder method: set DevTools debugging port
    pub fn debug_port(mut self, port: u16) -> Self {
        self.debug_port = Some(port);
        self
    }

    /// Builder method: enable/disable sandbox
    pub fn sandbox(mut self, sandbox: bool) -> Self {
        self.sandbox = sandbox;
        self
    }
}

/// Options for connecting to an existing browser instance
#[derive(Debug, Clone)]
pub struct ConnectionOptions {
    /// Chrome DevTools browser WebSocket URL, or a stable DevTools HTTP endpoint
    /// such as `http://127.0.0.1:9222`.
    pub ws_url: String,
}

impl ConnectionOptions {
    /// Create new ConnectionOptions with a Chrome browser WebSocket URL or a
    /// stable DevTools HTTP endpoint.
    pub fn new<S: Into<String>>(ws_url: S) -> Self {
        Self {
            ws_url: ws_url.into(),
        }
    }

    /// Resolve the configured endpoint into the browser-scoped DevTools WebSocket URL
    /// expected by `headless_chrome`.
    pub fn resolved_ws_url(&self) -> Result<String> {
        resolve_browser_ws_url(&self.ws_url)
    }
}

#[derive(Debug, Deserialize)]
struct DevToolsVersionResponse {
    #[serde(rename = "webSocketDebuggerUrl")]
    web_socket_debugger_url: String,
}

fn resolve_browser_ws_url(endpoint: &str) -> Result<String> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return Err(BrowserError::ConnectionFailed(
            "Chrome DevTools endpoint cannot be empty".to_string(),
        ));
    }

    if is_browser_ws_url(trimmed) {
        return Ok(trimmed.to_string());
    }

    let version_url = devtools_version_url(trimmed)?;
    fetch_browser_ws_url(&version_url)
}

fn is_browser_ws_url(endpoint: &str) -> bool {
    endpoint.starts_with("ws://") || endpoint.starts_with("wss://")
}

fn devtools_version_url(endpoint: &str) -> Result<String> {
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        let endpoint = endpoint.trim_end_matches('/');
        if endpoint.ends_with(DEVTOOLS_VERSION_PATH) {
            return Ok(endpoint.to_string());
        }
        return Ok(format!("{endpoint}{DEVTOOLS_VERSION_PATH}"));
    }

    Err(BrowserError::ConnectionFailed(format!(
        "Unsupported browser endpoint '{endpoint}'. Use a browser websocket URL or an http(s) DevTools address such as http://127.0.0.1:9222"
    )))
}

fn fetch_browser_ws_url(version_url: &str) -> Result<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(DEVTOOLS_VERSION_TIMEOUT))
        .build()
        .into();

    let mut response = agent.get(version_url).call().map_err(|err| {
        BrowserError::ConnectionFailed(format!(
            "Failed to resolve Chrome browser websocket from {version_url}: {err}"
        ))
    })?;

    let body = response.body_mut().read_to_string().map_err(|err| {
        BrowserError::ConnectionFailed(format!(
            "Failed to read Chrome DevTools metadata from {version_url}: {err}"
        ))
    })?;

    parse_browser_ws_url_from_version_body(version_url, &body)
}

fn parse_browser_ws_url_from_version_body(version_url: &str, body: &str) -> Result<String> {
    let payload: DevToolsVersionResponse = serde_json::from_str(body).map_err(|err| {
        BrowserError::ConnectionFailed(format!(
            "Failed to parse Chrome DevTools metadata from {version_url}: {err}"
        ))
    })?;

    let ws_url = payload.web_socket_debugger_url.trim();
    if ws_url.is_empty() {
        return Err(BrowserError::ConnectionFailed(format!(
            "Chrome DevTools metadata at {version_url} did not include webSocketDebuggerUrl"
        )));
    }

    Ok(ws_url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launch_options_default() {
        let opts = LaunchOptions::default();
        assert!(opts.headless);
        assert_eq!(opts.window_width, 1280);
        assert_eq!(opts.window_height, 720);
        assert_eq!(opts.debug_port, None);
        assert!(opts.sandbox);
    }

    #[test]
    fn test_launch_options_builder() {
        let opts = LaunchOptions::new()
            .headless(false)
            .window_size(1920, 1080)
            .debug_port(9222)
            .sandbox(false);

        assert!(!opts.headless);
        assert_eq!(opts.window_width, 1920);
        assert_eq!(opts.window_height, 1080);
        assert_eq!(opts.debug_port, Some(9222));
        assert!(!opts.sandbox);
    }

    #[test]
    fn test_connection_options() {
        let opts = ConnectionOptions::new("ws://localhost:9222");

        assert_eq!(opts.ws_url, "ws://localhost:9222");
    }

    #[test]
    fn test_connection_options_passes_websocket_urls_through() {
        let opts = ConnectionOptions::new("ws://127.0.0.1:9222/devtools/browser/test");

        assert_eq!(
            opts.resolved_ws_url()
                .expect("websocket URLs should pass through"),
            "ws://127.0.0.1:9222/devtools/browser/test"
        );
    }

    #[test]
    fn test_connection_options_appends_json_version_for_http_origin() {
        assert_eq!(
            devtools_version_url("http://127.0.0.1:9222").expect("http origin should normalize"),
            "http://127.0.0.1:9222/json/version"
        );
    }

    #[test]
    fn test_connection_options_keeps_explicit_json_version_url() {
        assert_eq!(
            devtools_version_url("http://127.0.0.1:9222/json/version")
                .expect("explicit json/version should be preserved"),
            "http://127.0.0.1:9222/json/version"
        );
    }

    #[test]
    fn test_connection_options_rejects_unknown_scheme() {
        let err = ConnectionOptions::new("localhost:9222")
            .resolved_ws_url()
            .expect_err("unknown schemes should fail");

        assert!(matches!(err, BrowserError::ConnectionFailed(_)));
        assert!(
            err.to_string()
                .contains("Unsupported browser endpoint 'localhost:9222'")
        );
    }

    #[test]
    fn test_connection_options_parses_devtools_version_body() {
        let resolved = parse_browser_ws_url_from_version_body(
            "http://127.0.0.1:9222/json/version",
            r#"{"webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/browser/fake"}"#,
        )
        .expect("version body should expose websocket URL");

        assert_eq!(resolved, "ws://127.0.0.1:9222/devtools/browser/fake");
    }
}
