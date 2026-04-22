use std::path::PathBuf;

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
    /// WebSocket URL for Chrome DevTools Protocol
    pub ws_url: String,
}

impl ConnectionOptions {
    /// Create new ConnectionOptions with WebSocket URL
    pub fn new<S: Into<String>>(ws_url: S) -> Self {
        Self {
            ws_url: ws_url.into(),
        }
    }
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
}
