//! Browser session bridge used by the TUI controller.
//!
//! Production uses [`SessionPageDriver`] over a shared [`BrowserSession`].
//! Tests inject [`FakePageDriver`] for lifecycle atomicity without Chrome.

use crate::browser::BrowserSession;
use crate::dom::DocumentMetadata;
use crate::error::{BrowserError, Result};
use crate::semantic::{SemanticDocument, SemanticKind, SemanticRef};
use crate::tools::utils::validate_navigation_url;
use std::time::Duration;

/// Page-mutating and capture operations the controller needs.
pub trait PageDriver {
    fn navigate(&mut self, url: &str) -> Result<()>;
    fn go_back(&mut self) -> Result<()>;
    fn go_forward(&mut self) -> Result<()>;
    fn reload(&mut self) -> Result<()>;
    fn wait_settle(&mut self) -> Result<()>;
    fn capture_semantic(&mut self) -> Result<SemanticDocument>;
    fn document_metadata(&mut self) -> Result<DocumentMetadata>;
    fn open_tab(&mut self, url: &str) -> Result<()>;
    fn close_active_tab(&mut self) -> Result<()>;
    fn next_tab(&mut self) -> Result<()>;
    fn prev_tab(&mut self) -> Result<()>;
    /// Activate a link or control identified by an exact current-document semantic_ref.
    fn activate_ref(
        &mut self,
        document: &SemanticDocument,
        semantic_ref: &SemanticRef,
        new_tab: bool,
    ) -> Result<bool>;
    /// Submit text into an input/textarea/select by exact semantic_ref.
    fn fill_control(
        &mut self,
        document: &SemanticDocument,
        semantic_ref: &SemanticRef,
        text: &str,
    ) -> Result<bool>;
    /// Availability belongs to the active browser tab. Implementations that
    /// cannot determine it must return `(false, false)` rather than inventing
    /// local history.
    fn history_availability(&mut self) -> Result<(bool, bool)> {
        Ok((false, false))
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
        // Invalidate caches via navigate-equivalent path.
        let _ = self.session.document_metadata();
        Ok(())
    }

    fn wait_settle(&mut self) -> Result<()> {
        // Some valid actions (focus, same-document controls, and form edits)
        // intentionally do not create a navigation event. Settling therefore
        // uses the active document's readiness barrier rather than issuing a
        // navigation wait and silently discarding its failure.
        self.session
            .wait_for_document_ready_with_timeout(Duration::from_secs(15))?;
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
        Ok(())
    }

