use crate::browser::backend::{
    ChromeSessionBackend, ScriptEvaluation, SessionBackend, TabDescriptor,
};
#[cfg(test)]
use crate::browser::backend::{
    DEBUG_PORT_END, DEBUG_PORT_START, FakeSessionBackend, build_launch_options, choose_debug_port,
};
use crate::browser::config::{ConnectionOptions, LaunchOptions};
use crate::dom::{DocumentMetadata, DomTree};
use crate::error::{BrowserError, Result};
use crate::tools::{ToolContext, ToolRegistry};
use headless_chrome::{Browser, Tab};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Wrapper for Tab and Element to maintain proper lifetime relationships
pub struct TabElement<'a> {
    pub tab: Arc<Tab>,
    pub element: headless_chrome::Element<'a>,
}

#[derive(Debug, Clone)]
pub(crate) struct MarkdownCacheEntry {
    pub document_id: String,
    pub revision: String,
    pub title: String,
    pub url: String,
    pub byline: String,
    pub excerpt: String,
    pub site_name: String,
    pub full_markdown: Arc<str>,
}

/// Browser session that manages a Chrome/Chromium instance
pub struct BrowserSession {
    backend: Arc<dyn SessionBackend>,

    /// Tool registry for executing browser automation tools
    tool_registry: ToolRegistry,

