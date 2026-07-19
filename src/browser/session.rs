//! Browser session state: backend handle, tool registry, and revision-keyed caches.
//!
//! Session helpers for tabs, history, and cache invalidation live in sibling modules.

use crate::browser::backend::{
    ChromeSessionBackend, ScreenshotCapture, ScreenshotRequest, ScriptEvaluation, SessionBackend,
};
#[cfg(test)]
use crate::browser::backend::{
    DEBUG_PORT_END, DEBUG_PORT_START, FakeSessionBackend, build_launch_options, choose_debug_port,
};
use crate::browser::commands::{BrowserCommand, BrowserCommandResult};
#[cfg(test)]
use crate::browser::config::CHROME_BROWSER_IDLE_TIMEOUT;
use crate::browser::{ConnectionOptions, LaunchOptions};
use crate::contract::{
    ViewportEmulationRequest, ViewportMetrics, ViewportOperationResult, ViewportResetRequest,
};
use crate::dom::{DocumentMetadata, DomTree};
use crate::error::{BrowserError, Result};
use crate::tools::utils::validate_startup_tab_url;
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

/// Owns the CDP backend, per-session caches, managed tabs, and default tool registry.
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

    /// Private per-session root for managed screenshot artifacts.
    screenshot_artifact_root: tempfile::TempDir,
}

/// Whether this session launched a disposable browser or attached to an existing one.
///
/// Launch mode seeds managed tabs from the initial process; attach mode starts with an empty
/// managed-tab set so pre-existing browser tabs are not closed as session-owned.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionOrigin {
    /// Browser process was started by chromewright (`BrowserSession::launch`).
    Launched,
    /// Connected to an existing DevTools endpoint (`BrowserSession::connect`).
    Connected,
}

/// Snapshot of one browser tab for `tab_list` and tab-management tools.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TabInfo {
    /// Stable backend tab id for `switch_tab` / `close_tab` (not a positional index).
    pub id: String,
    /// Tab title from the browser target.
    pub title: String,
    /// Tab URL from the browser target.
    pub url: String,
    /// Whether this tab is the session's active page target.
    pub active: bool,
}

/// Metadata returned when a tab is closed, including its index before removal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClosedTabSummary {
    /// Zero-based index of the closed tab in the pre-close tab list.
    pub index: usize,
    /// Stable id of the closed tab.
    pub id: String,
    /// Title captured at close time.
    pub title: String,
    /// URL captured at close time.
    pub url: String,
    /// Active tab after close, when one remains.
    pub active_tab: Option<TabInfo>,
}

/// Counts from closing session-owned managed tabs during attach-aware teardown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedTabsCloseSummary {
    /// Tabs successfully closed that were tracked as managed.
    pub closed_tabs: usize,
    /// Managed ids skipped because the backend no longer listed them.
    pub skipped_tabs: usize,
}

impl BrowserSession {
    /// Launch a disposable browser (launch mode) with the given process options.
    pub fn launch(options: LaunchOptions) -> Result<Self> {
        Self::from_backend_with_origin(
            ChromeSessionBackend::launch(options)?,
            SessionOrigin::Launched,
        )
    }

    /// Attach to an existing browser via WebSocket URL or DevTools HTTP endpoint (attach mode).
    ///
    /// Accepts either the browser-scoped DevTools WebSocket URL or a stable HTTP origin such as
    /// `http://127.0.0.1:9222`. Pre-existing tabs are not treated as managed tabs.
    pub fn connect(options: ConnectionOptions) -> Result<Self> {
        Self::from_backend_with_origin(
            ChromeSessionBackend::connect(options)?,
            SessionOrigin::Connected,
        )
    }

    /// Launch a disposable browser with default [`LaunchOptions`].
    pub fn new() -> Result<Self> {
        Self::launch(LaunchOptions::default())
    }

    /// Navigate the active tab and invalidate the Snapshot cache (markdown misses via revision/url keys).
    pub fn navigate(&self, url: &str) -> Result<()> {
        self.backend.navigate(url)?;
        self.invalidate_snapshot_cache()
    }

    /// Read document metadata (id, revision, url, ready state) from the active tab.
    pub fn document_metadata(&self) -> Result<DocumentMetadata> {
        self.backend.document_metadata()
    }

