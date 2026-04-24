use crate::browser::backend::{
    ChromeSessionBackend, ScreenshotCapture, ScreenshotRequest, ScriptEvaluation, SessionBackend,
    ViewportEmulationRequest, ViewportOperationResult, ViewportResetRequest,
};
#[cfg(test)]
use crate::browser::backend::{
    DEBUG_PORT_END, DEBUG_PORT_START, FakeSessionBackend, build_launch_options, choose_debug_port,
};
#[cfg(test)]
use crate::browser::config::CHROME_BROWSER_IDLE_TIMEOUT;
use crate::browser::{ConnectionOptions, LaunchOptions};
use crate::dom::{DocumentMetadata, DomTree};
use crate::error::{BrowserError, Result};
use crate::tools::{ToolContext, ToolRegistry};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) mod cache;
mod history;
mod tabs;

pub use cache::ScreenshotArtifact;
#[cfg(test)]
pub(crate) use cache::SnapshotCacheScope;
pub(crate) use cache::{MarkdownCacheEntry, SnapshotCacheEntry};

/// Browser session that manages a Chrome/Chromium instance
pub struct BrowserSession {
    backend: Arc<dyn SessionBackend>,

    /// Retains whether the session launched a disposable browser or attached
    /// to an existing browser instance.
    #[cfg_attr(not(test), allow(dead_code))]
    origin: SessionOrigin,

    /// Tracks tabs explicitly owned by this session so attach-mode callers can
    /// distinguish them from pre-existing browser tabs.
    managed_tab_ids: Mutex<HashSet<String>>,

    /// Tool registry for executing browser automation tools
    tool_registry: ToolRegistry,

    /// Cache the most recent markdown extraction by document revision.
    markdown_cache: Mutex<Option<Arc<MarkdownCacheEntry>>>,

    /// Cache the most recent snapshot base for delta-style follow-up reads.
    snapshot_cache: Mutex<Option<Arc<SnapshotCacheEntry>>>,

