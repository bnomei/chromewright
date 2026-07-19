//! Browser session bridge used by the TUI controller.
//!
//! Production uses [`SessionPageDriver`] over a shared [`BrowserSession`].
//! Tests inject [`FakePageDriver`] for lifecycle atomicity without Chrome.
//! Activations always resolve an exact current-document `semantic_ref` and
//! fail closed on stale document identity or ambiguous locators.

use crate::browser::BrowserSession;
use crate::dom::DocumentMetadata;
use crate::error::{BrowserError, Result};
use crate::semantic::{SemanticDocument, SemanticKind, SemanticRef};
use crate::tools::utils::validate_navigation_url;
use std::time::Duration;

/// Page-mutating and capture operations the controller needs.
///
/// Implementations must not invent local history or rebind stale refs. Settle
/// uses document readiness rather than requiring a navigation event, because
/// form focus/fill may not navigate.
pub trait PageDriver {
    fn navigate(&mut self, url: &str) -> Result<()>;
    fn go_back(&mut self) -> Result<()>;
    fn go_forward(&mut self) -> Result<()>;
    fn reload(&mut self) -> Result<()>;
    /// Wait until the active document is ready after a mutation.
    fn wait_settle(&mut self) -> Result<()>;
    /// Capture a complete SemanticDocument for atomic publish.
    fn capture_semantic(&mut self) -> Result<SemanticDocument>;
    /// Freshness barrier metadata; must match the just-captured document.
    fn document_metadata(&mut self) -> Result<DocumentMetadata>;
    fn open_tab(&mut self, url: &str) -> Result<()>;
    fn close_active_tab(&mut self) -> Result<()>;
    /// Whether the browser still has at least one open tab after a close.
    fn has_open_tabs(&mut self) -> Result<bool>;
    fn next_tab(&mut self) -> Result<()>;
    fn prev_tab(&mut self) -> Result<()>;
    /// Activate a link or control identified by an exact current-document `semantic_ref`.
    ///
    /// Returns whether the action is expected to change the page (true for
    /// links/buttons; false when only focusing a form control).
    fn activate_ref(
        &mut self,
        document: &SemanticDocument,
        semantic_ref: &SemanticRef,
        new_tab: bool,
    ) -> Result<bool>;
    /// Write text into an input/textarea/select without submitting the form.
    ///
    /// Used when tabbing between fields so multi-field values reach the live
    /// DOM before Enter triggers submit + recapture.
    fn set_control_value(
        &mut self,
        document: &SemanticDocument,
        semantic_ref: &SemanticRef,
        text: &str,
    ) -> Result<()>;
    /// Availability belongs to the active browser tab. Implementations that
    /// cannot determine it must return `(false, false)` rather than inventing
    /// local history.
    fn history_availability(&mut self) -> Result<(bool, bool)> {
        Ok((false, false))
    }

    /// Active tab ordinal among open tabs as `(index_1based, count)`.
    ///
    /// Return `None` when there are no tabs or position cannot be determined.
    fn tab_position(&mut self) -> Result<Option<(usize, usize)>> {
        Ok(None)
    }
}

/// Production driver over a shared [`BrowserSession`].
pub struct SessionPageDriver<'a> {
    pub session: &'a BrowserSession,
}

impl<'a> SessionPageDriver<'a> {
    pub fn new(session: &'a BrowserSession) -> Self {
        Self { session }
    }
}

