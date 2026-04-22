use crate::browser::config::{ConnectionOptions, LaunchOptions};
#[cfg(test)]
use crate::dom::{AriaChild, AriaNode};
use crate::dom::{DocumentMetadata, DomTree};
use crate::error::{BrowserError, Result};
use headless_chrome::{Browser, Tab};
use serde_json::Value;
use std::any::Any;
use std::ffi::OsStr;
#[cfg(test)]
use std::sync::Mutex;
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

pub(crate) trait SessionBackend: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
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
        let browser = Browser::connect(options.ws_url)
            .map_err(|e| BrowserError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            browser,
            active_tab_hint: RwLock::new(None),
        })
    }

    pub(crate) fn browser(&self) -> &Browser {
        &self.browser
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

    pub(crate) fn create_tab_handle(&self) -> Result<Arc<Tab>> {
        let tab = self.browser.new_tab().map_err(|e| {
            BrowserError::TabOperationFailed(format!("Failed to create tab: {}", e))
        })?;
        self.set_active_tab_hint(Some(tab_id(&tab)))?;
        Ok(tab)
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
    fn as_any(&self) -> &dyn Any {
        self
    }

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
pub(crate) struct FakeSessionBackend {
    state: Mutex<FakeState>,
    close_failure_urls: Vec<String>,
}

#[cfg(test)]
#[derive(Debug)]
struct FakeState {
    tabs: Vec<TabDescriptor>,
    active_tab_id: Option<String>,
    next_tab_id: usize,
    revision: usize,
}

#[cfg(test)]
impl FakeSessionBackend {
    pub(crate) fn new() -> Self {
        Self::with_close_failures(std::iter::empty::<String>())
    }

    pub(crate) fn with_no_active_tab() -> Self {
        let backend = Self::new();
        backend
            .state
            .lock()
            .expect("fake backend state should be writable")
            .active_tab_id = None;
        backend
    }

    pub(crate) fn with_close_failures<I, S>(close_failure_urls: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let initial_tab = TabDescriptor {
            id: "tab-1".to_string(),
            title: "about:blank".to_string(),
            url: "about:blank".to_string(),
        };

        Self {
            state: Mutex::new(FakeState {
                tabs: vec![initial_tab.clone()],
                active_tab_id: Some(initial_tab.id),
                next_tab_id: 2,
                revision: 1,
            }),
            close_failure_urls: close_failure_urls.into_iter().map(Into::into).collect(),
        }
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, FakeState>> {
        self.state.lock().map_err(|e| {
            BrowserError::TabOperationFailed(format!("Failed to access fake backend state: {}", e))
        })
    }

    fn active_tab_from_state(state: &FakeState) -> Result<&TabDescriptor> {
        let active_id = state
            .active_tab_id
            .as_deref()
            .ok_or_else(|| BrowserError::TabOperationFailed("No active tab found".to_string()))?;

        state
            .tabs
            .iter()
            .find(|tab| tab.id == active_id)
            .ok_or_else(|| BrowserError::TabOperationFailed("No active tab found".to_string()))
    }

    fn bump_revision(state: &mut FakeState) {
        state.revision += 1;
    }

    fn title_for_url(url: &str) -> String {
        url.to_string()
    }

    fn current_document(state: &FakeState) -> Result<DocumentMetadata> {
        let active = Self::active_tab_from_state(state)?;
        Ok(DocumentMetadata {
            document_id: active.id.clone(),
            revision: format!("fake:{}", state.revision),
            url: active.url.clone(),
            title: active.title.clone(),
            ready_state: "complete".to_string(),
            frames: Vec::new(),
        })
    }

    fn fake_dom(document: &DocumentMetadata) -> DomTree {
        let mut root = AriaNode::fragment();
        let mut fake_target = AriaNode::new("button", "Fake target")
            .with_index(0)
            .with_box(true, Some("pointer".to_string()));
        fake_target.active = Some(true);
        root.children.push(AriaChild::Node(Box::new(fake_target)));

        let mut dom = DomTree::new(root);
        dom.document = document.clone();
        dom.replace_selectors(vec!["#fake-target".to_string()]);
        dom
    }

    fn embedded_config(script: &str) -> Option<Value> {
        let needle = "const config = ";
        let start = script.find(needle)? + needle.len();
        let rest = &script[start..];
        let end = rest.find(';')?;
        serde_json::from_str(rest[..end].trim()).ok()
    }

    fn extract_selector(script: &str) -> Option<Option<String>> {
        let needle = "const selector = ";
        let start = script.find(needle)? + needle.len();
        let suffix = "\n            const element = ";
        let end = start + script[start..].find(suffix)?;
        let raw = script[start..end].trim().trim_end_matches(';').trim();

        if raw == "null" {
            return Some(None);
        }

        serde_json::from_str(raw).ok().map(Some)
    }

    fn extract_content_for_selector(selector: Option<&str>, html: bool) -> Result<String> {
        const FAKE_TARGET_SELECTOR: &str = "#fake-target";
        const FAKE_TARGET_TEXT: &str = "Fake target";
        const FAKE_TARGET_HTML: &str =
            r#"<button id="fake-target" class="fake">Fake target</button>"#;

        match selector {
            None | Some(FAKE_TARGET_SELECTOR) => Ok(if html {
                FAKE_TARGET_HTML.to_string()
            } else {
                FAKE_TARGET_TEXT.to_string()
            }),
            Some(selector) => Err(BrowserError::EvaluationFailed(format!(
                "Element not found: {}",
                selector
            ))),
        }
    }

    fn scripted_actionability(script: &str) -> Option<ScriptEvaluation> {
        if !script.contains("\"predicates\"") {
            return None;
        }

        let config = Self::embedded_config(script)?;
        let mut payload = serde_json::json!({
            "present": true,
            "frame_depth": 0,
            "diagnostics": {
                "pointer_events": "auto",
                "hit_target": {
                    "tag": "button",
                    "id": "fake-target",
                    "classes": []
                },
                "text_length": 11,
                "has_value": false
            }
        });

        for predicate in config["predicates"].as_array().into_iter().flatten() {
            match predicate.as_str() {
                Some("visible") => payload["visible"] = serde_json::json!(true),
                Some("enabled") => payload["enabled"] = serde_json::json!(true),
                Some("editable") => payload["editable"] = serde_json::json!(true),
                Some("stable") => payload["stable"] = serde_json::json!(true),
                Some("receives_events") => payload["receives_events"] = serde_json::json!(true),
                Some("in_viewport") => payload["in_viewport"] = serde_json::json!(true),
                Some("unobscured_center") => payload["unobscured_center"] = serde_json::json!(true),
                Some("text_contains") => payload["text_contains"] = serde_json::json!(true),
                Some("value_equals") => payload["value_equals"] = serde_json::json!(true),
                _ => {}
            }
        }

        Some(ScriptEvaluation {
            value: Some(Value::String(payload.to_string())),
            description: None,
            type_name: Some("String".to_string()),
        })
    }

    fn scripted_result(&self, script: &str) -> Option<Result<ScriptEvaluation>> {
        if script.contains("document.readyState") {
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String("complete".to_string())),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("window.history.back()") || script.contains("window.history.forward()") {
            return Some(Ok(ScriptEvaluation {
                value: Some(serde_json::json!(true)),
                description: None,
                type_name: Some("Boolean".to_string()),
            }));
        }

        if let Some(result) = Self::scripted_actionability(script) {
            return Some(Ok(result));
        }

        if script.contains("selectorExistsAcrossScopes") && script.contains("\"present\"") {
            let present = Self::embedded_config(script)
                .and_then(|config| {
                    config["selector"]
                        .as_str()
                        .map(|selector| !selector.is_empty())
                })
                .unwrap_or(false);
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String(
                    serde_json::json!({ "present": present }).to_string(),
                )),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("window.scrollBy") && script.contains("actualScroll") {
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String(
                    serde_json::json!({
                        "actualScroll": 0,
                        "isAtBottom": true,
                        "scrollY": 0,
                        "isAtTop": true
                    })
                    .to_string(),
                )),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("getDeepestActiveElement(document)") {
            return Some(Ok(ScriptEvaluation {
                value: Some(serde_json::json!({
                    "tag": "button",
                    "role": "button",
                    "name": "Fake target"
                })),
                description: None,
                type_name: Some("Object".to_string()),
            }));
        }

        if script.contains("querySelectorAll('a[href]')") {
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String("[]".to_string())),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("document.querySelector(selector)")
            && script.contains("const selector = ")
            && (script.contains("element ? element.innerHTML : ''")
                || script
                    .contains("element ? (element.innerText || element.textContent || '') : ''"))
        {
            let selector = Self::extract_selector(script)?;
            let content = Self::extract_content_for_selector(
                selector.as_deref(),
                script.contains("element ? element.innerHTML : ''"),
            );

            return Some(content.map(|content| ScriptEvaluation {
                value: Some(Value::String(content)),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("document.body.innerHTML") || script.contains("document.body.innerText")
        {
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String(String::new())),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("READABILITY_SCRIPT") && script.contains("readability_failed") {
            let url = self
                .active_tab()
                .map(|tab| tab.url)
                .unwrap_or_else(|_| "about:blank".to_string());
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String(
                    serde_json::json!({
                        "title": "Fake target",
                        "content": "<main><p>Fake content</p></main>",
                        "textContent": "Fake content",
                        "url": url,
                        "excerpt": "",
                        "byline": "",
                        "siteName": "",
                        "length": 12,
                        "lang": "en",
                        "dir": "ltr",
                        "publishedTime": "",
                        "readability_failed": false,
                        "error": null
                    })
                    .to_string(),
                )),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("document.body && document.body.textContent") {
            return Some(Ok(ScriptEvaluation {
                value: Some(serde_json::json!(0)),
                description: None,
                type_name: Some("Number".to_string()),
            }));
        }

        if script.contains("const config = ")
            && script.contains("resolveTargetElement(config)")
            && script.contains("element.click();")
        {
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String(
                    serde_json::json!({ "success": true }).to_string(),
                )),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("const config = ")
            && script.contains("resolveTargetElement(config)")
            && script.contains("Element does not accept text input")
        {
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String(
                    serde_json::json!({
                        "success": true,
                        "value": "fake text"
                    })
                    .to_string(),
                )),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("MouseEvent(\"mouseover\"") {
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String(
                    serde_json::json!({
                        "success": true,
                        "tagName": "BUTTON",
                        "id": "fake-target",
                        "className": "fake"
                    })
                    .to_string(),
                )),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("selectedText") && script.contains("Element is not a SELECT element") {
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String(
                    serde_json::json!({
                        "success": true,
                        "selectedValue": "fake-value",
                        "selectedText": "Fake option"
                    })
                    .to_string(),
                )),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        if script.contains("inspectElement(element, frameDepth, actionableIndex)") {
            let url = self
                .active_tab()
                .map(|tab| tab.url)
                .unwrap_or_else(|_| "about:blank".to_string());
            let incomplete_payload = Self::embedded_config(script)
                .and_then(|config| config["style_names"].as_array().cloned())
                .map(|style_names| {
                    style_names
                        .iter()
                        .any(|name| name.as_str() == Some("__incomplete_payload__"))
                })
                .unwrap_or(false);
            if incomplete_payload {
                return Some(Ok(ScriptEvaluation {
                    value: Some(Value::String(
                        serde_json::json!({
                            "success": true,
                            "actionable_index": 0
                        })
                        .to_string(),
                    )),
                    description: None,
                    type_name: Some("String".to_string()),
                }));
            }
            return Some(Ok(ScriptEvaluation {
                value: Some(Value::String(
                    serde_json::json!({
                        "success": true,
                        "identity": {
                            "tag": "button",
                            "id": "fake-target",
                            "classes": ["fake"]
                        },
                        "accessibility": {
                            "role": "button",
                            "name": "Fake target",
                            "active": true,
                            "checked": null,
                            "disabled": false,
                            "expanded": null,
                            "pressed": null,
                            "selected": null
                        },
                        "form_state": {
                            "value": null,
                            "placeholder": null,
                            "readonly": null,
                            "disabled": false
                        },
                        "layout": {
                            "bounding_box": {
                                "x": 0.0,
                                "y": 0.0,
                                "width": 100.0,
                                "height": 32.0
                            },
                            "visible": true,
                            "visible_in_viewport": true,
                            "receives_pointer_events": true,
                            "pointer_events": "auto",
                            "cursor": "pointer"
                        },
                        "context": {
                            "document_url": url,
                            "frame_depth": 0,
                            "inside_shadow_root": false
                        },
                        "actionable_index": 0,
                        "boundary": null,
                        "sections": null
                    })
                    .to_string(),
                )),
                description: None,
                type_name: Some("String".to_string()),
            }));
        }

        None
    }
}

#[cfg(test)]
impl Default for FakeSessionBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl SessionBackend for FakeSessionBackend {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn navigate(&self, url: &str) -> Result<()> {
        let mut state = self.lock_state()?;
        let active_id = state
            .active_tab_id
            .clone()
            .ok_or_else(|| BrowserError::NavigationFailed("No active tab available".to_string()))?;
        let tab = state
            .tabs
            .iter_mut()
            .find(|tab| tab.id == active_id)
            .ok_or_else(|| BrowserError::NavigationFailed("No active tab available".to_string()))?;
        tab.url = url.to_string();
        tab.title = Self::title_for_url(url);
        Self::bump_revision(&mut state);
        Ok(())
    }

    fn wait_for_navigation(&self) -> Result<()> {
        Ok(())
    }

    fn wait_for_document_ready_with_timeout(&self, _timeout: Duration) -> Result<()> {
        Ok(())
    }

    fn document_metadata(&self) -> Result<DocumentMetadata> {
        let state = self.lock_state()?;
        Self::current_document(&state)
    }

    fn extract_dom(&self) -> Result<DomTree> {
        let state = self.lock_state()?;
        Ok(Self::fake_dom(&Self::current_document(&state)?))
    }

    fn extract_dom_with_prefix(&self, _prefix: &str) -> Result<DomTree> {
        self.extract_dom()
    }

    fn evaluate(&self, script: &str, _await_promise: bool) -> Result<ScriptEvaluation> {
        self.scripted_result(script).unwrap_or_else(|| {
            Err(BrowserError::EvaluationFailed(
                "Fake backend does not support this JavaScript payload yet".to_string(),
            ))
        })
    }

    fn capture_screenshot(&self, _full_page: bool) -> Result<Vec<u8>> {
        Ok(vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 255,
            255, 255, 127, 0, 9, 251, 3, 253, 160, 114, 168, 187, 0, 0, 0, 0, 73, 69, 78, 68, 174,
            66, 96, 130,
        ])
    }

    fn press_key(&self, _key: &str) -> Result<()> {
        Ok(())
    }

    fn list_tabs(&self) -> Result<Vec<TabDescriptor>> {
        Ok(self.lock_state()?.tabs.clone())
    }

    fn active_tab(&self) -> Result<TabDescriptor> {
        let state = self.lock_state()?;
        Ok(Self::active_tab_from_state(&state)?.clone())
    }

    fn open_tab(&self, url: &str) -> Result<TabDescriptor> {
        let mut state = self.lock_state()?;
        let tab = TabDescriptor {
            id: format!("tab-{}", state.next_tab_id),
            title: Self::title_for_url(url),
            url: url.to_string(),
        };
        state.next_tab_id += 1;
        state.active_tab_id = Some(tab.id.clone());
        state.tabs.push(tab.clone());
        Self::bump_revision(&mut state);
        Ok(tab)
    }

    fn activate_tab(&self, tab_id: &str) -> Result<()> {
        let mut state = self.lock_state()?;
        if !state.tabs.iter().any(|tab| tab.id == tab_id) {
            return Err(BrowserError::TabOperationFailed(format!(
                "No tab found for id {}",
                tab_id
            )));
        }
        state.active_tab_id = Some(tab_id.to_string());
        Ok(())
    }

    fn close_tab(&self, tab_id: &str, _with_unload: bool) -> Result<()> {
        let mut state = self.lock_state()?;
        let index = state
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id)
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(format!("No tab found for id {}", tab_id))
            })?;
        let tab = state.tabs[index].clone();
        if self.close_failure_urls.iter().any(|url| url == &tab.url) {
            return Err(BrowserError::TabOperationFailed(format!(
                "Configured fake close failure for {}",
                tab.url
            )));
        }
        state.tabs.remove(index);

        if state.active_tab_id.as_deref() == Some(tab_id) {
            state.active_tab_id = state.tabs.first().map(|tab| tab.id.clone());
        }

        Self::bump_revision(&mut state);
        Ok(())
    }

    fn close(&self) -> Result<()> {
        let tab_ids = {
            let state = self.lock_state()?;
            state
                .tabs
                .iter()
                .map(|tab| tab.id.clone())
                .collect::<Vec<_>>()
        };
        let total_tabs = tab_ids.len();
        let mut failures = Vec::new();

        for tab_id in tab_ids {
            if let Err(err) = self.close_tab(&tab_id, false) {
                failures.push(err.to_string());
            }
        }

        session_close_result(total_tabs, failures)
    }
}