    /// Managed screenshot artifacts retained for the current session.
    screenshot_artifacts: Mutex<VecDeque<Arc<ScreenshotArtifact>>>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionOrigin {
    Launched,
    Connected,
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
    pub id: String,
    pub title: String,
    pub url: String,
    pub active_tab: Option<TabInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedTabsCloseSummary {
    pub closed_tabs: usize,
    pub skipped_tabs: usize,
}

impl BrowserSession {
    /// Launch a new browser instance with the given options
    pub fn launch(options: LaunchOptions) -> Result<Self> {
        Self::from_backend_with_origin(
            ChromeSessionBackend::launch(options)?,
            SessionOrigin::Launched,
        )
    }

    /// Connect to an existing browser instance via the browser WebSocket URL or
    /// a stable DevTools HTTP endpoint such as `http://127.0.0.1:9222`.
    pub fn connect(options: ConnectionOptions) -> Result<Self> {
        Self::from_backend_with_origin(
            ChromeSessionBackend::connect(options)?,
            SessionOrigin::Connected,
        )
    }

    /// Launch a browser with default options
    pub fn new() -> Result<Self> {
        Self::launch(LaunchOptions::default())
    }

    /// Navigate to a URL using the active tab
    pub fn navigate(&self, url: &str) -> Result<()> {
        self.backend.navigate(url)?;
        self.invalidate_snapshot_cache()
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

    /// Extract the DOM tree from a specific tab without activating it.
    pub(crate) fn extract_dom_for_tab(&self, tab_id: &str) -> Result<DomTree> {
        self.backend.extract_dom_for_tab(tab_id)
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

    pub(crate) fn evaluate_on_tab(
        &self,
        tab_id: &str,
        script: &str,
        await_promise: bool,
    ) -> Result<ScriptEvaluation> {
        self.backend.evaluate_on_tab(tab_id, script, await_promise)
    }

    #[cfg(test)]
    pub(crate) fn capture_screenshot(&self, full_page: bool) -> Result<Vec<u8>> {
        let artifact =
            self.capture_screenshot_artifact(ScreenshotRequest::from_legacy_full_page(full_page))?;
        Ok(artifact.bytes().as_ref().to_vec())
    }

    #[allow(dead_code)]
    pub(crate) fn capture_screenshot_artifact(
        &self,
        request: ScreenshotRequest,
    ) -> Result<Arc<ScreenshotArtifact>> {
        let capture = self.backend.capture_screenshot_with_request(&request)?;
        self.store_screenshot_artifact(capture)
    }

    pub(crate) fn capture_screenshot_artifact_with_capture(
        &self,
        request: ScreenshotRequest,
    ) -> Result<(Arc<ScreenshotArtifact>, ScreenshotCapture)> {
        let capture = self.backend.capture_screenshot_with_request(&request)?;
        let artifact = self.store_screenshot_artifact(capture.clone())?;
        Ok((artifact, capture))
    }

    pub(crate) fn apply_viewport_emulation(
        &self,
        request: ViewportEmulationRequest,
    ) -> Result<ViewportOperationResult> {
        let result = self.backend.apply_viewport_emulation(&request)?;
        self.invalidate_snapshot_cache()?;
        Ok(result)
    }

    pub(crate) fn reset_viewport_emulation(
        &self,
        request: ViewportResetRequest,
    ) -> Result<ViewportOperationResult> {
        let result = self.backend.reset_viewport_emulation(&request)?;
        self.invalidate_snapshot_cache()?;
        Ok(result)
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
        self.backend.close()?;
        self.clear_screenshot_artifacts()?;
        self.invalidate_snapshot_cache()?;
        self.clear_managed_tabs()
    }

    fn from_backend_with_origin<B: SessionBackend + 'static>(
        backend: B,
        origin: SessionOrigin,
    ) -> Result<Self> {
        let managed_tab_ids = match origin {
            SessionOrigin::Launched => backend
                .list_tabs()?
                .into_iter()
                .map(|tab| tab.id)
                .collect::<HashSet<_>>(),
            SessionOrigin::Connected => HashSet::new(),
        };

        Ok(Self {
            backend: Arc::new(backend),
            origin,
            managed_tab_ids: Mutex::new(managed_tab_ids),
            tool_registry: ToolRegistry::with_defaults(),
            markdown_cache: Mutex::new(None),
            snapshot_cache: Mutex::new(None),
            screenshot_artifacts: Mutex::new(VecDeque::new()),
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn session_origin(&self) -> SessionOrigin {
        self.origin
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_connected_session(&self) -> bool {
        self.origin == SessionOrigin::Connected
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_tab_managed(&self, tab_id: &str) -> Result<bool> {
        Ok(self.managed_tab_ids()?.contains(tab_id))
    }

    pub(crate) fn session_origin_label(&self) -> &'static str {
        match self.origin {
            SessionOrigin::Launched => "launched",
            SessionOrigin::Connected => "connected",
        }
    }

    pub(crate) fn remember_managed_tab(&self, tab_id: impl Into<String>) -> Result<()> {
        self.managed_tab_ids()?.insert(tab_id.into());
        Ok(())
    }

    pub(crate) fn forget_managed_tab(&self, tab_id: &str) -> Result<()> {
        self.managed_tab_ids()?.remove(tab_id);
        Ok(())
    }

    fn clear_managed_tabs(&self) -> Result<()> {
        self.managed_tab_ids()?.clear();
        Ok(())
    }

    fn managed_tab_ids(&self) -> Result<std::sync::MutexGuard<'_, HashSet<String>>> {
        self.managed_tab_ids.lock().map_err(|e| {
            BrowserError::TabOperationFailed(format!("Failed to access managed tab state: {}", e))
        })
    }

    #[cfg(test)]
    pub(crate) fn with_test_backend<B: SessionBackend + 'static>(backend: B) -> Self {
        Self::from_backend_with_origin(backend, SessionOrigin::Launched)
            .expect("test backend should construct")
    }

    #[cfg(test)]
    pub(crate) fn with_test_backend_origin<B: SessionBackend + 'static>(
        backend: B,
        origin: SessionOrigin,
    ) -> Self {
        Self::from_backend_with_origin(backend, origin).expect("test backend should construct")
    }

    #[cfg(test)]
    pub(crate) fn managed_tab_ids_for_test(&self) -> Vec<String> {
        let mut ids = self
            .managed_tab_ids()
            .expect("managed tab state should be readable")
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    #[cfg(test)]
    pub(crate) fn screenshot_artifacts_for_test(&self) -> Vec<Arc<ScreenshotArtifact>> {
        self.screenshot_artifacts
            .lock()
            .expect("screenshot artifact state should be readable")
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::backend::{
        ViewportEmulationRequest, ViewportOrientation, ViewportResetRequest,
    };
    use crate::browser::launch_error_is_environmental;
    use crate::browser::{ScreenshotMode, ScreenshotRequest};
    use crate::dom::SnapshotNode;
    use serde_json::json;
    use std::ffi::OsStr;
    use std::sync::Arc;

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

    fn seed_snapshot_cache(session: &BrowserSession) {
        let document = session
            .document_metadata()
            .expect("document metadata should be available");

        session
            .store_snapshot_cache(Arc::new(SnapshotCacheEntry {
                document,
                snapshot: Arc::<str>::from("button \"Fake target\""),
                nodes: Arc::<[SnapshotNode]>::from(Vec::new()),
                scope: SnapshotCacheScope {
                    mode: "viewport".to_string(),
                    fallback_mode: None,
                    viewport_biased: true,
                    returned_node_count: 0,
                    unavailable_frame_count: 0,
                    global_interactive_count: Some(1),
                },
            }))
            .expect("snapshot cache should store");
    }

    fn read_viewport_metrics(
        session: &BrowserSession,
        tab_id: Option<&str>,
    ) -> (f64, f64, f64, f64) {
        let evaluation = match tab_id {
            Some(tab_id) => session.evaluate_on_tab(
                tab_id,
                r#"(() => [
                    window.innerWidth,
                    window.innerHeight,
                    window.devicePixelRatio || 1,
                    Math.max(
                        document.documentElement.scrollHeight,
                        document.body ? document.body.scrollHeight : 0
                    )
                ])()"#,
                false,
            ),
            None => session.evaluate(
                r#"(() => [
                    window.innerWidth,
                    window.innerHeight,
                    window.devicePixelRatio || 1,
                    Math.max(
                        document.documentElement.scrollHeight,
                        document.body ? document.body.scrollHeight : 0
                    )
                ])()"#,
                false,
            ),
        }
        .expect("viewport metrics should be readable");

        let metrics = evaluation
            .value
            .expect("viewport metrics should include a value");
        let metrics = metrics
            .as_array()
            .expect("viewport metrics should return an array");
        (
            metrics[0].as_f64().expect("innerWidth should be numeric"),
            metrics[1].as_f64().expect("innerHeight should be numeric"),
            metrics[2]
                .as_f64()
                .expect("devicePixelRatio should be numeric"),
            metrics[3].as_f64().expect("scrollHeight should be numeric"),
        )
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
            CHROME_BROWSER_IDLE_TIMEOUT
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
    #[ignore]
    fn test_attach_session_survives_idle_timeout_window() {
        let port = choose_debug_port();
        let Some(_launched) = launch_or_skip(BrowserSession::launch(
            LaunchOptions::new().headless(true).debug_port(port),
        )) else {
            return;
        };

        let attached =
            BrowserSession::connect(ConnectionOptions::new(format!("http://127.0.0.1:{port}")))
                .expect("attach session should connect to launched browser");

        attached
            .navigate("data:text/html,<html><body><button id='save'>Save</button></body></html>")
            .expect("attached session should navigate");
        attached
            .wait_for_document_ready_with_timeout(Duration::from_secs(5))
            .expect("attached session should reach readyState complete");

        std::thread::sleep(Duration::from_secs(31));

        let snapshot = attached
            .execute_tool("snapshot", json!({}))
            .expect("snapshot should execute after the old 30-second timeout window");

        assert!(snapshot.success);
        let data = snapshot.data.expect("snapshot should include data");
        assert!(
            data["snapshot"]
                .as_str()
                .unwrap_or_default()
                .contains("button")
        );
        assert!(data["document"]["revision"].as_str().is_some());
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
    fn test_navigate_invalidates_snapshot_cache() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        seed_snapshot_cache(&session);
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_some()
        );

        session
            .navigate("https://example.com")
            .expect("navigation should succeed");

        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_none()
        );
    }

    #[test]
    fn test_apply_viewport_emulation_invalidates_snapshot_cache_without_advancing_revision() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let revision_before = session
            .document_metadata()
            .expect("document metadata should be available")
            .revision;
        seed_snapshot_cache(&session);

        let result = session
            .apply_viewport_emulation(ViewportEmulationRequest {
                width: 375,
                height: 812,
                device_scale_factor: 2.0,
                mobile: true,
                touch: true,
                orientation: Some(ViewportOrientation::PortraitPrimary),
                tab_id: None,
            })
            .expect("viewport emulation should succeed");

        assert_eq!(result.tab_id, "tab-1");
        assert_eq!(result.viewport_after.width, 375.0);
        assert_eq!(result.viewport_after.height, 812.0);
        assert_eq!(result.viewport_after.device_pixel_ratio, 2.0);
        assert_eq!(
            result.emulation,
            Some(crate::browser::backend::ViewportEmulation {
                width: 375,
                height: 812,
                device_scale_factor: 2.0,
                mobile: true,
                touch: true,
                orientation: Some(ViewportOrientation::PortraitPrimary),
            })
        );
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_none()
        );
        assert_eq!(
            session
                .document_metadata()
                .expect("document metadata should still be available")
                .revision,
            revision_before,
            "viewport-only changes should not advance the fake document revision"
        );
        assert_eq!(
            read_viewport_metrics(&session, None),
            (375.0, 812.0, 2.0, 1800.0)
        );
    }

    #[test]
    fn test_apply_viewport_emulation_can_target_inactive_tab_without_activation() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let first_tab_id = session.list_tabs().expect("tabs should list")[0].id.clone();
        let second_tab_id = session
            .open_tab_entry("https://second.example")
            .expect("second tab should open")
            .id;

        let result = session
            .apply_viewport_emulation(ViewportEmulationRequest {
                width: 640,
                height: 360,
                device_scale_factor: 1.5,
                mobile: false,
                touch: false,
                orientation: None,
                tab_id: Some(first_tab_id.clone()),
            })
            .expect("targeted viewport emulation should succeed");

        assert_eq!(result.tab_id, first_tab_id);
        assert_eq!(
            session
                .list_tabs()
                .expect("tabs should list")
                .into_iter()
                .find(|tab| tab.active)
                .expect("an active tab should remain")
                .id,
            second_tab_id,
            "specific-tab emulation should not activate the target tab"
        );
        assert_eq!(
            read_viewport_metrics(&session, Some(&result.tab_id)),
            (640.0, 360.0, 1.5, 1800.0)
        );
        assert_eq!(
            read_viewport_metrics(&session, Some(&second_tab_id)),
            (800.0, 600.0, 2.0, 1800.0)
        );
    }

    #[test]
    fn test_reset_viewport_emulation_restores_default_fake_metrics() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        session
            .apply_viewport_emulation(ViewportEmulationRequest {
                width: 1024,
                height: 768,
                device_scale_factor: 1.25,
                mobile: false,
                touch: false,
                orientation: None,
                tab_id: None,
            })
            .expect("viewport emulation should succeed");
        seed_snapshot_cache(&session);

        let result = session
            .reset_viewport_emulation(ViewportResetRequest::default())
            .expect("viewport reset should succeed");

        assert_eq!(result.tab_id, "tab-1");
        assert!(result.emulation.is_none());
        assert_eq!(result.viewport_after.width, 800.0);
        assert_eq!(result.viewport_after.height, 600.0);
        assert_eq!(result.viewport_after.device_pixel_ratio, 2.0);
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("snapshot cache should be readable")
                .is_none()
        );
        assert_eq!(
            read_viewport_metrics(&session, None),
            (800.0, 600.0, 2.0, 1800.0)
        );
    }

    #[test]
    fn test_apply_viewport_emulation_rejects_invalid_requests_without_mutation() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        let oversize = session.apply_viewport_emulation(ViewportEmulationRequest {
            width: 10_000_001,
            height: 600,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
            orientation: None,
            tab_id: None,
        });
        assert!(matches!(oversize, Err(BrowserError::InvalidArgument(_))));
        assert_eq!(
            read_viewport_metrics(&session, None),
            (800.0, 600.0, 2.0, 1800.0)
        );

        let empty_tab_id = session.apply_viewport_emulation(ViewportEmulationRequest {
            width: 320,
            height: 640,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
            orientation: None,
            tab_id: Some("   ".to_string()),
        });
        assert!(matches!(
            empty_tab_id,
            Err(BrowserError::InvalidArgument(_))
        ));
        assert_eq!(
            read_viewport_metrics(&session, None),
            (800.0, 600.0, 2.0, 1800.0)
        );

        let unknown_tab = session.apply_viewport_emulation(ViewportEmulationRequest {
            width: 320,
            height: 640,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
            orientation: None,
            tab_id: Some("missing-tab".to_string()),
        });
        assert!(matches!(
            unknown_tab,
            Err(BrowserError::TabOperationFailed(_))
        ));
        assert_eq!(
            read_viewport_metrics(&session, None),
            (800.0, 600.0, 2.0, 1800.0)
        );
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
        let new_tab_data = new_tab.data.expect("new_tab should include data");
        assert_eq!(new_tab_data["action"].as_str(), Some("new_tab"));
        assert_eq!(new_tab_data["tab"]["tab_id"].as_str(), Some("tab-2"));
        assert_eq!(new_tab_data["active_tab"]["tab_id"].as_str(), Some("tab-2"));

        let tab_list = session
            .execute_tool("tab_list", json!({}))
            .expect("tab_list should execute");
        let tab_list_data = tab_list.data.expect("tab_list should include data");
        assert_eq!(tab_list_data["count"].as_u64(), Some(2));
        assert_eq!(tab_list_data["tabs"][1]["tab_id"].as_str(), Some("tab-2"));
        assert_eq!(
            tab_list_data["active_tab"]["tab_id"].as_str(),
            Some("tab-2")
        );
        assert_eq!(
            tab_list_data["tabs"][1]["url"].as_str(),
            Some("https://second.example")
        );
        assert_eq!(tab_list_data["tabs"][1]["active"].as_bool(), Some(true));

        let switched = session
            .execute_tool("switch_tab", json!({ "tab_id": "tab-1" }))
            .expect("switch_tab should execute");
        let switched_data = switched.data.expect("switch_tab should include data");
        assert_eq!(switched_data["tab"]["index"].as_u64(), Some(0));
        assert_eq!(
            switched_data["active_tab"]["tab_id"].as_str(),
            Some("tab-1")
        );

        let closed = session
            .execute_tool("close_tab", json!({}))
            .expect("close_tab should execute");
        let closed_data = closed.data.expect("close_tab should include data");
        assert_eq!(closed_data["closed_tab"]["index"].as_u64(), Some(0));
        assert_eq!(closed_data["closed_tab"]["tab_id"].as_str(), Some("tab-1"));
        assert_eq!(closed_data["active_tab"]["tab_id"].as_str(), Some("tab-2"));
        assert_eq!(
            closed_data["closed_tab"]["url"].as_str(),
            Some("about:blank")
        );

        let remaining = session
            .execute_tool("tab_list", json!({}))
            .expect("tab_list should execute after close");
        let remaining_data = remaining.data.expect("tab_list should include data");
        assert_eq!(remaining_data["count"].as_u64(), Some(1));
        assert_eq!(
            remaining_data["tabs"][0]["url"].as_str(),
            Some("https://second.example")
        );
        assert_eq!(remaining_data["tabs"][0]["active"].as_bool(), Some(true));
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
                .contains("tab_id")
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
        assert_eq!(data["details"]["tool"].as_str(), Some("close"));
        assert!(
            data["error"]
                .as_str()
                .unwrap_or_default()
                .contains("stuck.example")
        );
    }

    #[test]
    fn test_launch_session_seeds_and_tracks_managed_tabs() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        assert_eq!(session.session_origin(), SessionOrigin::Launched);
        assert!(!session.is_connected_session());

        let initial_id = session.list_tabs().expect("initial tabs should list")[0]
            .id
            .clone();
        assert!(
            session
                .is_tab_managed(&initial_id)
                .expect("managed state should read")
        );

        let opened = session
            .open_tab_entry("https://managed.example")
            .expect("managed tab should open");
        assert!(
            session
                .is_tab_managed(&opened.id)
                .expect("opened tab should be tracked")
        );
        assert_eq!(
            session.managed_tab_ids_for_test(),
            vec![initial_id, opened.id.clone()]
        );

        session.close().expect("session close should succeed");
        assert!(session.managed_tab_ids_for_test().is_empty());
    }

    #[test]
    fn test_connected_session_tracks_only_tabs_opened_through_session() {
        let session = BrowserSession::with_test_backend_origin(
            FakeSessionBackend::new(),
            SessionOrigin::Connected,
        );

        assert_eq!(session.session_origin(), SessionOrigin::Connected);
        assert!(session.is_connected_session());

        let existing_id = session.list_tabs().expect("initial tabs should list")[0]
            .id
            .clone();
        assert!(
            !session
                .is_tab_managed(&existing_id)
                .expect("existing connected tab should be readable")
        );

        let opened = session
            .open_tab_entry("https://managed.example")
            .expect("managed tab should open");
        assert!(
            session
                .is_tab_managed(&opened.id)
                .expect("opened tab should be tracked")
        );
        assert_eq!(session.managed_tab_ids_for_test(), vec![opened.id.clone()]);

        let closed = session
            .close_active_tab_summary()
            .expect("active managed tab should close");
        assert_eq!(closed.url, "https://managed.example");
        assert_eq!(closed.id, opened.id);
        let active_tab = closed
            .active_tab
            .expect("remaining about:blank tab should become active");
        assert_eq!(active_tab.id, existing_id);
        assert!(active_tab.active);
        assert!(
            !session
                .is_tab_managed(&opened.id)
                .expect("closed tab should be forgotten")
        );
        assert!(session.managed_tab_ids_for_test().is_empty());
        assert!(
            !session
                .is_tab_managed(&existing_id)
                .expect("pre-existing tab should stay unmanaged")
        );
    }

    #[test]
    fn test_legacy_capture_screenshot_stores_managed_artifact() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        let bytes = session
            .capture_screenshot(true)
            .expect("legacy screenshot capture should succeed");

        assert!(
            bytes.starts_with(&[137, 80, 78, 71]),
            "legacy path should still return png bytes"
        );

        let artifacts = session.screenshot_artifacts_for_test();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].mode, ScreenshotMode::FullPage);
        assert_eq!(artifacts[0].byte_count, bytes.len());
    }

    #[test]
    fn test_close_clears_managed_screenshot_artifacts() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let artifact = session
            .capture_screenshot_artifact(ScreenshotRequest::default())
            .expect("managed screenshot should succeed");
        let path = artifact.path.clone();
        assert!(path.exists(), "managed screenshot should exist on disk");

        session.close().expect("session close should succeed");

        assert!(session.screenshot_artifacts_for_test().is_empty());
        assert!(
            !path.exists(),
            "managed screenshot artifacts should be removed on close"
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

    #[test]
    #[ignore]
    fn test_apply_and_reset_viewport_emulation_live() {
        let Some(session) =
            launch_or_skip(BrowserSession::launch(LaunchOptions::new().headless(true)))
        else {
            return;
        };

        session
            .navigate(
                "data:text/html,<html><body style='margin:0'><div style='height:2000px'>viewport</div></body></html>",
            )
            .expect("navigation should succeed");
        session
            .wait_for_document_ready_with_timeout(Duration::from_secs(5))
            .expect("document should become ready");

        let baseline = read_viewport_metrics(&session, None);

        let applied = session
            .apply_viewport_emulation(ViewportEmulationRequest {
                width: 412,
                height: 915,
                device_scale_factor: 2.0,
                mobile: true,
                touch: true,
                orientation: Some(ViewportOrientation::PortraitPrimary),
                tab_id: None,
            })
            .expect("viewport emulation should apply");

        assert!((applied.viewport_after.width - 412.0).abs() <= 1.0);
        assert!((applied.viewport_after.height - 915.0).abs() <= 1.0);
        assert!((applied.viewport_after.device_pixel_ratio - 2.0).abs() <= 0.1);

        let applied_metrics = read_viewport_metrics(&session, None);
        assert!((applied_metrics.0 - 412.0).abs() <= 1.0);
        assert!((applied_metrics.1 - 915.0).abs() <= 1.0);
        assert!((applied_metrics.2 - 2.0).abs() <= 0.1);

        let reset = session
            .reset_viewport_emulation(ViewportResetRequest::default())
            .expect("viewport reset should succeed");

        assert!(reset.emulation.is_none());
        assert!((reset.viewport_after.width - baseline.0).abs() <= 2.0);
        assert!((reset.viewport_after.height - baseline.1).abs() <= 2.0);
        assert!((reset.viewport_after.device_pixel_ratio - baseline.2).abs() <= 0.2);
    }
}