impl PageDriver for SessionPageDriver<'_> {
    fn navigate(&mut self, url: &str) -> Result<()> {
        let normalized = validate_navigation_url(url, false)?;
        self.session.navigate(&normalized)?;
        // Match the MCP navigate tool: do not return until the tab has left the
        // previous document and reached readyState=complete. Without this,
        // wait_settle can succeed on the *old* complete page and capture stale DOM
        // while the address bar already shows the new URL.
        self.session.wait_for_navigation()?;
        Ok(())
    }

    fn go_back(&mut self) -> Result<()> {
        self.session.go_back()
    }

    fn go_forward(&mut self) -> Result<()> {
        self.session.go_forward()
    }

    fn reload(&mut self) -> Result<()> {
        self.session.evaluate("location.reload()", false)?;
        // Same barrier as navigate: reload is asynchronous.
        let _ = self.session.wait_for_navigation();
        Ok(())
    }

    fn wait_settle(&mut self) -> Result<()> {
        // 1) Document must be complete (covers mid-navigation / reload).
        // 2) Then wait until identity+revision+url stay quiet so SPA/filter
        //    mutations after input/change finish before semantic capture.
        //    readyState alone returns immediately on already-complete pages and
        //    used to publish URL-updated but body-stale snapshots.
        self.session
            .wait_for_document_ready_with_timeout(Duration::from_secs(15))?;
        self.session
            .wait_for_dom_quiet(Duration::from_secs(4), Duration::from_millis(250))?;
        Ok(())
    }

    fn capture_semantic(&mut self) -> Result<SemanticDocument> {
        self.session.extract_semantic_document()
    }

    fn document_metadata(&mut self) -> Result<DocumentMetadata> {
        self.session.document_metadata()
    }

    fn open_tab(&mut self, url: &str) -> Result<()> {
        let normalized = validate_navigation_url(url, false)?;
        self.session.open_tab(&normalized)?;
        // New tab navigation is async; wait so the first capture is not blank/stale.
        let _ = self.session.wait_for_navigation();
        Ok(())
    }

    fn close_active_tab(&mut self) -> Result<()> {
        self.session.close_active_tab()?;
        Ok(())
    }

    fn has_open_tabs(&mut self) -> Result<bool> {
        Ok(!self.session.list_tabs()?.is_empty())
    }

    fn next_tab(&mut self) -> Result<()> {
        cycle_tab(self.session, 1)
    }

    fn prev_tab(&mut self) -> Result<()> {
        cycle_tab(self.session, -1)
    }

    fn activate_ref(
        &mut self,
        document: &SemanticDocument,
        semantic_ref: &SemanticRef,
        new_tab: bool,
    ) -> Result<bool> {
        let component = document
            .resolve(semantic_ref)
            .map_err(|e| BrowserError::InvalidArgument(e.to_string()))?;
        match component.kind {
            SemanticKind::Link => {
                if new_tab {
                    match resolved_link_target(self.session, document, component) {
                        Ok(target) => self.open_tab(&target)?,
                        Err(_) => {
                            // Live locator went stale; fall back to captured href.
                            let target = link_href_fallback(document, component)?;
                            self.open_tab(&target)?;
                        }
                    }
                } else {
                    // Prefer a real click (SPA handlers, relative resolution).
                    // On dynamic pages the capture revision/locator often goes
                    // stale before the click runs — fall back to navigating the
                    // captured/resolved href so the user can still leave the page.
                    if let Err(click_err) = click_component(self.session, document, component, true)
                    {
                        match link_href_fallback(document, component) {
                            Ok(target) => {
                                log::warn!(
                                    "link click failed ({click_err}); navigating to fallback href {target}"
                                );
                                self.navigate(&target)?;
                            }
                            Err(_) => return Err(click_err),
                        }
                    }
                }
                Ok(true)
            }
            SemanticKind::Button => {
                click_component(self.session, document, component, false)?;
                Ok(true)
            }
            SemanticKind::Input => {
                let t = component
                    .attrs
                    .input_type
                    .as_deref()
                    .unwrap_or("text")
                    .to_ascii_lowercase();
                if matches!(t.as_str(), "checkbox" | "radio" | "submit" | "button" | "reset") {
                    // Toggle / activate — may not navigate; controller still recaptures.
                    click_component(self.session, document, component, false)?;
                    Ok(true)
                } else {
                    // Text-like: focus only (edit mode is TUI-local).
                    focus_component(self.session, document, component)?;
                    Ok(false)
                }
            }
            SemanticKind::Textarea | SemanticKind::Select => {
                focus_component(self.session, document, component)?;
                Ok(false)
            }
            _ => Err(BrowserError::InvalidArgument(
                "component is not activatable".into(),
            )),
        }
    }

    fn set_control_value(
        &mut self,
        document: &SemanticDocument,
        semantic_ref: &SemanticRef,
        text: &str,
    ) -> Result<()> {
        let component = document
            .resolve(semantic_ref)
            .map_err(|e| BrowserError::InvalidArgument(e.to_string()))?;
        match component.kind {
            SemanticKind::Input | SemanticKind::Textarea => {
                set_value_only(self.session, document, component, text)
            }
            SemanticKind::Select => select_value(self.session, document, component, text),
            _ => Err(BrowserError::InvalidArgument(
                "component is not a settable control".into(),
            )),
        }
    }

    fn history_availability(&mut self) -> Result<(bool, bool)> {
        let script = r#"(function(){ const n = globalThis.navigation; return JSON.stringify({back: !!(n && n.canGoBack), forward: !!(n && n.canGoForward)}); })()"#;
        let value = self.session.evaluate(script, false)?;
        let parsed = value.value.and_then(|v| {
            v.as_str()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        });
        Ok(parsed
            .map(|v| {
                (
                    v["back"].as_bool().unwrap_or(false),
                    v["forward"].as_bool().unwrap_or(false),
                )
            })
            .unwrap_or((false, false)))
    }

    fn tab_position(&mut self) -> Result<Option<(usize, usize)>> {
        let tabs = self.session.list_tabs()?;
        if tabs.is_empty() {
            return Ok(None);
        }
        let index = tabs
            .iter()
            .position(|t| t.active)
            .map(|i| i + 1)
            .unwrap_or(1);
        Ok(Some((index, tabs.len())))
    }
}

