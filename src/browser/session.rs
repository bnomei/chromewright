use crate::browser::backend::{
    ChromeSessionBackend, ScriptEvaluation, SessionBackend,
};
#[cfg(test)]
use crate::browser::backend::{
    DEBUG_PORT_END, DEBUG_PORT_START, FakeSessionBackend, build_launch_options, choose_debug_port,
};
use crate::browser::{ConnectionOptions, LaunchOptions};
use crate::dom::{DocumentMetadata, DomTree};
use crate::error::Result;
use crate::tools::{ToolContext, ToolRegistry};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod cache;
mod history;
mod tabs;

pub(crate) use cache::MarkdownCacheEntry;

/// Browser session that manages a Chrome/Chromium instance
pub struct BrowserSession {
    backend: Arc<dyn SessionBackend>,

    /// Tool registry for executing browser automation tools
    tool_registry: ToolRegistry,

    /// Cache the most recent markdown extraction by document revision.
    markdown_cache: Mutex<Option<Arc<MarkdownCacheEntry>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TabInfo {
    pub id: String,
    pub title: String,
    pub url: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClosedTabSummary {
    pub index: usize,
    pub title: String,
    pub url: String,
}

impl BrowserSession {
    /// Launch a new browser instance with the given options
    pub fn launch(options: LaunchOptions) -> Result<Self> {
        Self::from_backend(ChromeSessionBackend::launch(options)?)
    }

    /// Connect to an existing browser instance via WebSocket
    pub fn connect(options: ConnectionOptions) -> Result<Self> {
        Self::from_backend(ChromeSessionBackend::connect(options)?)
    }

    /// Launch a browser with default options
    pub fn new() -> Result<Self> {
        Self::launch(LaunchOptions::default())
    }

    /// Navigate to a URL using the active tab
    pub fn navigate(&self, url: &str) -> Result<()> {
        self.backend.navigate(url)
    }

    /// Read document metadata from the active tab without rebuilding the full DOM snapshot.
    pub fn document_metadata(&self) -> Result<DocumentMetadata> {
        self.backend.document_metadata()
    }

    /// Wait for navigation to complete
    pub fn wait_for_navigation(&self) -> Result<()> {
        self.backend.wait_for_navigation()
    }

    /// Read the current document ready state from the active tab.
    pub fn document_ready_state(&self) -> Result<String> {
        Ok(self.document_metadata()?.ready_state)
    }

    /// Wait for the current document to reach the `complete` ready state.
    pub fn wait_for_document_ready_with_timeout(&self, timeout: Duration) -> Result<()> {
        self.backend.wait_for_document_ready_with_timeout(timeout)
    }

    /// Extract the DOM tree from the active tab
    pub fn extract_dom(&self) -> Result<DomTree> {
        self.backend.extract_dom()
    }

    /// Extract the DOM tree with a custom ref prefix (for iframe handling)
    pub fn extract_dom_with_prefix(&self, prefix: &str) -> Result<DomTree> {
        self.backend.extract_dom_with_prefix(prefix)
    }

    /// Get the tool registry
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// Get mutable tool registry
    pub fn tool_registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tool_registry
    }

    /// Execute a tool by name
    pub fn execute_tool(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<crate::tools::ToolResult> {
        let mut context = ToolContext::new(self);
        self.tool_registry.execute(name, params, &mut context)
    }

    /// List browser tabs using backend-neutral descriptors.
    pub fn list_tabs(&self) -> Result<Vec<TabInfo>> {
        self.tab_overview()
    }

    /// Activate a tab by backend-neutral tab id.
    pub fn activate_tab(&self, tab_id: &str) -> Result<()> {
        self.activate_tab_by_id(tab_id)
    }

    /// Open a new tab and mark it active.
    pub fn open_tab(&self, url: &str) -> Result<TabInfo> {
        let tab = self.open_tab_entry(url)?;

        Ok(TabInfo {
            id: tab.id,
            title: tab.title,
            url: tab.url,
            active: true,
        })
    }

    /// Close the active tab and return its summary.
    pub fn close_active_tab(&self) -> Result<ClosedTabSummary> {
        self.close_active_tab_summary()
    }

    pub(crate) fn evaluate(&self, script: &str, await_promise: bool) -> Result<ScriptEvaluation> {
        self.backend.evaluate(script, await_promise)
    }

    pub(crate) fn capture_screenshot(&self, full_page: bool) -> Result<Vec<u8>> {
        self.backend.capture_screenshot(full_page)
    }

    pub(crate) fn press_key(&self, key: &str) -> Result<()> {
        self.backend.press_key(key)
    }

    /// Navigate back in browser history
    pub fn go_back(&self) -> Result<()> {
        self.go_back_with_metrics().map(|_| ())
    }

    /// Navigate forward in browser history
    pub fn go_forward(&self) -> Result<()> {
        self.go_forward_with_metrics().map(|_| ())
    }

    /// Close all open tabs in the current session backend.
    pub fn close(&self) -> Result<()> {
        self.backend.close()
    }

    fn from_backend<B: SessionBackend + 'static>(backend: B) -> Result<Self> {
        Ok(Self {
            backend: Arc::new(backend),
            tool_registry: ToolRegistry::with_defaults(),
            markdown_cache: Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_test_backend<B: SessionBackend + 'static>(backend: B) -> Self {
        Self::from_backend(backend).expect("test backend should construct")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::launch_error_is_environmental;
    use serde_json::json;
    use std::ffi::OsStr;

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
    fn test_launch_options_builder() {
        let opts = LaunchOptions::new().headless(true).window_size(800, 600);

        assert!(opts.headless);
        assert_eq!(opts.window_width, 800);
        assert_eq!(opts.window_height, 600);
    }

    #[test]
    fn test_connection_options() {
        let opts = ConnectionOptions::new("ws://localhost:9222");

        assert_eq!(opts.ws_url, "ws://localhost:9222");
    }

    #[test]
    fn test_choose_debug_port_advances_within_expected_range() {
        let first = choose_debug_port();
        let second = choose_debug_port();

        assert!((DEBUG_PORT_START..=DEBUG_PORT_END).contains(&first));
        assert!((DEBUG_PORT_START..=DEBUG_PORT_END).contains(&second));
        assert_ne!(first, second);
    }

    #[test]
    fn test_build_launch_options_maps_browser_settings() {
        let options = LaunchOptions::new()
            .headless(false)
            .window_size(1024, 768)
            .sandbox(false)
            .debug_port(45555)
            .chrome_path("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into())
            .user_data_dir("/tmp/chromewright-test".into());

        let launch_opts = build_launch_options(options);

        assert!(!launch_opts.headless);
        assert_eq!(launch_opts.window_size, Some((1024, 768)));
        assert_eq!(launch_opts.port, Some(45555));
        assert!(!launch_opts.sandbox);
        assert_eq!(
            launch_opts.path.as_deref(),
            Some(std::path::Path::new(
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
            ))
        );
        assert_eq!(
            launch_opts.user_data_dir.as_deref(),
            Some(std::path::Path::new("/tmp/chromewright-test"))
        );
        assert_eq!(
            launch_opts.idle_browser_timeout,
            Duration::from_secs(60 * 60)
        );
        assert!(
            launch_opts
                .ignore_default_args
                .iter()
                .any(|arg| *arg == OsStr::new("--enable-automation"))
        );
        assert!(
            launch_opts
                .args
                .iter()
                .any(|arg| { *arg == OsStr::new("--disable-blink-features=AutomationControlled") })
        );
    }

    #[test]
    fn test_build_launch_options_chooses_debug_port_when_missing() {
        let launch_opts = build_launch_options(LaunchOptions::new());
        let port = launch_opts.port.expect("port should be assigned");

        assert!((DEBUG_PORT_START..=DEBUG_PORT_END).contains(&port));
    }

    #[test]
    fn test_fake_backend_execute_tool_navigate_updates_document_metadata() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        let result = session
            .execute_tool(
                "navigate",
                json!({
                    "url": "https://example.com",
                    "wait_for_load": true
                }),
            )
            .expect("navigate should execute");

        assert!(result.success);
        let data = result.data.expect("navigate should include data");
        assert_eq!(data["url"].as_str(), Some("https://example.com"));
        assert_eq!(
            data["document"]["url"].as_str(),
            Some("https://example.com")
        );
        assert_eq!(data["document"]["ready_state"].as_str(), Some("complete"));
    }

    #[test]
    fn test_fake_backend_execute_tool_tab_workflow() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        let new_tab = session
            .execute_tool(
                "new_tab",
                json!({
                    "url": "https://second.example"
                }),
            )
            .expect("new_tab should execute");
        assert!(new_tab.success);

        let tab_list = session
            .execute_tool("tab_list", json!({}))
            .expect("tab_list should execute");
        let tab_list_data = tab_list.data.expect("tab_list should include data");
        assert_eq!(tab_list_data["count"].as_u64(), Some(2));
        assert_eq!(
            tab_list_data["tab_list"][1]["url"].as_str(),
            Some("https://second.example")
        );
        assert_eq!(tab_list_data["tab_list"][1]["active"].as_bool(), Some(true));

        let switched = session
            .execute_tool("switch_tab", json!({ "index": 0 }))
            .expect("switch_tab should execute");
        let switched_data = switched.data.expect("switch_tab should include data");
        assert_eq!(switched_data["index"].as_u64(), Some(0));

        let closed = session
            .execute_tool("close_tab", json!({}))
            .expect("close_tab should execute");
        let closed_data = closed.data.expect("close_tab should include data");
        assert_eq!(closed_data["index"].as_u64(), Some(0));
        assert_eq!(closed_data["url"].as_str(), Some("about:blank"));

        let remaining = session
            .execute_tool("tab_list", json!({}))
            .expect("tab_list should execute after close");
        let remaining_data = remaining.data.expect("tab_list should include data");
        assert_eq!(remaining_data["count"].as_u64(), Some(1));
        assert_eq!(
            remaining_data["tab_list"][0]["url"].as_str(),
            Some("https://second.example")
        );
        assert_eq!(
            remaining_data["tab_list"][0]["active"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn test_execute_tool_returns_structured_failure_for_invalid_parameters() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        let result = session
            .execute_tool("switch_tab", json!({}))
            .expect("invalid parameters should stay a tool failure");

        assert!(!result.success);
        let data = result
            .data
            .expect("invalid parameter failure should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_argument"));
        assert!(
            data["error"]
                .as_str()
                .unwrap_or_default()
                .contains("Invalid parameters")
        );
    }

    #[test]
    fn test_execute_tool_returns_structured_failure_for_close_errors() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::with_close_failures([
            "https://stuck.example",
        ]));
        session
            .open_tab_entry("https://stuck.example")
            .expect("stuck tab should open");

        let result = session
            .execute_tool("close", json!({}))
            .expect("close failures should stay a tool failure");

        assert!(!result.success);
        let data = result.data.expect("close failure should include details");
        assert_eq!(data["code"].as_str(), Some("tool_execution_failed"));
        assert_eq!(data["tool"].as_str(), Some("close"));
        assert!(
            data["error"]
                .as_str()
                .unwrap_or_default()
                .contains("stuck.example")
        );
    }

    #[test]
    #[ignore]
    fn test_list_tabs() {
        let Some(session) =
            launch_or_skip(BrowserSession::launch(LaunchOptions::new().headless(true)))
        else {
            return;
        };

        let tabs = session.list_tabs();
        assert!(tabs.is_ok());
    }

    // Integration tests (require Chrome to be installed)
    #[test]
    #[ignore] // Ignore by default, run with: cargo test -- --ignored
    fn test_launch_browser() {
        let Some(_session) =
            launch_or_skip(BrowserSession::launch(LaunchOptions::new().headless(true)))
        else {
            return;
        };
    }

    #[test]
    #[ignore]
    fn test_navigate() {
        let Some(session) =
            launch_or_skip(BrowserSession::launch(LaunchOptions::new().headless(true)))
        else {
            return;
        };

        let result = session.navigate("about:blank");
        assert!(result.is_ok());
    }

    #[test]
    #[ignore]
    fn test_open_tab() {
        let Some(session) =
            launch_or_skip(BrowserSession::launch(LaunchOptions::new().headless(true)))
        else {
            return;
        };

        let result = session.open_tab("about:blank");
        assert!(result.is_ok());

        let tabs = session.list_tabs().expect("Failed to list tabs");
        assert!(tabs.len() >= 2);
    }
}