    /// Cache the most recent markdown extraction by document revision.
    markdown_cache: Mutex<Option<Arc<MarkdownCacheEntry>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionTab {
    pub id: String,
    pub title: String,
    pub url: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClosedTabSummary {
    pub index: usize,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HistoryNavigationMetrics {
    pub browser_evaluations: u64,
    pub poll_iterations: u64,
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

    /// Get the active tab.
    ///
    /// This is a Chrome-only escape hatch used by browser-backed tests and low-level integrations.
    pub fn tab(&self) -> Result<Arc<Tab>> {
        self.get_active_tab()
    }

    /// Create a new tab and set it as active.
    ///
    /// This is a Chrome-only escape hatch used by browser-backed tests and low-level integrations.
    pub fn new_tab(&mut self) -> Result<Arc<Tab>> {
        self.chrome_backend()?.create_tab_handle()
    }

    /// Get all tabs.
    ///
    /// This is a Chrome-only escape hatch used by browser-backed tests and low-level integrations.
    pub fn get_tabs(&self) -> Result<Vec<Arc<Tab>>> {
        self.chrome_backend()?.tabs()
    }

    /// Get the currently active tab.
    ///
    /// This is a Chrome-only escape hatch used by browser-backed tests and low-level integrations.
    pub fn get_active_tab(&self) -> Result<Arc<Tab>> {
        self.chrome_backend()?.active_tab_handle()
    }

    /// Close the active tab
    pub fn close_active_tab(&mut self) -> Result<()> {
        let active_tab = self.backend.active_tab()?;
        self.backend.close_tab(&active_tab.id, true)
    }

    /// Get the underlying Browser instance.
    ///
    /// This is a Chrome-only escape hatch used by browser-backed tests and low-level integrations.
    pub fn browser(&self) -> &Browser {
        self.chrome_backend()
            .expect("browser() is only available for the real Chrome backend")
            .browser()
    }

    /// Activate the provided tab and remember it as the active-tab hint.
    ///
    /// This is a Chrome-only escape hatch used by browser-backed tests and low-level integrations.
    pub fn activate_tab(&self, tab: &Arc<Tab>) -> Result<()> {
        self.chrome_backend()?.activate_real_tab(tab)
    }

    /// Open a new tab, navigate to the URL, wait for the initial load, and mark it active.
    ///
    /// This is a Chrome-only escape hatch used by browser-backed tests and low-level integrations.
    pub fn open_tab(&self, url: &str) -> Result<Arc<Tab>> {
        self.chrome_backend()?.open_real_tab(url)
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

    fn wait_for_history_settle(
        &self,
        previous_url: &str,
        timeout: Duration,
    ) -> Result<HistoryNavigationMetrics> {
        let start = Instant::now();
        let mut observed_navigation = false;
        let mut metrics = HistoryNavigationMetrics::default();

        loop {
            metrics.poll_iterations += 1;
            let document = self.document_metadata()?;
            let current_url = document.url;
            if current_url != previous_url {
                observed_navigation = true;
            }

            metrics.browser_evaluations += 1;
            let elapsed = start.elapsed();
            let grace_period = Duration::from_millis(500);

            if document.ready_state == "complete"
                && (observed_navigation || elapsed >= grace_period)
            {
                return Ok(metrics);
            }

            if elapsed >= timeout {
                return Err(BrowserError::Timeout(format!(
                    "History navigation did not settle within {} ms",
                    timeout.as_millis()
                )));
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Extract the DOM tree from the active tab
    pub fn extract_dom(&self) -> Result<DomTree> {
        self.backend.extract_dom()
    }

    /// Extract the DOM tree with a custom ref prefix (for iframe handling)
    pub fn extract_dom_with_prefix(&self, prefix: &str) -> Result<DomTree> {
        self.backend.extract_dom_with_prefix(prefix)
    }

    /// Find an element by CSS selector using the provided tab
    pub fn find_element<'a>(
        &self,
        tab: &'a Arc<Tab>,
        css_selector: &str,
    ) -> Result<headless_chrome::Element<'a>> {
        tab.find_element(css_selector).map_err(|e| {
            BrowserError::ElementNotFound(format!("Element '{}' not found: {}", css_selector, e))
        })
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

    pub(crate) fn markdown_cache_entry(
        &self,
        document: &DocumentMetadata,
    ) -> Result<Option<Arc<MarkdownCacheEntry>>> {
        let guard = self
            .markdown_cache
            .lock()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "get_markdown".to_string(),
                reason: format!("Failed to read markdown cache: {}", e),
            })?;

        Ok(guard.as_ref().and_then(|entry| {
            (entry.document_id == document.document_id && entry.revision == document.revision)
                .then_some(Arc::clone(entry))
        }))
    }

    pub(crate) fn store_markdown_cache(&self, entry: Arc<MarkdownCacheEntry>) -> Result<()> {
        *self
            .markdown_cache
            .lock()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "get_markdown".to_string(),
                reason: format!("Failed to write markdown cache: {}", e),
            })? = Some(entry);
        Ok(())
    }

    pub(crate) fn tab_overview(&self) -> Result<Vec<SessionTab>> {
        let tabs = self.backend.list_tabs()?;
        let active_id = match self.backend.active_tab() {
            Ok(tab) => Some(tab.id),
            Err(BrowserError::TabOperationFailed(reason))
                if reason.contains("No active tab found") =>
            {
                None
            }
            Err(err) => return Err(err),
        };

        Ok(tabs
            .into_iter()
            .map(|tab| SessionTab {
                active: active_id.as_deref() == Some(tab.id.as_str()),
                id: tab.id,
                title: tab.title,
                url: tab.url,
            })
            .collect())
    }

    pub(crate) fn activate_tab_by_id(&self, tab_id: &str) -> Result<()> {
        self.backend.activate_tab(tab_id)
    }

    pub(crate) fn open_tab_entry(&self, url: &str) -> Result<TabDescriptor> {
        self.backend.open_tab(url)
    }

    pub(crate) fn close_active_tab_summary(&self) -> Result<ClosedTabSummary> {
        let tabs = self.backend.list_tabs()?;
        let active = self.backend.active_tab()?;
        let index = tabs.iter().position(|tab| tab.id == active.id).unwrap_or(0);

        self.backend.close_tab(&active.id, true)?;

        Ok(ClosedTabSummary {
            index,
            title: active.title,
            url: active.url,
        })
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

    pub(crate) fn go_back_with_metrics(&self) -> Result<HistoryNavigationMetrics> {
        let previous_url = self.document_metadata()?.url;
        let go_back_js = r#"
            (function() {
                window.history.back();
                return true;
            })()
        "#;

        self.evaluate(go_back_js, false)
            .map_err(|e| BrowserError::NavigationFailed(format!("Failed to go back: {}", e)))?;
        let settle_metrics = self.wait_for_history_settle(&previous_url, Duration::from_secs(5))?;

        Ok(HistoryNavigationMetrics {
            browser_evaluations: settle_metrics.browser_evaluations + 1,
            poll_iterations: settle_metrics.poll_iterations,
        })
    }

    /// Navigate forward in browser history
    pub fn go_forward(&self) -> Result<()> {
        self.go_forward_with_metrics().map(|_| ())
    }

    pub(crate) fn go_forward_with_metrics(&self) -> Result<HistoryNavigationMetrics> {
        let previous_url = self.document_metadata()?.url;
        let go_forward_js = r#"
            (function() {
                window.history.forward();
                return true;
            })()
        "#;

        self.evaluate(go_forward_js, false)
            .map_err(|e| BrowserError::NavigationFailed(format!("Failed to go forward: {}", e)))?;
        let settle_metrics = self.wait_for_history_settle(&previous_url, Duration::from_secs(5))?;

        Ok(HistoryNavigationMetrics {
            browser_evaluations: settle_metrics.browser_evaluations + 1,
            poll_iterations: settle_metrics.poll_iterations,
        })
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

    fn chrome_backend(&self) -> Result<&ChromeSessionBackend> {
        self.backend
            .as_any()
            .downcast_ref::<ChromeSessionBackend>()
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(
                    "This operation requires the real Chrome backend".to_string(),
                )
            })
    }

    #[cfg(test)]
    pub(crate) fn with_test_backend<B: SessionBackend + 'static>(backend: B) -> Self {
        Self::from_backend(backend).expect("test backend should construct")
    }
}

impl Default for BrowserSession {
    fn default() -> Self {
        Self::new().expect("Failed to create default browser session")
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
        let opts = ConnectionOptions::new("ws://localhost:9222").timeout(5000);

        assert_eq!(opts.ws_url, "ws://localhost:9222");
        assert_eq!(opts.timeout, 5000);
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
    fn test_get_active_tab() {
        let Some(session) =
            launch_or_skip(BrowserSession::launch(LaunchOptions::new().headless(true)))
        else {
            return;
        };

        let tab = session.get_active_tab();
        assert!(tab.is_ok());
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
    fn test_new_tab() {
        let Some(mut session) =
            launch_or_skip(BrowserSession::launch(LaunchOptions::new().headless(true)))
        else {
            return;
        };

        let result = session.new_tab();
        assert!(result.is_ok());

        let tabs = session.get_tabs().expect("Failed to get tabs");
        assert!(tabs.len() >= 2);
    }
}