fn cycle_tab(session: &BrowserSession, delta: isize) -> Result<()> {
    let tabs = session.list_tabs()?;
    if tabs.is_empty() {
        return Ok(());
    }
    let active = tabs.iter().position(|t| t.active).unwrap_or(0);
    let len = tabs.len() as isize;
    let next = (active as isize + delta).rem_euclid(len) as usize;
    session.activate_tab(&tabs[next].id)?;
    Ok(())
}

fn resolve_href(base: &str, href: &str) -> String {
    if href.starts_with("http://")
        || href.starts_with("https://")
        || href.starts_with("about:")
        || href.starts_with("data:")
        || href.starts_with("file:")
    {
        return href.to_string();
    }
    if href.starts_with("//") {
        let scheme = if base.starts_with("https") {
            "https:"
        } else {
            "http:"
        };
        return format!("{scheme}{href}");
    }
    if href.starts_with('/')
        && let Some(origin_end) = base.find("://")
    {
        let after = &base[origin_end + 3..];
        let host_end = after
            .find('/')
            .map(|i| origin_end + 3 + i)
            .unwrap_or(base.len());
        return format!("{}{}", &base[..host_end], href);
    }
    // Relative to current path directory
    if let Some(slash) = base.rfind('/') {
        return format!("{}{}", &base[..=slash], href);
    }
    href.to_string()
}

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn exact_target_prelude(
    document: &SemanticDocument,
    component: &crate::semantic::SemanticComponent,
) -> Result<String> {
    let locator = component.interaction_selector.as_deref().ok_or_else(|| {
        BrowserError::InvalidArgument("semantic ref has no exact interaction locator".into())
    })?;
    let selectors: Vec<String> = serde_json::from_str(locator).map_err(|_| {
        BrowserError::InvalidArgument("semantic interaction locator is malformed".into())
    })?;
    if selectors.is_empty() || selectors.iter().any(|selector| selector.is_empty()) {
        return Err(BrowserError::InvalidArgument(
            "semantic interaction locator is empty".into(),
        ));
    }
    let selectors = serde_json::to_string(&selectors).map_err(|error| {
        BrowserError::InvalidArgument(format!(
            "semantic interaction locator cannot be encoded: {error}"
        ))
    })?;
    let expected_tag = component
        .attrs
        .tag
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase();
    Ok(format!(
        r#"const state = globalThis.__browserUseDocumentState;
            if (!state || state.documentId !== {document_id} || ('main:' + String(state.revision)) !== {revision}) return 'stale';
            const selectors = {selectors};
            let scope = document;
            let el = null;
            for (let index = 0; index < selectors.length; index += 1) {{
                const matches = Array.from(scope.querySelectorAll(selectors[index]));
                if (matches.length !== 1) return 'ambiguous';
                el = matches[0];
                if (index + 1 < selectors.length) {{
                    if (!el.shadowRoot) return 'missing_shadow_root';
                    scope = el.shadowRoot;
                }}
            }}
            if ({expected_tag} && el.tagName !== {expected_tag}) return 'kind_mismatch';"#,
        document_id = js_string(&document.document.document_id),
        revision = js_string(&document.document.revision),
        selectors = selectors,
        expected_tag = js_string(&expected_tag),
    ))
}

