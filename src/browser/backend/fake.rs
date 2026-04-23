use super::{
    ScreenshotCapture, ScreenshotImageMetrics, ScreenshotRequest, ScreenshotScale,
    ScriptEvaluation, SessionBackend, TabDescriptor,
};
use crate::dom::{AriaChild, AriaNode, DocumentMetadata, DomTree};
use crate::error::{BrowserError, Result};
use serde_json::Value;
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

    fn fake_capture_geometry(request: &ScreenshotRequest) -> (f64, f64, f64) {
        let css_width = request
            .clip
            .as_ref()
            .map(|clip| clip.width)
            .unwrap_or(FAKE_SCREENSHOT_VIEWPORT_WIDTH);
        let css_height = request
            .clip
            .as_ref()
            .map(|clip| clip.height)
            .unwrap_or_else(|| {
                if request.mode.capture_beyond_viewport() {
                    FAKE_SCREENSHOT_FULL_PAGE_HEIGHT
                } else {
                    FAKE_SCREENSHOT_VIEWPORT_HEIGHT
                }
            });
        let pixel_scale = match request.scale {
            ScreenshotScale::Device => FAKE_SCREENSHOT_DEVICE_PIXEL_RATIO,
            ScreenshotScale::Css => 1.0,
        };

        (css_width, css_height, pixel_scale)
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

    fn scripted_result(&self, script: &str) -> Option<Result<ScriptEvaluation>> {
        self.scripted_result_with_url(script, None)
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
        self.scripted_result(script).unwrap_or_else(|| {
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

    fn capture_screenshot(&self, _full_page: bool) -> Result<Vec<u8>> {
        Ok(Self::fake_png(1, 1))
    }

    fn capture_screenshot_with_request(
        &self,
        request: &ScreenshotRequest,
    ) -> Result<ScreenshotCapture> {
        request.validate()?;
        let tab = match request.tab_id.as_deref() {
            Some(tab_id) => {
                let state = self.lock_state()?;
                Self::tab_from_state(&state, tab_id)?.clone()
            }
            None => self.active_tab()?,
        };

        let (css_width, css_height, pixel_scale) = Self::fake_capture_geometry(request);
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
                device_pixel_ratio: FAKE_SCREENSHOT_DEVICE_PIXEL_RATIO,
            },
            bytes,
        )
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
