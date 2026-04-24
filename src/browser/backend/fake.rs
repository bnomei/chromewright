use super::{
    ScreenshotCapture, ScreenshotImageMetrics, ScreenshotRequest, ScreenshotScale,
    ScriptEvaluation, SessionBackend, TabDescriptor,
};
use crate::browser::commands::{
    ActionCommandResult, ActionabilityDiagnostics, ActionabilityElementSummary,
    ActionabilityPredicate, ActionabilityProbeResult, BrowserCommand, BrowserCommandResult,
    HoverCommandResult, InputCommandResult, InteractionCommand, InteractionCommandResult,
    SelectCommandResult, SelectorIdentityProbeResult,
};
use crate::contract::{
    ViewportEmulation, ViewportEmulationRequest, ViewportMetrics, ViewportOperationResult,
    ViewportResetRequest,
};
use crate::dom::{AriaChild, AriaNode, DocumentMetadata, DomTree};
use crate::error::{BrowserError, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

const FAKE_SCREENSHOT_VIEWPORT_WIDTH: f64 = 800.0;
const FAKE_SCREENSHOT_VIEWPORT_HEIGHT: f64 = 600.0;
const FAKE_SCREENSHOT_FULL_PAGE_HEIGHT: f64 = 1800.0;
const FAKE_SCREENSHOT_DEVICE_PIXEL_RATIO: f64 = 2.0;
const FAKE_PNG_BYTES: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 255, 255, 255, 127, 0,
    9, 251, 3, 253, 160, 114, 168, 187, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

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

pub(crate) struct FakeSessionBackend {
    state: Mutex<FakeState>,
    close_failure_urls: Vec<String>,
}

#[derive(Debug)]
struct FakeState {
    tabs: Vec<TabDescriptor>,
    active_tab_id: Option<String>,
    next_tab_id: usize,
    revision: usize,
    viewport_emulation_by_tab_id: HashMap<String, ViewportEmulation>,
}

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
                viewport_emulation_by_tab_id: HashMap::new(),
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

        Self::tab_from_state(state, active_id)
    }

    fn tab_from_state<'a>(state: &'a FakeState, tab_id: &str) -> Result<&'a TabDescriptor> {
        state
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(format!("No tab found for id {}", tab_id))
            })
    }

    fn bump_revision(state: &mut FakeState) {
        state.revision += 1;
    }

    fn title_for_url(url: &str) -> String {
        url.to_string()
    }

    fn current_document(state: &FakeState) -> Result<DocumentMetadata> {
        let active = Self::active_tab_from_state(state)?;
        Self::document_for_tab(state, active)
    }

    fn document_for_tab(state: &FakeState, tab: &TabDescriptor) -> Result<DocumentMetadata> {
        Ok(DocumentMetadata {
            document_id: tab.id.clone(),
            revision: format!("fake:{}", state.revision),
            url: tab.url.clone(),
            title: tab.title.clone(),
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

    fn fake_png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = FAKE_PNG_BYTES.to_vec();
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        bytes
    }

    fn default_viewport_metrics() -> ViewportMetrics {
        ViewportMetrics {
            width: FAKE_SCREENSHOT_VIEWPORT_WIDTH,
            height: FAKE_SCREENSHOT_VIEWPORT_HEIGHT,
            device_pixel_ratio: FAKE_SCREENSHOT_DEVICE_PIXEL_RATIO,
        }
    }

    fn current_viewport_metrics(state: &FakeState, tab_id: &str) -> ViewportMetrics {
        state
            .viewport_emulation_by_tab_id
            .get(tab_id)
            .map(|emulation| ViewportMetrics {
                width: emulation.width as f64,
                height: emulation.height as f64,
                device_pixel_ratio: emulation.device_scale_factor,
            })
            .unwrap_or_else(Self::default_viewport_metrics)
    }

    fn current_scroll_height(state: &FakeState, tab_id: &str) -> f64 {
        Self::current_viewport_metrics(state, tab_id)
            .height
            .max(FAKE_SCREENSHOT_FULL_PAGE_HEIGHT)
    }

    fn fake_capture_geometry(
        state: &FakeState,
        tab_id: &str,
        request: &ScreenshotRequest,
    ) -> (f64, f64, f64) {
        let viewport = Self::current_viewport_metrics(state, tab_id);
        let css_width = request
            .clip
            .as_ref()
            .map(|clip| clip.width)
            .unwrap_or(viewport.width);
        let css_height = request
            .clip
            .as_ref()
            .map(|clip| clip.height)
            .unwrap_or_else(|| {
                if request.mode.capture_beyond_viewport() {
                    Self::current_scroll_height(state, tab_id)
                } else {
                    viewport.height
                }
            });
        let pixel_scale = match request.scale {
            ScreenshotScale::Device => viewport.device_pixel_ratio,
            ScreenshotScale::Css => 1.0,
        };

        (css_width, css_height, pixel_scale)
    }

    fn fake_actionability_result(
        predicates: &[ActionabilityPredicate],
    ) -> ActionabilityProbeResult {
        let mut result = ActionabilityProbeResult {
            present: true,
            frame_depth: Some(0),
            diagnostics: Some(ActionabilityDiagnostics {
                pointer_events: Some("auto".to_string()),
                hit_target: Some(ActionabilityElementSummary {
                    tag: "button".to_string(),
                    id: Some("fake-target".to_string()),
                    classes: Vec::new(),
                }),
                text_length: Some(11),
                has_value: Some(false),
            }),
            ..ActionabilityProbeResult::default()
        };

        for predicate in predicates {
            match predicate {
                ActionabilityPredicate::Present => {}
                ActionabilityPredicate::Visible => result.visible = Some(true),
                ActionabilityPredicate::Enabled => result.enabled = Some(true),
                ActionabilityPredicate::Editable => result.editable = Some(true),
                ActionabilityPredicate::Stable => result.stable = Some(true),
                ActionabilityPredicate::ReceivesEvents => result.receives_events = Some(true),
                ActionabilityPredicate::InViewport => result.in_viewport = Some(true),
                ActionabilityPredicate::UnobscuredCenter => result.unobscured_center = Some(true),
                ActionabilityPredicate::TextContains => result.text_contains = Some(true),
                ActionabilityPredicate::ValueEquals => result.value_equals = Some(true),
            }
        }

        result
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

    fn scripted_result_with_url(
        &self,
        script: &str,
        document_url: Option<&str>,
    ) -> Option<Result<ScriptEvaluation>> {
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
            let url = document_url
                .map(str::to_string)
                .or_else(|| self.active_tab().ok().map(|tab| tab.url))
                .unwrap_or_else(|| "about:blank".to_string());
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

        if script.contains("inspectElement(element, frameDepth, actionableIndex)") {
            let url = document_url
                .map(str::to_string)
                .or_else(|| self.active_tab().ok().map(|tab| tab.url))
                .unwrap_or_else(|| "about:blank".to_string());
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

impl Default for FakeSessionBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionBackend for FakeSessionBackend {
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

    fn extract_dom_for_tab(&self, tab_id: &str) -> Result<DomTree> {
        let state = self.lock_state()?;
        let tab = Self::tab_from_state(&state, tab_id)?;
        Ok(Self::fake_dom(&Self::document_for_tab(&state, tab)?))
    }

    fn extract_dom_with_prefix(&self, _prefix: &str) -> Result<DomTree> {
        self.extract_dom()
    }

    fn evaluate(&self, script: &str, _await_promise: bool) -> Result<ScriptEvaluation> {
        let state = self.lock_state()?;
        let active_tab = Self::active_tab_from_state(&state)?;
        self.scripted_result_with_url(script, Some(active_tab.url.as_str()))
            .unwrap_or_else(|| {
                Err(BrowserError::EvaluationFailed(
                    "Fake backend does not support this JavaScript payload yet".to_string(),
                ))
            })
    }

    fn evaluate_on_tab(
        &self,
        tab_id: &str,
        script: &str,
        _await_promise: bool,
    ) -> Result<ScriptEvaluation> {
        let state = self.lock_state()?;
        let tab = Self::tab_from_state(&state, tab_id)?;
        self.scripted_result_with_url(script, Some(tab.url.as_str()))
            .unwrap_or_else(|| {
                Err(BrowserError::EvaluationFailed(
                    "Fake backend does not support this JavaScript payload yet".to_string(),
                ))
            })
    }

    fn execute_command(&self, command: BrowserCommand) -> Result<BrowserCommandResult> {
        match command {
            BrowserCommand::ActionabilityProbe(request) => {
                Ok(BrowserCommandResult::ActionabilityProbe(
                    Self::fake_actionability_result(&request.predicates),
                ))
            }
            BrowserCommand::SelectorIdentityProbe(request) => Ok(
                BrowserCommandResult::SelectorIdentityProbe(SelectorIdentityProbeResult {
                    present: !request.selector.is_empty(),
                    unique: !request.selector.is_empty(),
                }),
            ),
            BrowserCommand::Interaction(command) => {
                let result = match command {
                    InteractionCommand::Click(_) => {
                        InteractionCommandResult::Click(ActionCommandResult {
                            success: true,
                            code: None,
                            error: None,
                        })
                    }
                    InteractionCommand::Input(_) => {
                        InteractionCommandResult::Input(InputCommandResult {
                            success: true,
                            code: None,
                            error: None,
                            value: Some("fake text".to_string()),
                        })
                    }
                    InteractionCommand::Hover(_) => {
                        InteractionCommandResult::Hover(HoverCommandResult {
                            success: true,
                            code: None,
                            error: None,
                            tag_name: Some("BUTTON".to_string()),
                            id: Some("fake-target".to_string()),
                            class_name: Some("fake".to_string()),
                        })
                    }
                    InteractionCommand::Select(_) => {
                        InteractionCommandResult::Select(SelectCommandResult {
                            success: true,
                            code: None,
                            error: None,
                            selected_value: Some("fake-value".to_string()),
                            selected_text: Some("Fake option".to_string()),
                        })
                    }
                };
                Ok(BrowserCommandResult::Interaction(result))
            }
        }
    }

    fn capture_screenshot(&self, _full_page: bool) -> Result<Vec<u8>> {
        Ok(Self::fake_png(1, 1))
    }

    fn capture_screenshot_with_request(
        &self,
        request: &ScreenshotRequest,
    ) -> Result<ScreenshotCapture> {
        request.validate()?;
        let state = self.lock_state()?;
        let tab = match request.tab_id.as_deref() {
            Some(tab_id) => Self::tab_from_state(&state, tab_id)?.clone(),
            None => Self::active_tab_from_state(&state)?.clone(),
        };

        let viewport = Self::current_viewport_metrics(&state, &tab.id);
        let (css_width, css_height, pixel_scale) =
            Self::fake_capture_geometry(&state, &tab.id, request);
        let width = (css_width * pixel_scale).round().max(1.0) as u32;
        let height = (css_height * pixel_scale).round().max(1.0) as u32;
        let bytes = Self::fake_png(width, height);
        ScreenshotCapture::from_png_bytes(
            request.mode,
            request.scale,
            tab,
            request.clip.clone(),
            ScreenshotImageMetrics {
                css_width,
                css_height,
                device_pixel_ratio: viewport.device_pixel_ratio,
            },
            bytes,
        )
    }

    fn viewport_metrics(&self, tab_id: Option<&str>) -> Result<ViewportMetrics> {
        let state = self.lock_state()?;
        let tab = match tab_id {
            Some(tab_id) => Self::tab_from_state(&state, tab_id)?,
            None => Self::active_tab_from_state(&state)?,
        };

        Ok(Self::current_viewport_metrics(&state, &tab.id))
    }

    fn apply_viewport_emulation(
        &self,
        request: &ViewportEmulationRequest,
    ) -> Result<ViewportOperationResult> {
        request.validate()?;

        let mut state = self.lock_state()?;
        let tab = match request.tab_id.as_deref() {
            Some(tab_id) => Self::tab_from_state(&state, tab_id)?.clone(),
            None => Self::active_tab_from_state(&state)?.clone(),
        };
        let emulation = request.normalized_emulation();
        state
            .viewport_emulation_by_tab_id
            .insert(tab.id.clone(), emulation.clone());

        Ok(ViewportOperationResult {
            tab_id: tab.id.clone(),
            emulation: Some(emulation),
            viewport_after: Self::current_viewport_metrics(&state, &tab.id),
        })
    }

    fn reset_viewport_emulation(
        &self,
        request: &ViewportResetRequest,
    ) -> Result<ViewportOperationResult> {
        request.validate()?;

        let mut state = self.lock_state()?;
        let tab = match request.tab_id.as_deref() {
            Some(tab_id) => Self::tab_from_state(&state, tab_id)?.clone(),
            None => Self::active_tab_from_state(&state)?.clone(),
        };
        state.viewport_emulation_by_tab_id.remove(&tab.id);

        Ok(ViewportOperationResult {
            tab_id: tab.id.clone(),
            emulation: None,
            viewport_after: Self::current_viewport_metrics(&state, &tab.id),
        })
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

    fn activate_tab(&self, target_tab_id: &str) -> Result<()> {
        let mut state = self.lock_state()?;
        if state.tabs.iter().any(|tab| tab.id == target_tab_id) {
            state.active_tab_id = Some(target_tab_id.to_string());
            Self::bump_revision(&mut state);
            Ok(())
        } else {
            Err(BrowserError::TabOperationFailed(format!(
                "No tab found for id {}",
                target_tab_id
            )))
        }
    }

    fn close_tab(&self, target_tab_id: &str, _with_unload: bool) -> Result<()> {
        let mut state = self.lock_state()?;
        let Some(index) = state.tabs.iter().position(|tab| tab.id == target_tab_id) else {
            return Err(BrowserError::TabOperationFailed(format!(
                "No tab found for id {}",
                target_tab_id
            )));
        };

        let tab = state.tabs[index].clone();
        if self.close_failure_urls.iter().any(|url| url == &tab.url) {
            return Err(BrowserError::TabOperationFailed(format!(
                "Failed to close tab: {}",
                tab.url
            )));
        }

        state.tabs.remove(index);
        state.viewport_emulation_by_tab_id.remove(target_tab_id);
        if state.tabs.is_empty() {
            state.active_tab_id = None;
        } else if state.active_tab_id.as_deref() == Some(target_tab_id) {
            state.active_tab_id = Some(state.tabs[0].id.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::commands::{
        ActionabilityProbeRequest, InputInteractionRequest, SelectInteractionRequest,
        SelectorIdentityProbeRequest, TargetedInteractionRequest,
    };

    #[test]
    fn fake_backend_actionability_uses_command_predicates() {
        let backend = FakeSessionBackend::new();
        let result = backend
            .execute_command(BrowserCommand::ActionabilityProbe(
                ActionabilityProbeRequest {
                    selector: "#fake-target".to_string(),
                    target_index: None,
                    predicates: vec![
                        ActionabilityPredicate::Present,
                        ActionabilityPredicate::Visible,
                    ],
                    expected_text: None,
                    expected_value: None,
                },
            ))
            .expect("fake backend should execute actionability command");

        let BrowserCommandResult::ActionabilityProbe(result) = result else {
            panic!("expected actionability command result");
        };
        assert!(result.present);
        assert_eq!(result.visible, Some(true));
        assert_eq!(result.enabled, None);
        assert_eq!(result.receives_events, None);
    }

    #[test]
    fn fake_backend_selector_identity_uses_selector_command_payload() {
        let backend = FakeSessionBackend::new();
        let present = backend
            .execute_command(BrowserCommand::SelectorIdentityProbe(
                SelectorIdentityProbeRequest {
                    selector: "#fake-target".to_string(),
                },
            ))
            .expect("fake backend should execute selector identity command");
        let absent = backend
            .execute_command(BrowserCommand::SelectorIdentityProbe(
                SelectorIdentityProbeRequest {
                    selector: String::new(),
                },
            ))
            .expect("fake backend should execute empty selector identity command");

        let BrowserCommandResult::SelectorIdentityProbe(present) = present else {
            panic!("expected selector identity command result");
        };
        let BrowserCommandResult::SelectorIdentityProbe(absent) = absent else {
            panic!("expected selector identity command result");
        };

        assert!(present.present);
        assert!(present.unique);
        assert!(!absent.present);
        assert!(!absent.unique);
    }

    #[test]
    fn fake_backend_interactions_use_typed_command_variants() {
        let backend = FakeSessionBackend::new();
        let target = TargetedInteractionRequest {
            selector: "#fake-target".to_string(),
            target_index: Some(0),
        };

        let click = backend
            .execute_command(BrowserCommand::Interaction(InteractionCommand::Click(
                target.clone(),
            )))
            .expect("fake backend should execute click command");
        let input = backend
            .execute_command(BrowserCommand::Interaction(InteractionCommand::Input(
                InputInteractionRequest {
                    target: target.clone(),
                    text: "hello".to_string(),
                    clear: true,
                },
            )))
            .expect("fake backend should execute input command");
        let hover = backend
            .execute_command(BrowserCommand::Interaction(InteractionCommand::Hover(
                target.clone(),
            )))
            .expect("fake backend should execute hover command");
        let select = backend
            .execute_command(BrowserCommand::Interaction(InteractionCommand::Select(
                SelectInteractionRequest {
                    target,
                    value: "fake-value".to_string(),
                },
            )))
            .expect("fake backend should execute select command");

        assert!(matches!(
            click,
            BrowserCommandResult::Interaction(InteractionCommandResult::Click(
                ActionCommandResult { success: true, .. }
            ))
        ));
        assert!(matches!(
            input,
            BrowserCommandResult::Interaction(InteractionCommandResult::Input(
                InputCommandResult {
                    success: true,
                    value: Some(_),
                    ..
                }
            ))
        ));
        assert!(matches!(
            hover,
            BrowserCommandResult::Interaction(InteractionCommandResult::Hover(
                HoverCommandResult {
                    success: true,
                    tag_name: Some(_),
                    ..
                }
            ))
        ));
        assert!(matches!(
            select,
            BrowserCommandResult::Interaction(InteractionCommandResult::Select(
                SelectCommandResult {
                    success: true,
                    selected_value: Some(_),
                    ..
                }
            ))
        ));
    }

    #[test]
    fn fake_backend_no_longer_recognizes_rendered_actionability_script() {
        let backend = FakeSessionBackend::new();
        let script = BrowserCommand::ActionabilityProbe(ActionabilityProbeRequest {
            selector: "#fake-target".to_string(),
            target_index: None,
            predicates: vec![ActionabilityPredicate::Present],
            expected_text: None,
            expected_value: None,
        })
        .render_script();

        let error = backend
            .evaluate(&script, true)
            .expect_err("fake backend should not recognize migrated rendered scripts");
        assert!(matches!(error, BrowserError::EvaluationFailed(_)));
    }
}