fn ensure_target_result(result: crate::browser::backend::ScriptEvaluation) -> Result<()> {
    match result.value.as_ref().and_then(|v| v.as_str()) {
        Some("ok") => Ok(()),
        Some(reason) => Err(BrowserError::InvalidArgument(format!(
            "semantic target rejected: {reason}"
        ))),
        None => Err(BrowserError::InvalidArgument(
            "semantic target evaluation failed".into(),
        )),
    }
}

fn click_component(
    session: &BrowserSession,
    document: &SemanticDocument,
    component: &crate::semantic::SemanticComponent,
    force_current_tab: bool,
) -> Result<()> {
    let prelude = exact_target_prelude(document, component)?;
    let script = format!(
        r#"(function(){{ {prelude} if ({force_current_tab}) el.target = '_self'; el.click(); return 'ok'; }})()"#
    );
    ensure_target_result(session.evaluate(&script, false)?)
}

fn focus_component(
    session: &BrowserSession,
    document: &SemanticDocument,
    component: &crate::semantic::SemanticComponent,
) -> Result<()> {
    let prelude = exact_target_prelude(document, component)?;
    let script = format!(
        r#"(function(){{ {prelude} el.focus(); const root = el.getRootNode(); return root.activeElement === el ? 'ok' : 'missing'; }})()"#
    );
    ensure_target_result(session.evaluate(&script, false)?)
}

/// Set a control value and fire input/change without submitting the form.
fn set_value_only(
    session: &BrowserSession,
    document: &SemanticDocument,
    component: &crate::semantic::SemanticComponent,
    text: &str,
) -> Result<()> {
    let prelude = exact_target_prelude(document, component)?;
    // Native value setter + InputEvent: React-style trackers see the prototype
    // write; Holmes and similar plain `input` listeners only need bubbles + value.
    let script = format!(
        r#"(function(){{
            {prelude}
            if (el.readOnly || el.disabled) return 'readonly';
            el.focus();
            if (el.type === 'checkbox' || el.type === 'radio') {{
                const want = {text} === 'true' || {text} === '1' || {text} === 'on';
                if (el.checked !== want) el.click();
                return 'ok';
            }}
            const proto = el instanceof HTMLTextAreaElement
                ? HTMLTextAreaElement.prototype
                : HTMLInputElement.prototype;
            const desc = Object.getOwnPropertyDescriptor(proto, 'value');
            if (desc && desc.set) {{
                desc.set.call(el, {text});
            }} else {{
                el.value = {text};
            }}
            try {{
                el.dispatchEvent(new InputEvent('input', {{
                    bubbles: true,
                    cancelable: true,
                    inputType: 'insertText',
                    data: {text}
                }}));
            }} catch (e) {{
                el.dispatchEvent(new Event('input', {{ bubbles: true }}));
            }}
            el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            return 'ok';
        }})()"#,
        prelude = prelude,
        text = js_string(text)
    );
    ensure_target_result(session.evaluate(&script, false)?)
}

