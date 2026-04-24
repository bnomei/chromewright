//! Browser management module
//!
//! This module provides functionality for launching and managing Chrome/Chromium browser instances.
//! It includes configuration options, session management, and browser lifecycle control.

pub(crate) mod backend;
mod config;
mod session;

pub use backend::{
    ScreenshotClip, ScreenshotFormat, ScreenshotMode, ScreenshotRequest, ViewportEmulation,
    ViewportEmulationRequest, ViewportMetrics, ViewportOperationResult, ViewportOrientation,
    ViewportResetRequest,
};
pub use config::{ConnectionOptions, LaunchOptions};
#[cfg(test)]
pub(crate) use session::SessionOrigin;
pub(crate) use session::cache::{
    MarkdownCacheEntry, MarkdownCacheMetadata, SnapshotCacheEntry, SnapshotCacheScope,
};
pub use session::{BrowserSession, ClosedTabSummary, ScreenshotArtifact, TabInfo};

use crate::error::Result;

/// Initialize a new browser session with default options
pub fn init() -> Result<BrowserSession> {
    BrowserSession::new()
}

/// Initialize a new browser session with custom launch options
pub fn init_with_options(options: LaunchOptions) -> Result<BrowserSession> {
    BrowserSession::launch(options)
}

/// Connect to an existing browser instance.
///
/// Accepts either the browser-scoped DevTools WebSocket URL or a stable
/// DevTools HTTP endpoint such as `http://127.0.0.1:9222`.
pub fn connect(endpoint: &str) -> Result<BrowserSession> {
    BrowserSession::connect(ConnectionOptions::new(endpoint))
}

#[cfg(test)]
pub(super) fn launch_error_is_environmental(err: &crate::error::BrowserError) -> bool {
    let message = match err {
        crate::error::BrowserError::LaunchFailed(message)
        | crate::error::BrowserError::ChromeError(message) => message.as_str(),
        _ => return false,
    };

    [
        "didn't give us a WebSocket URL before we timed out",
        "There are no available ports between 8000 and 9000 for debugging",
        "Could not auto detect a chrome executable",
        "Running as root without --no-sandbox is not supported",
    ]
    .iter()
    .any(|fragment| message.contains(fragment))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch_or_skip(result: Result<BrowserSession>) -> Option<BrowserSession> {
        match result {
            Ok(session) => Some(session),
            Err(err) if launch_error_is_environmental(&err) => {
                eprintln!("Skipping browser launch test due to environment: {}", err);
                None
            }
            Err(err) => panic!("Unexpected launch failure: {}", err),
        }
    }

    #[test]
    fn test_launch_options_export() {
        let opts = LaunchOptions::new().headless(true);
        assert!(opts.headless);
    }

    #[test]
    fn test_connection_options_export() {
        let opts = ConnectionOptions::new("ws://localhost:9222");
        assert_eq!(opts.ws_url, "ws://localhost:9222");
    }

    #[test]
    fn test_launch_error_is_environmental_matches_known_messages() {
        assert!(launch_error_is_environmental(
            &crate::error::BrowserError::LaunchFailed(
                "Chrome launched, but didn't give us a WebSocket URL before we timed out"
                    .to_string(),
            ),
        ));
        assert!(launch_error_is_environmental(
            &crate::error::BrowserError::ChromeError(
                "Could not auto detect a chrome executable".to_string(),
            ),
        ));
        assert!(!launch_error_is_environmental(
            &crate::error::BrowserError::NavigationFailed("something else".to_string()),
        ));
    }

    #[test]
    #[ignore]
    fn test_init() {
        let Some(_session) = launch_or_skip(init()) else {
            return;
        };
    }

    #[test]
    #[ignore]
    fn test_init_with_options() {
        let opts = LaunchOptions::new().headless(true).window_size(1024, 768);

        let Some(_session) = launch_or_skip(init_with_options(opts)) else {
            return;
        };
    }
}