    fn close_active_tab(&mut self) -> Result<()> {
        self.session.close_active_tab()?;
        Ok(())
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
                    let target = resolved_link_target(self.session, document, component)?;
                    self.open_tab(&target)?;
                } else {
                    // A real click preserves fragment/query resolution and page-defined
                    // click semantics. The locator is derived from the exact ref only.
                    click_component(self.session, document, component, true)?;
                }
                Ok(true)
            }
            SemanticKind::Button => {
                click_component(self.session, document, component, false)?;
                Ok(true)
            }
            SemanticKind::Input | SemanticKind::Textarea | SemanticKind::Select => {
                // Focusing a control is not page-changing by itself.
                focus_component(self.session, document, component)?;
                Ok(false)
            }
            _ => Err(BrowserError::InvalidArgument(
                "component is not activatable".into(),
            )),
        }
    }

    fn fill_control(
        &mut self,
        document: &SemanticDocument,
        semantic_ref: &SemanticRef,
        text: &str,
    ) -> Result<bool> {
        let component = document
            .resolve(semantic_ref)
            .map_err(|e| BrowserError::InvalidArgument(e.to_string()))?;
        match component.kind {
            SemanticKind::Input | SemanticKind::Textarea => {
                set_value_and_submit(self.session, document, component, text)?;
                // Enter may submit; treat as potentially page-changing when type is submit
                // or form has default submit — controller always recaptures after fill confirm.
                Ok(true)
            }
            SemanticKind::Select => {
                select_value(self.session, document, component, text)?;
                Ok(true)
            }
            SemanticKind::Button => {
                click_component(self.session, document, component, false)?;
                Ok(true)
            }
            _ => Err(BrowserError::InvalidArgument(
                "component is not a fillable control".into(),
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

#[allow(dead_code)]
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

fn exact_dom_id(component: &crate::semantic::SemanticComponent) -> Result<String> {
    component
        .semantic_ref
        .author_id()
        .map_err(|e| BrowserError::InvalidArgument(e.to_string()))?
        .ok_or_else(|| BrowserError::InvalidArgument("semantic ref has no exact DOM id".into()))
}

fn exact_target_prelude(
    document: &SemanticDocument,
    component: &crate::semantic::SemanticComponent,
) -> Result<String> {
    let id = exact_dom_id(component)?;
    let expected_tag = component
        .attrs
        .tag
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase();
    Ok(format!(
        r#"const state = globalThis.__browserUseDocumentState;
            if (!state || state.documentId !== {document_id} || String(state.revision) !== {revision}) return 'stale';
            const matches = Array.from(document.querySelectorAll('[id]')).filter((candidate) => candidate.id === {id});
            if (matches.length !== 1) return 'ambiguous';
            const el = matches[0];
            if ({expected_tag} && el.tagName !== {expected_tag}) return 'kind_mismatch';"#,
        document_id = js_string(&document.document.document_id),
        revision = js_string(&document.document.revision),
        id = js_string(&id),
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
        r#"(function(){{ {prelude} el.focus(); return document.activeElement === el ? 'ok' : 'missing'; }})()"#
    );
    ensure_target_result(session.evaluate(&script, false)?)
}

fn set_value_and_submit(
    session: &BrowserSession,
    document: &SemanticDocument,
    component: &crate::semantic::SemanticComponent,
    text: &str,
) -> Result<()> {
    let prelude = exact_target_prelude(document, component)?;
    let script = format!(
        r#"(function(){{
            {prelude}
            if (el.readOnly || el.disabled) return 'readonly';
            el.focus();
            if (el.type === 'checkbox' || el.type === 'radio') {{
                el.click();
                return 'ok';
            }}
            el.value = {text};
            el.dispatchEvent(new Event('input', {{ bubbles: true }}));
            el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            if (el.form) {{
                if (typeof el.form.requestSubmit === 'function') el.form.requestSubmit();
                else el.form.submit();
            }}
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

/// Scripted fake driver for unit tests.
#[derive(Debug, Default)]
pub struct FakePageDriver {
    pub pages: Vec<SemanticDocument>,
    pub page_index: usize,
    pub navigate_calls: Vec<String>,
    pub back_calls: usize,
    pub forward_calls: usize,
    pub reload_calls: usize,
    pub capture_calls: usize,
    pub fail_next: Option<String>,
    pub fail_capture: Option<String>,
    pub activated: Vec<(String, bool)>,
    pub filled: Vec<(String, String)>,
    pub open_tabs: Vec<String>,
    pub tab_ops: Vec<&'static str>,
    pub history: (bool, bool),
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
        if self.page_index + 1 < self.pages.len() {
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
        self.current()
    }

    fn document_metadata(&mut self) -> Result<DocumentMetadata> {
        Ok(self.current()?.document)
    }

    fn open_tab(&mut self, url: &str) -> Result<()> {
        self.open_tabs.push(url.to_string());
        Ok(())
    }

    fn close_active_tab(&mut self) -> Result<()> {
        self.tab_ops.push("close");
        Ok(())
    }

    fn next_tab(&mut self) -> Result<()> {
        self.tab_ops.push("next");
        Ok(())
    }

    fn prev_tab(&mut self) -> Result<()> {
        self.tab_ops.push("prev");
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
        if component.kind == SemanticKind::Link {
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
        } else {
            Ok(false)
        }
    }

    fn fill_control(
        &mut self,
        document: &SemanticDocument,
        semantic_ref: &SemanticRef,
        text: &str,
    ) -> Result<bool> {
        document
            .resolve(semantic_ref)
            .map_err(|e| BrowserError::InvalidArgument(e.to_string()))?;
        self.filled
            .push((semantic_ref.as_str().to_string(), text.to_string()));
        Ok(true)
    }

    fn history_availability(&mut self) -> Result<(bool, bool)> {
        Ok(self.history)
    }
}