    /// Read document metadata for `tab_id` without activating that tab when the backend supports it.
    pub(crate) fn document_metadata_for_tab(&self, tab_id: &str) -> Result<DocumentMetadata> {
        self.backend.document_metadata_for_tab(tab_id)
    }

    /// Block until the active tab's in-flight navigation settles.
    pub fn wait_for_navigation(&self) -> Result<()> {
        self.backend.wait_for_navigation()
    }

    /// Read `document.readyState` from the active tab via document metadata.
    pub fn document_ready_state(&self) -> Result<String> {
        Ok(self.document_metadata()?.ready_state)
    }

    /// Poll until the active document reaches `readyState === "complete"` or `timeout` elapses.
    pub fn wait_for_document_ready_with_timeout(&self, timeout: Duration) -> Result<()> {
        self.backend.wait_for_document_ready_with_timeout(timeout)
    }

    /// Poll until main-frame document identity, revision, and URL stay unchanged
    /// for `quiet_for`, or until `timeout` elapses.
    ///
    /// Used after form field writes and SPA mutations: `readyState` alone is
    /// already `complete` on live pages, so capture must wait for the DOM
    /// MutationObserver revision counter to stop moving. On timeout returns
    /// `Ok(())` (fail-open) so callers still publish the latest complete snapshot.
    pub fn wait_for_dom_quiet(&self, timeout: Duration, quiet_for: Duration) -> Result<()> {
        use std::time::Instant;
        let start = Instant::now();
        let mut last_key: Option<(String, String, String)> = None;
        let mut stable_since: Option<Instant> = None;
        let poll = Duration::from_millis(50);

        loop {
            let meta = self.document_metadata()?;
            let main_rev = meta
                .revision
                .split('|')
                .next()
                .unwrap_or(meta.revision.as_str())
                .to_string();
            let key = (meta.document_id.clone(), main_rev, meta.url.clone());

            if last_key.as_ref() == Some(&key) {
                let since = stable_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= quiet_for {
                    return Ok(());
                }
            } else {
                last_key = Some(key);
                stable_since = Some(Instant::now());
            }

            if start.elapsed() >= timeout {
                return Ok(());
            }
            std::thread::sleep(poll);
        }
    }

    /// Extract the actionability/ARIA DOM tree from the active tab.
    pub fn extract_dom(&self) -> Result<DomTree> {
        self.backend.extract_dom()
    }

    /// Capture a bounded semantic document from the active tab.
    ///
    /// Independent of [`Self::extract_dom`]: reads the hydrated HTML DOM, not the
    /// actionability/ARIA tree. Available only with the opt-in `tui` feature.
    #[cfg(feature = "tui")]
    pub fn extract_semantic_document(&self) -> Result<crate::semantic::SemanticDocument> {
        crate::semantic::extract_semantic_document(self)
    }

    /// Extract the DOM tree from `tab_id` without activating it when the backend supports it.
    pub(crate) fn extract_dom_for_tab(&self, tab_id: &str) -> Result<DomTree> {
        self.backend.extract_dom_for_tab(tab_id)
    }

    /// Extract the DOM tree with a custom node-ref prefix (iframe / nested-document handling).
    pub fn extract_dom_with_prefix(&self, prefix: &str) -> Result<DomTree> {
        self.backend.extract_dom_with_prefix(prefix)
    }