fn select_value(
    session: &BrowserSession,
    document: &SemanticDocument,
    component: &crate::semantic::SemanticComponent,
    value: &str,
) -> Result<()> {
    let prelude = exact_target_prelude(document, component)?;
    let script = format!(
        r#"(function(){{
            {prelude}
            if (el.disabled) return 'readonly';
            el.value = {value};
            if (el.value !== {value}) return 'missing';
            el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            return 'ok';
        }})()"#,
        prelude = prelude,
        value = js_string(value)
    );
    ensure_target_result(session.evaluate(&script, false)?)
}

fn resolved_link_target(
    session: &BrowserSession,
    document: &SemanticDocument,
    component: &crate::semantic::SemanticComponent,
) -> Result<String> {
    let prelude = exact_target_prelude(document, component)?;
    let script = format!(r#"(function(){{ {prelude} return el.href || 'missing'; }})()"#,);
    let result = session.evaluate(&script, false)?;
    result
        .value
        .and_then(|v| v.as_str().map(str::to_owned))
        .filter(|s| {
            !matches!(
                s.as_str(),
                "missing" | "stale" | "ambiguous" | "kind_mismatch"
            )
        })
        .ok_or_else(|| BrowserError::InvalidArgument("target link not found in page".into()))
}

/// Build an absolute navigation target from the captured href when the live
/// DOM locator is no longer valid (SPA re-render, revision bump, etc.).
fn link_href_fallback(
    document: &SemanticDocument,
    component: &crate::semantic::SemanticComponent,
) -> Result<String> {
    let href = component
        .attrs
        .href
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            BrowserError::InvalidArgument("link has no href for navigation fallback".into())
        })?;
    let base = document.document.url.as_str();
    let absolute = resolve_href(base, href);
    // Reuse the same safety gate as explicit navigation (http/https only by default).
    validate_navigation_url(&absolute, false)
}

/// Scripted fake driver for unit tests of lifecycle atomicity without Chrome.
///
/// Pages advance on navigation/history/reload; failures and metadata handoffs
/// are injectable so tests can assert Loading → Ready | Error without a browser.
#[derive(Debug)]
pub struct FakePageDriver {
    pub pages: Vec<SemanticDocument>,
    pub page_index: usize,
    pub navigate_calls: Vec<String>,
    pub back_calls: usize,
    pub forward_calls: usize,
    pub reload_calls: usize,
    pub capture_calls: usize,
    /// Metadata returned after captures when a test needs to model a browser
    /// handoff that differs from the captured semantic document.
    pub metadata_responses: Vec<DocumentMetadata>,
    /// Advance to the next scripted page immediately before capture. This
    /// models hydration changing the document after settling but before the
    /// semantic snapshot is extracted.
    pub advance_page_on_capture: bool,
    pub fail_next: Option<String>,
    pub fail_capture: Option<String>,
    pub activated: Vec<(String, bool)>,
    pub filled: Vec<(String, String)>,
    pub open_tabs: Vec<String>,
    pub tab_ops: Vec<&'static str>,
    /// Simulated open-tab count for empty-session close tests.
    pub open_tab_count: usize,
    /// 1-based active tab index for chrome `2/5` (clamped to `open_tab_count`).
    pub active_tab_index: usize,
    pub history: (bool, bool),
}

