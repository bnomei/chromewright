use crate::browser::{ConnectionOptions, LaunchOptions};
use crate::dom::{DocumentMetadata, DomTree};
use crate::error::{BrowserError, Result};
use headless_chrome::{Browser, Tab};
use serde_json::Value;
use std::ffi::OsStr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub(crate) const DEBUG_PORT_START: u16 = 40_000;
pub(crate) const DEBUG_PORT_END: u16 = 59_999;
static DEBUG_PORT_COUNTER: AtomicU16 = AtomicU16::new(DEBUG_PORT_START);

fn session_close_result(total_tabs: usize, failures: Vec<String>) -> Result<()> {
    if failures.is_empty() {
        return Ok(());
    }

    Err(BrowserError::TabOperationFailed(format!(
        "Session close encountered {} error(s) after attempting {} tab(s): {}",
        failures.len(),
        total_tabs,
        failures.join("; ")
    )))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabDescriptor {
    pub id: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ScriptEvaluation {
    pub value: Option<Value>,
    pub description: Option<String>,
    pub type_name: Option<String>,
}

pub(crate) trait SessionBackend: Send + Sync {
    fn navigate(&self, url: &str) -> Result<()>;
    fn wait_for_navigation(&self) -> Result<()>;
    fn wait_for_document_ready_with_timeout(&self, timeout: Duration) -> Result<()>;
    fn document_metadata(&self) -> Result<DocumentMetadata>;
    fn extract_dom(&self) -> Result<DomTree>;
    fn extract_dom_with_prefix(&self, prefix: &str) -> Result<DomTree>;
    fn evaluate(&self, script: &str, await_promise: bool) -> Result<ScriptEvaluation>;
    fn capture_screenshot(&self, full_page: bool) -> Result<Vec<u8>>;
    fn press_key(&self, key: &str) -> Result<()>;
    fn list_tabs(&self) -> Result<Vec<TabDescriptor>>;
    fn active_tab(&self) -> Result<TabDescriptor>;
    fn open_tab(&self, url: &str) -> Result<TabDescriptor>;
    fn activate_tab(&self, tab_id: &str) -> Result<()>;
    fn close_tab(&self, tab_id: &str, with_unload: bool) -> Result<()>;
    fn close(&self) -> Result<()>;
}

pub(crate) fn choose_debug_port() -> u16 {
    let span = DEBUG_PORT_END - DEBUG_PORT_START + 1;
    let offset = DEBUG_PORT_COUNTER.fetch_add(1, Ordering::Relaxed) % span;
    DEBUG_PORT_START + offset
}

pub(crate) fn build_launch_options(
    options: LaunchOptions,
) -> headless_chrome::LaunchOptions<'static> {
    let mut launch_opts = headless_chrome::LaunchOptions::default();

    launch_opts
        .ignore_default_args
        .push(OsStr::new("--enable-automation"));
    launch_opts
        .args
        .push(OsStr::new("--disable-blink-features=AutomationControlled"));

    launch_opts.idle_browser_timeout = Duration::from_secs(60 * 60);
    launch_opts.headless = options.headless;
    launch_opts.window_size = Some((options.window_width, options.window_height));
    launch_opts.port = Some(options.debug_port.unwrap_or_else(choose_debug_port));
    launch_opts.sandbox = options.sandbox;

    if let Some(path) = options.chrome_path {
        launch_opts.path = Some(path);
    }

    if let Some(dir) = options.user_data_dir {
        launch_opts.user_data_dir = Some(dir);
    }

    launch_opts
}

pub(crate) struct ChromeSessionBackend {
    browser: Browser,
    active_tab_hint: RwLock<Option<String>>,
}

impl ChromeSessionBackend {
    pub(crate) fn launch(options: LaunchOptions) -> Result<Self> {
        let launch_opts = build_launch_options(options);
        let browser =
            Browser::new(launch_opts).map_err(|e| BrowserError::LaunchFailed(e.to_string()))?;
        let initial_tab = browser
            .new_tab()
            .map_err(|e| BrowserError::LaunchFailed(format!("Failed to create tab: {}", e)))?;

        Ok(Self {
            browser,
            active_tab_hint: RwLock::new(Some(tab_id(&initial_tab))),
        })
    }

    pub(crate) fn connect(options: ConnectionOptions) -> Result<Self> {
        let ws_url = options.resolved_ws_url()?;
        let browser =
            Browser::connect(ws_url).map_err(|e| BrowserError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            browser,
            active_tab_hint: RwLock::new(None),
        })
    }

    pub(crate) fn active_tab_handle(&self) -> Result<Arc<Tab>> {
        if let Some(tab) = self.cached_active_tab()? {
            return Ok(tab);
        }

        let tabs = self.tabs()?;

        for tab in &tabs {
            let result = tab.evaluate(
                "document.visibilityState === 'visible' && document.hasFocus()",
                false,
            );
            match result {
                Ok(remote_object) => {
                    if remote_object
                        .value
                        .as_ref()
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                    {
                        self.set_active_tab_hint(Some(tab_id(tab)))?;
                        return Ok(tab.clone());
                    }
                }
                Err(e) => {
                    log::debug!("Failed to check tab status: {}", e);
                }
            }
        }

        for tab in &tabs {
            let result = tab.evaluate("document.visibilityState === 'visible'", false);
            match result {
                Ok(remote_object) => {
                    if remote_object
                        .value
                        .as_ref()
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                    {
                        self.set_active_tab_hint(Some(tab_id(tab)))?;
                        return Ok(tab.clone());
                    }
                }
                Err(_) => continue,
            }
        }

        Err(BrowserError::TabOperationFailed(
            "No active tab found".to_string(),
        ))
    }

    pub(crate) fn tabs(&self) -> Result<Vec<Arc<Tab>>> {
        let tabs = self
            .browser
            .get_tabs()
            .lock()
            .map_err(|e| BrowserError::TabOperationFailed(format!("Failed to get tabs: {}", e)))?
            .clone();
        Ok(tabs)
    }

    pub(crate) fn activate_real_tab(&self, tab: &Arc<Tab>) -> Result<()> {
        tab.activate().map_err(|e| {
            BrowserError::TabOperationFailed(format!("Failed to activate tab: {}", e))
        })?;
        self.set_active_tab_hint(Some(tab_id(tab)))?;
        Ok(())
    }

    pub(crate) fn open_real_tab(&self, url: &str) -> Result<Arc<Tab>> {
        let tab = self.browser.new_tab().map_err(|e| {
            BrowserError::TabOperationFailed(format!("Failed to create tab: {}", e))
        })?;

        tab.navigate_to(url).map_err(|e| {
            BrowserError::NavigationFailed(format!("Failed to navigate to {}: {}", url, e))
        })?;

        tab.wait_until_navigated().map_err(|e| {
            BrowserError::NavigationFailed(format!("Navigation to {} did not complete: {}", url, e))
        })?;

        self.activate_real_tab(&tab)?;
        Ok(tab)
    }

    fn cached_active_tab(&self) -> Result<Option<Arc<Tab>>> {
        let Some(tab_id_hint) = self.active_tab_hint()? else {
            return Ok(None);
        };

        Ok(self
            .tabs()?
            .into_iter()
            .find(|tab| tab_id(tab) == tab_id_hint))
    }

    fn active_tab_hint(&self) -> Result<Option<String>> {
        Ok(self
            .active_tab_hint
            .read()
            .map_err(|e| {
                BrowserError::TabOperationFailed(format!("Failed to read active tab hint: {}", e))
            })?
            .clone())
    }

    fn set_active_tab_hint(&self, tab_id: Option<String>) -> Result<()> {
        *self.active_tab_hint.write().map_err(|e| {
            BrowserError::TabOperationFailed(format!("Failed to write active tab hint: {}", e))
        })? = tab_id;
        Ok(())
    }

    fn document_ready_state_for_tab(&self, tab: &Arc<Tab>) -> Result<String> {
        let result = tab.evaluate("document.readyState", false).map_err(|e| {
            BrowserError::NavigationFailed(format!("Failed to read readyState: {}", e))
        })?;

        result
            .value
            .and_then(|value| value.as_str().map(str::to_string))
            .ok_or_else(|| {
                BrowserError::NavigationFailed(
                    "Browser did not return a document.readyState value".to_string(),
                )
            })
    }

    fn wait_for_document_ready_with_tab(&self, tab: &Arc<Tab>, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        loop {
            let ready_state = self.document_ready_state_for_tab(tab)?;
            if ready_state == "complete" {
                return Ok(());
            }

            if start.elapsed() >= timeout {
                return Err(BrowserError::Timeout(format!(
                    "Document did not reach readyState=complete within {} ms",
                    timeout.as_millis()
                )));
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl SessionBackend for ChromeSessionBackend {
    fn navigate(&self, url: &str) -> Result<()> {
        self.active_tab_handle()?.navigate_to(url).map_err(|e| {
            BrowserError::NavigationFailed(format!("Failed to navigate to {}: {}", url, e))
        })?;
        Ok(())
    }

    fn wait_for_navigation(&self) -> Result<()> {
        let tab = self.active_tab_handle()?;
        tab.wait_until_navigated()
            .map_err(|e| BrowserError::NavigationFailed(format!("Navigation timeout: {}", e)))?;
        self.wait_for_document_ready_with_tab(&tab, Duration::from_secs(30))
    }

    fn wait_for_document_ready_with_timeout(&self, timeout: Duration) -> Result<()> {
        let tab = self.active_tab_handle()?;
        self.wait_for_document_ready_with_tab(&tab, timeout)
    }

    fn document_metadata(&self) -> Result<DocumentMetadata> {
        let tab = self.active_tab_handle()?;
        DocumentMetadata::from_tab(&tab)
    }

    fn extract_dom(&self) -> Result<DomTree> {
        let tab = self.active_tab_handle()?;
        DomTree::from_tab(&tab)
    }

    fn extract_dom_with_prefix(&self, prefix: &str) -> Result<DomTree> {
        let tab = self.active_tab_handle()?;
        DomTree::from_tab_with_prefix(&tab, prefix)
    }

    fn evaluate(&self, script: &str, await_promise: bool) -> Result<ScriptEvaluation> {
        let result = self
            .active_tab_handle()?
            .evaluate(script, await_promise)
            .map_err(|e| BrowserError::EvaluationFailed(e.to_string()))?;

        Ok(ScriptEvaluation {
            value: result.value,
            description: result.description,
            type_name: Some(format!("{:?}", result.Type)),
        })
    }

    fn capture_screenshot(&self, full_page: bool) -> Result<Vec<u8>> {
        self.active_tab_handle()?
            .capture_screenshot(
                headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                None,
                None,
                full_page,
            )
            .map_err(|e| BrowserError::ScreenshotFailed(e.to_string()))
    }

    fn press_key(&self, key: &str) -> Result<()> {
        self.active_tab_handle()?
            .press_key(key)
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "press_key".to_string(),
                reason: e.to_string(),
            })
            .map(|_| ())
    }

    fn list_tabs(&self) -> Result<Vec<TabDescriptor>> {
        Ok(self.tabs()?.iter().map(descriptor_for_tab).collect())
    }

    fn active_tab(&self) -> Result<TabDescriptor> {
        Ok(descriptor_for_tab(&self.active_tab_handle()?))
    }

    fn open_tab(&self, url: &str) -> Result<TabDescriptor> {
        Ok(descriptor_for_tab(&self.open_real_tab(url)?))
    }

    fn activate_tab(&self, target_tab_id: &str) -> Result<()> {
        let tab = self
            .tabs()?
            .into_iter()
            .find(|tab| tab_id(tab) == target_tab_id)
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(format!("No tab found for id {}", target_tab_id))
            })?;
        self.activate_real_tab(&tab)
    }

    fn close_tab(&self, target_tab_id: &str, with_unload: bool) -> Result<()> {
        let tab = self
            .tabs()?
            .into_iter()
            .find(|tab| tab_id(tab) == target_tab_id)
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(format!("No tab found for id {}", target_tab_id))
            })?;

        if self.active_tab_hint()?.as_deref() == Some(target_tab_id) {
            self.set_active_tab_hint(None)?;
        }

        tab.close(with_unload)
            .map_err(|e| BrowserError::TabOperationFailed(format!("Failed to close tab: {}", e)))
            .map(|_| ())
    }

    fn close(&self) -> Result<()> {
        let tabs = self.tabs()?;
        let total_tabs = tabs.len();
        let mut failures = Vec::new();

        for tab in tabs {
            let descriptor = descriptor_for_tab(&tab);
            if let Err(err) = tab.close(false) {
                failures.push(format!(
                    "failed to close '{}' ({}) [id={}]: {}",
                    descriptor.title, descriptor.url, descriptor.id, err
                ));
            }
        }

        if let Err(err) = self.set_active_tab_hint(None) {
            failures.push(format!("failed to clear active tab hint: {}", err));
        }

        session_close_result(total_tabs, failures)
    }
}

fn tab_id(tab: &Arc<Tab>) -> String {
    tab.get_target_id().to_string()
}

fn descriptor_for_tab(tab: &Arc<Tab>) -> TabDescriptor {
    TabDescriptor {
        id: tab_id(tab),
        title: tab.get_title().unwrap_or_default(),
        url: tab.get_url(),
    }
}

#[cfg(test)]
mod fake;

#[cfg(test)]
pub(crate) use fake::FakeSessionBackend;