    /// Shared tool registry used as the MCP/tool-dispatch surface for this session.
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// Mutable access to the session tool registry (tests and custom registration).
    pub fn tool_registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tool_registry
    }

    /// Execute a registered tool by name with a fresh [`ToolContext`] bound to this session.
    pub fn execute_tool(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<crate::tools::ToolResult> {
        let mut context = ToolContext::new(self);
        self.tool_registry.execute(name, params, &mut context)
    }

    /// List all backend tabs with active-flag resolution (attach-safe when page target is lost).
    pub fn list_tabs(&self) -> Result<Vec<TabInfo>> {
        self.tab_overview()
    }

    /// Activate a tab by stable `tab_id` and invalidate revision-scoped caches.
    pub fn activate_tab(&self, tab_id: &str) -> Result<()> {
        self.activate_tab_by_id(tab_id)
    }

    /// Open a new tab, record it as a managed tab, and mark it active.
    pub fn open_tab(&self, url: &str) -> Result<TabInfo> {
        let tab = self.open_tab_entry(url)?;

        Ok(TabInfo {
            id: tab.id,
            title: tab.title,
            url: tab.url,
            active: true,
        })
    }

    /// Seed the startup session with one tab per safe URL, in the supplied order.
    ///
    /// Launch mode reuses its initial managed page for the first URL, then opens
    /// one managed tab per remaining URL. Attach mode never mutates existing
    /// tabs, so it opens one new managed tab for every URL. All URLs are
    /// validated before any tab is opened or navigated; a failure while opening
    /// a later URL rolls back every startup tab that was newly opened.
    pub fn seed_startup_urls(&self, urls: &[String]) -> Result<Vec<TabInfo>> {
        let normalized_urls = urls
            .iter()
            .map(|url| validate_startup_tab_url(url))
            .collect::<Result<Vec<_>>>()?;

        let mut urls = normalized_urls.into_iter();
        let Some(first_url) = urls.next() else {
            return Ok(Vec::new());
        };

        let mut seeded = Vec::new();
        let mut opened_tab_ids = Vec::new();
        if self.origin == SessionOrigin::Launched {
            self.navigate(&first_url).map_err(|error| {
                BrowserError::NavigationFailed(format!(
                    "Failed to navigate the initial startup tab to '{first_url}': {error}"
                ))
            })?;
            self.wait_for_navigation().map_err(|error| {
                BrowserError::NavigationFailed(format!(
                    "Initial startup tab did not settle at '{first_url}': {error}"
                ))
            })?;
            let initial_tab = self
                .tab_overview()?
                .into_iter()
                .find(|tab| tab.active)
                .ok_or_else(|| {
                    BrowserError::TabOperationFailed(
                        "Initial startup tab was not active after navigation".into(),
                    )
                })?;
            seeded.push(initial_tab);
        } else {
            match self.open_tab(&first_url) {
                Ok(tab) => {
                    opened_tab_ids.push(tab.id.clone());
                    seeded.push(tab);
                }
                Err(error) => {
                    return Err(BrowserError::TabOperationFailed(format!(
                        "Failed to open initial URL 1 ('{first_url}'): {error}"
                    )));
                }
            }
        }

        for (index, url) in urls.enumerate() {
            match self.open_tab(&url) {
                Ok(tab) => {
                    opened_tab_ids.push(tab.id.clone());
                    seeded.push(tab);
                }
                Err(error) => {
                    let startup_error = BrowserError::TabOperationFailed(format!(
                        "Failed to open initial URL {} ('{url}'): {error}",
                        index + 2
                    ));
                    return match self.rollback_startup_tabs(&opened_tab_ids) {
                        Ok(()) => Err(startup_error),
                        Err(rollback_error) => Err(BrowserError::TabOperationFailed(format!(
                            "{startup_error}; startup tab rollback also failed: {rollback_error}"
                        ))),
                    };
                }
            }
        }

        Ok(seeded)
    }

    fn rollback_startup_tabs(&self, tab_ids: &[String]) -> Result<()> {
        let mut closed_tabs = 0usize;
        let mut failures = Vec::new();
        for tab_id in tab_ids.iter().rev() {
            match self.backend.close_tab(tab_id, false) {
                Ok(()) => {
                    closed_tabs += 1;
                    if let Err(error) = self.forget_managed_tab(tab_id) {
                        failures.push(format!(
                            "closed startup tab [id={tab_id}] but could not release managed ownership: {error}"
                        ));
                    }
                }
                Err(error) => failures.push(format!(
                    "failed to close startup tab [id={tab_id}]: {error}"
                )),
            }
        }
        if closed_tabs > 0 {
            if let Err(error) = self.invalidate_snapshot_cache() {
                failures.push(format!(
                    "failed to invalidate cache after rollback: {error}"
                ));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(BrowserError::TabOperationFailed(format!(
                "Startup tab rollback encountered {} error(s): {}",
                failures.len(),
                failures.join("; ")
            )))
        }
    }

    /// Close the active tab, drop managed ownership, and return a close summary.
    pub fn close_active_tab(&self) -> Result<ClosedTabSummary> {
        self.close_active_tab_summary()
    }

    /// Evaluate JavaScript on the active page target via the SessionBackend.
    ///
    /// Prefer typed [`BrowserCommand`] paths for product interactions; this is the escape hatch
    /// used by history navigation, readiness probes, and the guarded `evaluate` tool.
    pub(crate) fn evaluate(&self, script: &str, await_promise: bool) -> Result<ScriptEvaluation> {
        self.backend.evaluate(script, await_promise)
    }

    /// Evaluate JavaScript on a specific `tab_id` without requiring it to be active.
    pub(crate) fn evaluate_on_tab(
        &self,
        tab_id: &str,
        script: &str,
        await_promise: bool,
    ) -> Result<ScriptEvaluation> {
        self.backend.evaluate_on_tab(tab_id, script, await_promise)
    }

    /// Read CSS viewport metrics (width, height, DPR) for the active tab or optional `tab_id`.
    pub(crate) fn viewport_metrics(&self, tab_id: Option<&str>) -> Result<ViewportMetrics> {
        self.backend.viewport_metrics(tab_id)
    }

    /// Compile and run a [`BrowserCommand`] (probe or interaction) on the active page target.
    pub(crate) fn execute_command(&self, command: BrowserCommand) -> Result<BrowserCommandResult> {
        self.backend.execute_command(command)
    }

    /// Capture a PNG via the legacy full-page flag and return raw bytes (tests only).
    #[cfg(test)]
    pub(crate) fn capture_screenshot(&self, full_page: bool) -> Result<Vec<u8>> {
        let artifact =
            self.capture_screenshot_artifact(ScreenshotRequest::from_legacy_full_page(full_page))?;
        Ok(artifact.bytes().as_ref().to_vec())
    }

    /// Capture a screenshot and store it as a managed private artifact under the session root.
    #[allow(dead_code)]
    pub(crate) fn capture_screenshot_artifact(
        &self,
        request: ScreenshotRequest,
    ) -> Result<Arc<ScreenshotArtifact>> {
        let capture = self.backend.capture_screenshot_with_request(&request)?;
        self.store_screenshot_artifact(capture)
    }

    /// Capture a screenshot, store the managed artifact, and return both artifact and raw capture.
    pub(crate) fn capture_screenshot_artifact_with_capture(
        &self,
        request: ScreenshotRequest,
    ) -> Result<(Arc<ScreenshotArtifact>, ScreenshotCapture)> {
        let capture = self.backend.capture_screenshot_with_request(&request)?;
        let artifact = self.store_screenshot_artifact(capture.clone())?;
        Ok((artifact, capture))
    }

    /// Apply CDP device-metrics Viewport emulation and invalidate Snapshot cache.
    pub(crate) fn apply_viewport_emulation(
        &self,
        request: ViewportEmulationRequest,
    ) -> Result<ViewportOperationResult> {
        let result = self.backend.apply_viewport_emulation(&request)?;
        self.invalidate_snapshot_cache()?;
        Ok(result)
    }

    /// Clear Viewport emulation overrides and invalidate Snapshot cache.
    pub(crate) fn reset_viewport_emulation(
        &self,
        request: ViewportResetRequest,
    ) -> Result<ViewportOperationResult> {
        let result = self.backend.reset_viewport_emulation(&request)?;
        self.invalidate_snapshot_cache()?;
        Ok(result)
    }

    /// Dispatch a CDP key press to the active page target (no automatic cache invalidation).
    pub(crate) fn press_key(&self, key: &str) -> Result<()> {
        self.backend.press_key(key)
    }

    /// Navigate history back and wait for document settle; blocks unsafe URL schemes by default.
    pub fn go_back(&self) -> Result<()> {
        self.go_back_with_metrics(false).map(|_| ())
    }

    /// Navigate history forward and wait for document settle; blocks unsafe URL schemes by default.
    pub fn go_forward(&self) -> Result<()> {
        self.go_forward_with_metrics(false).map(|_| ())
    }

    /// Tear down the session: close backend tabs, clear screenshot artifacts, caches, and managed tabs.
    ///
    /// In attach mode this closes every backend-listed tab the backend `close` path targets; prefer
    /// [`Self::close_managed_tabs`] when only session-owned tabs should be removed.
    pub fn close(&self) -> Result<()> {
        self.backend.close()?;
        self.clear_screenshot_artifacts()?;
        self.invalidate_snapshot_cache()?;
        self.clear_managed_tabs()
    }

    /// Build a session around an existing backend, seeding managed tabs only in launch mode.
    fn from_backend_with_origin<B: SessionBackend + 'static>(
        backend: B,
        origin: SessionOrigin,
    ) -> Result<Self> {
        // Launch mode owns the process's initial tabs; attach mode starts with an empty managed set
        // so pre-existing user tabs are never closed as session-owned.
        let managed_tab_ids = match origin {
            SessionOrigin::Launched => backend
                .list_tabs()?
                .into_iter()
                .map(|tab| tab.id)
                .collect::<HashSet<_>>(),
            SessionOrigin::Connected => HashSet::new(),
        };

        let screenshot_artifact_root = tempfile::Builder::new()
            .prefix("chromewright-screenshots-")
            .tempdir()
            .map_err(|e| {
                BrowserError::ScreenshotFailed(format!(
                    "Failed to prepare screenshot artifact directory: {}",
                    e
                ))
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                screenshot_artifact_root.path(),
                std::fs::Permissions::from_mode(0o700),
            )
            .map_err(|e| {
                BrowserError::ScreenshotFailed(format!(
                    "Failed to secure screenshot artifact directory: {}",
                    e
                ))
            })?;
        }

        Ok(Self {
            backend: Arc::new(backend),
            origin,
            managed_tab_ids: Mutex::new(managed_tab_ids),
            tool_registry: ToolRegistry::with_all_tools(),
            markdown_cache: Mutex::new(None),
            snapshot_cache: Mutex::new(None),
            screenshot_artifacts: Mutex::new(VecDeque::new()),
            screenshot_artifact_root,
        })
    }

    /// Return whether this session is launch mode or attach mode.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn session_origin(&self) -> SessionOrigin {
        self.origin
    }

    /// True when this session attached to an existing DevTools endpoint (attach mode).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_connected_session(&self) -> bool {
        self.origin == SessionOrigin::Connected
    }

    /// True when `tab_id` is among the session-owned managed tabs.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_tab_managed(&self, tab_id: &str) -> Result<bool> {
        Ok(self.managed_tab_ids()?.contains(tab_id))
    }

    /// Stable wire label for session origin (`"launched"` or `"connected"`).
    pub(crate) fn session_origin_label(&self) -> &'static str {
        match self.origin {
            SessionOrigin::Launched => "launched",
            SessionOrigin::Connected => "connected",
        }
    }

    /// Record a newly opened tab as session-owned (attach-mode ownership boundary).
    pub(crate) fn remember_managed_tab(&self, tab_id: impl Into<String>) -> Result<()> {
        self.managed_tab_ids()?.insert(tab_id.into());
        Ok(())
    }

    /// Drop managed ownership for a closed or released tab without closing it.
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

    /// Wrap a test SessionBackend as a launch-mode BrowserSession.
    #[cfg(test)]
    pub(crate) fn with_test_backend<B: SessionBackend + 'static>(backend: B) -> Self {
        Self::from_backend_with_origin(backend, SessionOrigin::Launched)
            .expect("test backend should construct")
    }

    /// Wrap a test SessionBackend with an explicit launch/attach origin.
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

    #[cfg(test)]
    pub(crate) fn screenshot_artifact_root_for_test(&self) -> std::path::PathBuf {
        self.screenshot_artifact_root
            .path()
            .canonicalize()
            .unwrap_or_else(|_| self.screenshot_artifact_root.path().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::launch_error_is_environmental;
    use crate::browser::{ScreenshotMode, ScreenshotRequest};
    use crate::contract::{
        ViewportEmulation, ViewportEmulationRequest, ViewportOrientation, ViewportResetRequest,
    };
    use crate::dom::SnapshotNode;
    use serde_json::json;
    use std::ffi::OsStr;
    use std::sync::Arc;

    #[test]
    fn seed_startup_urls_reuses_launched_initial_tab_then_activates_last_url() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let urls = vec![
            "example.com".to_string(),
            "https://second.example/path".to_string(),
        ];

        let seeded = session
            .seed_startup_urls(&urls)
            .expect("startup URLs should seed launched session");

        assert_eq!(seeded.len(), 2);
        assert_eq!(seeded[0].id, "tab-1");
        assert_eq!(seeded[0].url, "https://example.com");
        assert_eq!(seeded[1].id, "tab-2");
        assert_eq!(seeded[1].url, "https://second.example/path");
        assert!(seeded[1].active);

        let tabs = session.list_tabs().expect("list seeded tabs");
        assert_eq!(tabs.len(), 2, "launch should not retain a blank seed tab");
        assert!(tabs[1].active, "final seeded URL should be active");
    }

    #[test]
    fn seed_startup_urls_preserves_connected_tabs_and_marks_new_tabs_managed() {
        let session = BrowserSession::with_test_backend_origin(
            FakeSessionBackend::new(),
            SessionOrigin::Connected,
        );
        let existing = session.list_tabs().expect("list existing tab");
        let urls = vec![
            "https://first.example".to_string(),
            "https://second.example".to_string(),
        ];

        let seeded = session
            .seed_startup_urls(&urls)
            .expect("startup URLs should seed connected session");

        assert_eq!(seeded.len(), 2);
        let tabs = session.list_tabs().expect("list seeded tabs");
        assert_eq!(tabs.len(), existing.len() + 2);
        assert_eq!(tabs[0].id, existing[0].id, "existing tab id must stay put");
        assert_eq!(
            tabs[0].url, existing[0].url,
            "existing tab URL must stay put"
        );
        assert!(tabs.last().expect("last tab").active);
        assert!(
            session
                .is_tab_managed(&seeded[0].id)
                .expect("managed tab state")
        );
        assert!(
            session
                .is_tab_managed(&seeded[1].id)
                .expect("managed tab state")
        );
    }

    #[test]
    fn seed_startup_urls_rejects_all_unsafe_urls_before_mutating_tabs() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let before = session.list_tabs().expect("list initial tab");
        let urls = vec![
            "https://safe.example".to_string(),
            "/relative/without-an-origin".to_string(),
        ];

        let error = session
            .seed_startup_urls(&urls)
            .expect_err("unsafe initial URL should fail before startup mutation");

        assert!(error.to_string().contains("must be an absolute"));
        assert_eq!(
            session.list_tabs().expect("list tabs after failure"),
            before
        );
    }

    #[test]
    fn seed_startup_urls_rolls_back_previously_opened_tabs_after_operational_failure() {
        let session = BrowserSession::with_test_backend_origin(
            FakeSessionBackend::with_open_failures(["https://broken.example"]),
            SessionOrigin::Connected,
        );
        let before = session
            .list_tabs()
            .expect("list attached tabs before seeding");

        let error = session
            .seed_startup_urls(&[
                "https://first.example".to_string(),
                "https://broken.example".to_string(),
            ])
            .expect_err("the scripted second URL should fail");

        assert!(error.to_string().contains("broken.example"));
        assert_eq!(
            session.list_tabs().expect("list tabs after rollback"),
            before,
            "a failed startup seed must not leave its earlier tab open"
        );
        assert!(
            !session
                .is_tab_managed("tab-2")
                .expect("managed tab state should be available"),
            "rollback must release managed ownership too"
        );
    }

    #[test]
    fn seed_startup_urls_reports_failed_rollback_and_retains_managed_ownership() {
        let session = BrowserSession::with_test_backend_origin(
            FakeSessionBackend::with_open_and_close_failures(
                ["https://first.example"],
                ["https://broken.example"],
            ),
            SessionOrigin::Connected,
        );

        let error = session
            .seed_startup_urls(&[
                "https://first.example".to_string(),
                "https://broken.example".to_string(),
            ])
            .expect_err("the scripted second URL should fail");

        assert!(error.to_string().contains("rollback also failed"));
        assert!(
            session
                .is_tab_managed("tab-2")
                .expect("managed tab state should be available"),
            "a tab left open after rollback failure must remain session-managed"
        );
        assert!(
            session
                .list_tabs()
                .expect("list tabs after rollback failure")
                .iter()
                .any(|tab| tab.id == "tab-2"),
            "a rollback close failure leaves the tab open for normal managed cleanup"
        );
    }

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

    fn read_viewport_metrics(session: &BrowserSession, tab_id: Option<&str>) -> (f64, f64, f64) {
        let metrics = session
            .viewport_metrics(tab_id)
            .expect("viewport metrics should be readable");

        (metrics.width, metrics.height, metrics.device_pixel_ratio)
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
                allow_large_viewport: false,
            })
            .expect("viewport emulation should succeed");

        assert_eq!(result.tab_id, "tab-1");
        assert_eq!(result.viewport_after.width, 375.0);
        assert_eq!(result.viewport_after.height, 812.0);
        assert_eq!(result.viewport_after.device_pixel_ratio, 2.0);
        assert_eq!(
            result.emulation,
            Some(ViewportEmulation {
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
        assert_eq!(read_viewport_metrics(&session, None), (375.0, 812.0, 2.0));
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
                allow_large_viewport: false,
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
            (640.0, 360.0, 1.5)
        );
        assert_eq!(
            read_viewport_metrics(&session, Some(&second_tab_id)),
            (800.0, 600.0, 2.0)
        );
    }

    #[test]
    fn test_fake_backend_viewport_metrics_do_not_depend_on_script_matching() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        session
            .apply_viewport_emulation(ViewportEmulationRequest {
                width: 500,
                height: 400,
                device_scale_factor: 1.25,
                mobile: false,
                touch: false,
                orientation: None,
                tab_id: None,
                allow_large_viewport: false,
            })
            .expect("viewport emulation should succeed");

        assert_eq!(read_viewport_metrics(&session, None), (500.0, 400.0, 1.25));

        let evaluation = session.evaluate(
            r#"(() => [window.innerWidth, window.innerHeight, window.devicePixelRatio || 1])()"#,
            false,
        );
        assert!(
            matches!(evaluation, Err(BrowserError::EvaluationFailed(_))),
            "fake viewport metrics should be exposed by the typed backend method, not viewport JS string matching"
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
                allow_large_viewport: false,
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
        assert_eq!(read_viewport_metrics(&session, None), (800.0, 600.0, 2.0));
    }

    #[test]
    fn test_apply_viewport_emulation_rejects_invalid_requests_without_mutation() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        let oversize = session.apply_viewport_emulation(ViewportEmulationRequest {
            width: 10_001,
            height: 600,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
            orientation: None,
            tab_id: None,
            allow_large_viewport: false,
        });
        assert!(matches!(oversize, Err(BrowserError::InvalidArgument(_))));
        assert_eq!(read_viewport_metrics(&session, None), (800.0, 600.0, 2.0));

        session
            .apply_viewport_emulation(ViewportEmulationRequest {
                width: 10_001,
                height: 600,
                device_scale_factor: 1.0,
                mobile: false,
                touch: false,
                orientation: None,
                tab_id: None,
                allow_large_viewport: true,
            })
            .expect("intentional large viewport override should succeed");
        assert_eq!(read_viewport_metrics(&session, None), (10001.0, 600.0, 1.0));
        session
            .reset_viewport_emulation(ViewportResetRequest::default())
            .expect("viewport reset after large override should succeed");
        assert_eq!(read_viewport_metrics(&session, None), (800.0, 600.0, 2.0));

        let empty_tab_id = session.apply_viewport_emulation(ViewportEmulationRequest {
            width: 320,
            height: 640,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
            orientation: None,
            tab_id: Some("   ".to_string()),
            allow_large_viewport: false,
        });
        assert!(matches!(
            empty_tab_id,
            Err(BrowserError::InvalidArgument(_))
        ));
        assert_eq!(read_viewport_metrics(&session, None), (800.0, 600.0, 2.0));

        let unknown_tab = session.apply_viewport_emulation(ViewportEmulationRequest {
            width: 320,
            height: 640,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
            orientation: None,
            tab_id: Some("missing-tab".to_string()),
            allow_large_viewport: false,
        });
        assert!(matches!(
            unknown_tab,
            Err(BrowserError::TabOperationFailed(_))
        ));
        assert_eq!(read_viewport_metrics(&session, None), (800.0, 600.0, 2.0));
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
                allow_large_viewport: false,
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