impl Default for FakePageDriver {
    fn default() -> Self {
        Self {
            pages: Vec::new(),
            page_index: 0,
            navigate_calls: Vec::new(),
            back_calls: 0,
            forward_calls: 0,
            reload_calls: 0,
            capture_calls: 0,
            metadata_responses: Vec::new(),
            advance_page_on_capture: false,
            fail_next: None,
            fail_capture: None,
            activated: Vec::new(),
            filled: Vec::new(),
            open_tabs: Vec::new(),
            tab_ops: Vec::new(),
            open_tab_count: 1,
            active_tab_index: 1,
            history: (false, false),
        }
    }
}

impl FakePageDriver {
    pub fn new(pages: Vec<SemanticDocument>) -> Self {
        Self {
            pages,
            ..Default::default()
        }
    }

    fn current(&self) -> Result<SemanticDocument> {
        self.pages
            .get(self.page_index)
            .cloned()
            .ok_or_else(|| BrowserError::DomParseFailed("no fake page".into()))
    }
}

impl PageDriver for FakePageDriver {
    fn navigate(&mut self, url: &str) -> Result<()> {
        if let Some(msg) = self.fail_next.take() {
            return Err(BrowserError::NavigationFailed(msg));
        }
        self.navigate_calls.push(url.to_string());
        // Prefer matching a scripted page by URL when present; otherwise keep
        // the current fixture page (index advance) for multi-step nav tests.
        if let Some(idx) = self.pages.iter().position(|p| p.document.url == url) {
            self.page_index = idx;
        } else if self.page_index + 1 < self.pages.len() {
            self.page_index += 1;
        }
        Ok(())
    }

    fn go_back(&mut self) -> Result<()> {
        if let Some(msg) = self.fail_next.take() {
            return Err(BrowserError::NavigationFailed(msg));
        }
        self.back_calls += 1;
        if self.page_index > 0 {
            self.page_index -= 1;
        }
        Ok(())
    }

    fn go_forward(&mut self) -> Result<()> {
        if let Some(msg) = self.fail_next.take() {
            return Err(BrowserError::NavigationFailed(msg));
        }
        self.forward_calls += 1;
        if self.page_index + 1 < self.pages.len() {
            self.page_index += 1;
        }
        Ok(())
    }

    fn reload(&mut self) -> Result<()> {
        if let Some(msg) = self.fail_next.take() {
            return Err(BrowserError::NavigationFailed(msg));
        }
        self.reload_calls += 1;
        // Test fixtures may provide the post-reload capture as the next page;
        // advance deterministically so lifecycle/anchor tests exercise a
        // genuine fresh document rather than replaying the old one.
        if self.page_index + 1 < self.pages.len() {
            self.page_index += 1;
        }
        Ok(())
    }

    fn wait_settle(&mut self) -> Result<()> {
        if let Some(msg) = self.fail_next.take() {
            return Err(BrowserError::Timeout(msg));
        }
        Ok(())
    }

    fn capture_semantic(&mut self) -> Result<SemanticDocument> {
        self.capture_calls += 1;
        if let Some(msg) = self.fail_capture.take() {
            return Err(BrowserError::DomParseFailed(msg));
        }
        if self.advance_page_on_capture && self.page_index + 1 < self.pages.len() {
            self.page_index += 1;
        }
        self.current()
    }

    fn document_metadata(&mut self) -> Result<DocumentMetadata> {
        if self.metadata_responses.is_empty() {
            Ok(self.current()?.document)
        } else if self.metadata_responses.len() == 1 {
            // Sticky final scripted response so capture retries keep seeing the
            // injected mismatch instead of falling through to a matching doc.
            Ok(self.metadata_responses[0].clone())
        } else {
            Ok(self.metadata_responses.remove(0))
        }
    }

    fn open_tab(&mut self, url: &str) -> Result<()> {
        self.open_tabs.push(url.to_string());
        self.open_tab_count = self.open_tab_count.saturating_add(1);
        self.active_tab_index = self.open_tab_count.max(1);
        // Prefer an existing scripted page for this URL (empty-session navigate
        // recovery). Otherwise provide a captureable blank/page for the new tab.
        if let Some(idx) = self.pages.iter().position(|p| p.document.url == url) {
            self.page_index = idx;
            return Ok(());
        }
        let blank = SemanticDocument::empty(DocumentMetadata {
            document_id: format!("blank-{}", self.open_tabs.len()),
            revision: "main:1".into(),
            url: url.to_string(),
            title: String::new(),
            ready_state: "complete".into(),
            frames: vec![],
        })
        .map_err(|e| BrowserError::DomParseFailed(e.to_string()))?;
        self.pages.push(blank);
        self.page_index = self.pages.len() - 1;
        Ok(())
    }

    fn close_active_tab(&mut self) -> Result<()> {
        self.tab_ops.push("close");
        self.open_tab_count = self.open_tab_count.saturating_sub(1);
        if self.open_tab_count == 0 {
            self.pages.clear();
            self.page_index = 0;
            self.active_tab_index = 0;
        } else {
            self.active_tab_index = self.active_tab_index.min(self.open_tab_count).max(1);
        }
        Ok(())
    }

    fn has_open_tabs(&mut self) -> Result<bool> {
        Ok(self.open_tab_count > 0)
    }

    fn next_tab(&mut self) -> Result<()> {
        self.tab_ops.push("next");
        if self.open_tab_count > 0 {
            self.active_tab_index = if self.active_tab_index >= self.open_tab_count {
                1
            } else {
                self.active_tab_index + 1
            };
        }
        Ok(())
    }

    fn prev_tab(&mut self) -> Result<()> {
        self.tab_ops.push("prev");
        if self.open_tab_count > 0 {
            self.active_tab_index = if self.active_tab_index <= 1 {
                self.open_tab_count
            } else {
                self.active_tab_index - 1
            };
        }
        Ok(())
    }

    fn activate_ref(
        &mut self,
        document: &SemanticDocument,
        semantic_ref: &SemanticRef,
        new_tab: bool,
    ) -> Result<bool> {
        let component = document
            .resolve(semantic_ref)
            .map_err(|e| BrowserError::InvalidArgument(e.to_string()))?;
        self.activated
            .push((semantic_ref.as_str().to_string(), new_tab));
        match component.kind {
            SemanticKind::Link => {
                if let Some(href) = &component.attrs.href {
                    if new_tab {
                        self.open_tabs.push(href.clone());
                    } else {
                        self.navigate_calls.push(href.clone());
                        if self.page_index + 1 < self.pages.len() {
                            self.page_index += 1;
                        }
                    }
                }
                Ok(true)
            }
            // Buttons / submit-like inputs: click without leaving the page unless
            // a later test advances pages. Matches production activate_ref.
            SemanticKind::Button => Ok(true),
            SemanticKind::Input => {
                let t = component
                    .attrs
                    .input_type
                    .as_deref()
                    .unwrap_or("text")
                    .to_ascii_lowercase();
                if matches!(
                    t.as_str(),
                    "checkbox" | "radio" | "submit" | "button" | "reset"
                ) {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
        }
    }

    fn set_control_value(
        &mut self,
        document: &SemanticDocument,
        semantic_ref: &SemanticRef,
        text: &str,
    ) -> Result<()> {
        document
            .resolve(semantic_ref)
            .map_err(|e| BrowserError::InvalidArgument(e.to_string()))?;
        self.filled
            .push((semantic_ref.as_str().to_string(), text.to_string()));
        Ok(())
    }

    fn history_availability(&mut self) -> Result<(bool, bool)> {
        Ok(self.history)
    }

    fn tab_position(&mut self) -> Result<Option<(usize, usize)>> {
        if self.open_tab_count == 0 {
            return Ok(None);
        }
        let index = self.active_tab_index.clamp(1, self.open_tab_count);
        Ok(Some((index, self.open_tab_count)))
    }
}
