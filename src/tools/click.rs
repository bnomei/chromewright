use crate::dom::{Cursor, DocumentMetadata, DomTree, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelope, ResolvedTarget, TargetEnvelope, TargetResolution, Tool, ToolContext,
    ToolResult,
    actionability::{
        ActionabilityDiagnostics, ActionabilityPredicate, ActionabilityProbeResult,
        ActionabilityRequest, probe_actionability,
    },
    resolve_target_with_cursor,
};
use headless_chrome::Tab;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CLICK_JS: &str = r#"
(() => {
  const config = __CLICK_CONFIG__;

  function getDocumentView(doc) {
    return doc.defaultView || window;
  }

  function isElementHiddenForAria(element) {
    const tagName = element.tagName;
    if (['STYLE', 'SCRIPT', 'NOSCRIPT', 'TEMPLATE'].includes(tagName)) {
      return true;
    }

    const style = getDocumentView(element.ownerDocument).getComputedStyle(element);
    if (style.visibility !== 'visible' || style.display === 'none') {
      return true;
    }

    if (element.getAttribute('aria-hidden') === 'true') {
      return true;
    }

    return false;
  }

  function isElementVisible(element) {
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  function computeBox(element) {
    const view = getDocumentView(element.ownerDocument);
    const style = view.getComputedStyle(element);
    const rect = element.getBoundingClientRect();
    return {
      rect,
      visible: rect.width > 0 && rect.height > 0,
      cursor: style.cursor
    };
  }

  function getInputRole(input) {
    const type = (input.type || 'text').toLowerCase();
    const roles = {
      button: 'button',
      checkbox: 'checkbox',
      radio: 'radio',
      range: 'slider',
      search: 'searchbox',
      text: 'textbox',
      email: 'textbox',
      tel: 'textbox',
      url: 'textbox',
      number: 'spinbutton'
    };
    return roles[type] || 'textbox';
  }

  function getAriaRole(element) {
    const explicitRole = element.getAttribute('role');
    if (explicitRole) {
      const roles = explicitRole.split(' ').map((role) => role.trim());
      if (roles[0]) {
        return roles[0];
      }
    }

    const implicitRoles = {
      BUTTON: 'button',
      A: element.hasAttribute('href') ? 'link' : null,
      INPUT: getInputRole(element),
      TEXTAREA: 'textbox',
      SELECT: element.hasAttribute('multiple') || element.size > 1 ? 'listbox' : 'combobox',
      DIALOG: 'dialog'
    };

    return implicitRoles[element.tagName] || 'generic';
  }

  function isActionableRole(role) {
    return [
      'button',
      'link',
      'textbox',
      'searchbox',
      'checkbox',
      'radio',
      'combobox',
      'listbox',
      'option',
      'menuitem',
      'menuitemcheckbox',
      'menuitemradio',
      'tab',
      'slider',
      'spinbutton',
      'switch',
      'dialog',
      'alertdialog'
    ].includes(role);
  }

  function isActionableElement(element) {
    const role = getAriaRole(element);
    const box = computeBox(element);
    return box.visible && (isActionableRole(role) || box.cursor === 'pointer');
  }

  function searchActionableIndex(targetIndex) {
    let currentIndex = 0;

    function visit(node) {
      if (!node || node.nodeType !== 1) {
        return null;
      }

      const element = node;
      const visible = !isElementHiddenForAria(element) || isElementVisible(element);
      if (!visible) {
        return null;
      }

      if (isActionableElement(element)) {
        if (currentIndex === targetIndex) {
          return element;
        }
        currentIndex += 1;
      }

      if (element.nodeName === 'SLOT') {
        for (const child of element.assignedNodes()) {
          const match = visit(child);
          if (match) {
            return match;
          }
        }
      } else {
        for (let child = element.firstChild; child; child = child.nextSibling) {
          if (!child.assignedSlot) {
            const match = visit(child);
            if (match) {
              return match;
            }
          }
        }

        if (element.shadowRoot) {
          for (let child = element.shadowRoot.firstChild; child; child = child.nextSibling) {
            const match = visit(child);
            if (match) {
              return match;
            }
          }
        }

        if (element.tagName === 'IFRAME') {
          try {
            const frameDoc = element.contentDocument;
            const frameWindow = element.contentWindow;
            if (frameDoc && frameWindow) {
              const frameRoot = frameDoc.body || frameDoc.documentElement;
              const match = visit(frameRoot);
              if (match) {
                return match;
              }
            }
          } catch (error) {
            // Cross-origin frame; actionable lookup stops at the iframe boundary.
          }
        }
      }

      return null;
    }

    const root = document.body || document.documentElement;
    return visit(root);
  }

  function querySelectorAcrossScopes(selector) {
    const visitedDocs = new Set();

    function searchRoot(root) {
      if (!root || typeof root.querySelector !== 'function') {
        return null;
      }

      let directMatch = null;
      try {
        directMatch = root.querySelector(selector);
      } catch (error) {
        return null;
      }

      if (directMatch) {
        return directMatch;
      }

      const elements = root.querySelectorAll ? root.querySelectorAll('*') : [];
      for (const element of elements) {
        if (element.shadowRoot) {
          const shadowMatch = searchRoot(element.shadowRoot);
          if (shadowMatch) {
            return shadowMatch;
          }
        }

        if (element.tagName === 'IFRAME') {
          try {
            const frameDoc = element.contentDocument;
            if (!frameDoc || visitedDocs.has(frameDoc)) {
              continue;
            }

            visitedDocs.add(frameDoc);
            const frameMatch = searchRoot(frameDoc);
            if (frameMatch) {
              return frameMatch;
            }
          } catch (error) {
            // Cross-origin frame; selector lookup stops at the iframe boundary.
          }
        }
      }

      return null;
    }

    visitedDocs.add(document);
    return searchRoot(document);
  }

  const selectorMatch = config.selector
    ? querySelectorAcrossScopes(config.selector)
    : null;
  const element = selectorMatch && selectorMatch.isConnected
    ? selectorMatch
    : typeof config.target_index === 'number'
      ? searchActionableIndex(config.target_index)
      : null;

  if (!element || !element.isConnected) {
    return JSON.stringify({
      success: false,
      code: 'target_detached',
      error: 'Element is no longer present'
    });
  }

  if (typeof element.scrollIntoView === 'function') {
    element.scrollIntoView({
      behavior: 'auto',
      block: 'center',
      inline: 'center'
    });
  }

  element.click();

  return JSON.stringify({ success: true });
})()
"#;

const TARGET_EXISTS_TEMPLATE_JS: &str = r#"
(() => {
  const config = __TARGET_EXISTS_CONFIG__;
  const selector = config.selector;
  const visitedDocs = new Set();

  function searchRoot(root) {
    if (!root || typeof root.querySelector !== 'function') {
      return false;
    }

    try {
      if (root.querySelector(selector)) {
        return true;
      }
    } catch (error) {
      return false;
    }

    const elements = root.querySelectorAll ? root.querySelectorAll('*') : [];
    for (const element of elements) {
      if (element.shadowRoot && searchRoot(element.shadowRoot)) {
        return true;
      }

      if (element.tagName === 'IFRAME') {
        try {
          const frameDoc = element.contentDocument;
          if (!frameDoc || visitedDocs.has(frameDoc)) {
            continue;
          }

          visitedDocs.add(frameDoc);
          if (searchRoot(frameDoc)) {
            return true;
          }
        } catch (error) {
          // Cross-origin frame; selector lookup stops at the iframe boundary.
        }
      }
    }

    return false;
  }

  visitedDocs.add(document);
  return JSON.stringify({ present: searchRoot(document) });
})()
"#;

const SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS: &str = r#"
(() => {
  const config = __SCROLL_TARGET_CONFIG__;

  function getDocumentView(doc) {
    return doc.defaultView || window;
  }

  function isElementHiddenForAria(element) {
    const tagName = element.tagName;
    if (['STYLE', 'SCRIPT', 'NOSCRIPT', 'TEMPLATE'].includes(tagName)) {
      return true;
    }

    const style = getDocumentView(element.ownerDocument).getComputedStyle(element);
    if (style.visibility !== 'visible' || style.display === 'none') {
      return true;
    }

    if (element.getAttribute('aria-hidden') === 'true') {
      return true;
    }

    return false;
  }

  function isElementVisible(element) {
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  function computeBox(element) {
    const view = getDocumentView(element.ownerDocument);
    const style = view.getComputedStyle(element);
    const rect = element.getBoundingClientRect();
    return {
      rect,
      visible: rect.width > 0 && rect.height > 0,
      cursor: style.cursor
    };
  }

  function getInputRole(input) {
    const type = (input.type || 'text').toLowerCase();
    const roles = {
      button: 'button',
      checkbox: 'checkbox',
      radio: 'radio',
      range: 'slider',
      search: 'searchbox',
      text: 'textbox',
      email: 'textbox',
      tel: 'textbox',
      url: 'textbox',
      number: 'spinbutton'
    };
    return roles[type] || 'textbox';
  }

  function getAriaRole(element) {
    const explicitRole = element.getAttribute('role');
    if (explicitRole) {
      const roles = explicitRole.split(' ').map((role) => role.trim());
      if (roles[0]) {
        return roles[0];
      }
    }

    const implicitRoles = {
      BUTTON: 'button',
      A: element.hasAttribute('href') ? 'link' : null,
      INPUT: getInputRole(element),
      TEXTAREA: 'textbox',
      SELECT: element.hasAttribute('multiple') || element.size > 1 ? 'listbox' : 'combobox',
      DIALOG: 'dialog'
    };

    return implicitRoles[element.tagName] || 'generic';
  }

  function isActionableRole(role) {
    return [
      'button',
      'link',
      'textbox',
      'searchbox',
      'checkbox',
      'radio',
      'combobox',
      'listbox',
      'option',
      'menuitem',
      'menuitemcheckbox',
      'menuitemradio',
      'tab',
      'slider',
      'spinbutton',
      'switch',
      'dialog',
      'alertdialog'
    ].includes(role);
  }

  function isActionableElement(element) {
    const role = getAriaRole(element);
    const box = computeBox(element);
    return box.visible && (isActionableRole(role) || box.cursor === 'pointer');
  }

  function searchActionableIndex(targetIndex) {
    let currentIndex = 0;

    function visit(node) {
      if (!node || node.nodeType !== 1) {
        return null;
      }

      const element = node;
      const visible = !isElementHiddenForAria(element) || isElementVisible(element);
      if (!visible) {
        return null;
      }

      if (isActionableElement(element)) {
        if (currentIndex === targetIndex) {
          return element;
        }
        currentIndex += 1;
      }

      if (element.nodeName === 'SLOT') {
        for (const child of element.assignedNodes()) {
          const match = visit(child);
          if (match) {
            return match;
          }
        }
      } else {
        for (let child = element.firstChild; child; child = child.nextSibling) {
          if (!child.assignedSlot) {
            const match = visit(child);
            if (match) {
              return match;
            }
          }
        }

        if (element.shadowRoot) {
          for (let child = element.shadowRoot.firstChild; child; child = child.nextSibling) {
            const match = visit(child);
            if (match) {
              return match;
            }
          }
        }

        if (element.tagName === 'IFRAME') {
          try {
            const frameDoc = element.contentDocument;
            const frameWindow = element.contentWindow;
            if (frameDoc && frameWindow) {
              const frameRoot = frameDoc.body || frameDoc.documentElement;
              const match = visit(frameRoot);
              if (match) {
                return match;
              }
            }
          } catch (error) {
            // Cross-origin frame; actionable lookup stops at the iframe boundary.
          }
        }
      }

      return null;
    }

    const root = document.body || document.documentElement;
    return visit(root);
  }

  function querySelectorAcrossScopes(selector) {
    const visitedDocs = new Set();

    function searchRoot(root) {
      if (!root || typeof root.querySelector !== 'function') {
        return null;
      }

      let directMatch = null;
      try {
        directMatch = root.querySelector(selector);
      } catch (error) {
        return null;
      }

      if (directMatch) {
        return directMatch;
      }

      const elements = root.querySelectorAll ? root.querySelectorAll('*') : [];
      for (const element of elements) {
        if (element.shadowRoot) {
          const shadowMatch = searchRoot(element.shadowRoot);
          if (shadowMatch) {
            return shadowMatch;
          }
        }

        if (element.tagName === 'IFRAME') {
          try {
            const frameDoc = element.contentDocument;
            if (!frameDoc || visitedDocs.has(frameDoc)) {
              continue;
            }

            visitedDocs.add(frameDoc);
            const frameMatch = searchRoot(frameDoc);
            if (frameMatch) {
              return frameMatch;
            }
          } catch (error) {
            // Cross-origin frame; selector lookup stops at the iframe boundary.
          }
        }
      }

      return null;
    }

    visitedDocs.add(document);
    return searchRoot(document);
  }

  const selectorMatch = config.selector
    ? querySelectorAcrossScopes(config.selector)
    : null;
  const element = selectorMatch && selectorMatch.isConnected
    ? selectorMatch
    : typeof config.target_index === 'number'
      ? searchActionableIndex(config.target_index)
      : null;

  if (!element || !element.isConnected) {
    return JSON.stringify({ scrolled: false });
  }

  if (typeof element.scrollIntoView === 'function') {
    element.scrollIntoView({
      behavior: 'auto',
      block: 'center',
      inline: 'center'
    });
  }

  return JSON.stringify({ scrolled: true });
})()
"#;

pub(crate) const DEFAULT_ACTIONABILITY_TIMEOUT_MS: u64 = 5_000;
const ACTIONABILITY_POLL_INTERVAL_MS: u64 = 50;

/// Parameters for the click tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClickParams {
    /// CSS selector (use either this or index, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Element index from DOM tree (use either this or selector, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Revision-scoped node reference from the snapshot tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    /// Cursor from the snapshot or inspect_node tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

/// Tool for clicking elements
#[derive(Default)]
pub struct ClickTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    Same,
    Rebound,
    Detached,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClickOutput {
    #[serde(flatten)]
    pub envelope: DocumentEnvelope,
    pub action: String,
    pub target_before: TargetEnvelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_after: Option<TargetEnvelope>,
    pub target_status: TargetStatus,
}

impl Tool for ClickTool {
    type Params = ClickParams;
    type Output = ClickOutput;

    fn name(&self) -> &str {
        "click"
    }

    fn execute_typed(&self, params: ClickParams, context: &mut ToolContext) -> Result<ToolResult> {
        let ClickParams {
            selector,
            index,
            node_ref,
            cursor,
        } = params;
        let target = match resolve_interaction_target(
            "click", selector, index, node_ref, cursor, context,
        )? {
            TargetResolution::Resolved(target) => target,
            TargetResolution::Failure(failure) => return Ok(failure),
        };

        let tab = context.session.tab()?;
        let predicates = click_actionability_predicates();
        match wait_for_actionability(&tab, &target, predicates, DEFAULT_ACTIONABILITY_TIMEOUT_MS)? {
            ActionabilityWaitState::Ready => {}
            ActionabilityWaitState::TimedOut(probe) => {
                return build_actionability_failure(
                    "click", &tab, &target, &probe, predicates, None,
                );
            }
        }

        let config = serde_json::json!({
            "selector": target.selector,
            "target_index": target.cursor.as_ref().map(|cursor| cursor.index).or(target.index),
        });
        let click_js = CLICK_JS.replace("__CLICK_CONFIG__", &config.to_string());
        let result =
            tab.evaluate(&click_js, false)
                .map_err(|e| BrowserError::ToolExecutionFailed {
                    tool: "click".to_string(),
                    reason: e.to_string(),
                })?;
        let action_result = decode_action_result(
            result.value,
            serde_json::json!({
                "success": false,
                "code": "target_detached",
                "error": "Element is no longer present"
            }),
        )?;

        if action_result["success"].as_bool() != Some(true) {
            let code = action_result["code"]
                .as_str()
                .unwrap_or("target_detached")
                .to_string();
            let error = action_result["error"]
                .as_str()
                .unwrap_or("Click failed")
                .to_string();
            return build_interaction_failure(
                "click",
                &tab,
                &target,
                code,
                error,
                Vec::new(),
                None,
            );
        }

        let handoff = build_interaction_handoff(context, &tab, &target)?;
        Ok(ToolResult::success_with(ClickOutput {
            envelope: handoff.envelope,
            action: "click".to_string(),
            target_before: handoff.target_before,
            target_after: handoff.target_after,
            target_status: handoff.target_status,
        }))
    }
}

pub(crate) fn resolve_interaction_target(
    tool: &str,
    selector: Option<String>,
    index: Option<usize>,
    node_ref: Option<NodeRef>,
    cursor: Option<Cursor>,
    context: &mut ToolContext,
) -> Result<TargetResolution> {
    let dom = Some(context.get_dom()?);
    resolve_target_with_cursor(tool, selector, index, node_ref, cursor, dom)
}

pub(crate) enum ActionabilityWaitState {
    Ready,
    TimedOut(ActionabilityProbeResult),
}

pub(crate) fn wait_for_actionability(
    tab: &Arc<Tab>,
    target: &ResolvedTarget,
    predicates: &[ActionabilityPredicate],
    timeout_ms: u64,
) -> Result<ActionabilityWaitState> {
    let start = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);
    let requested_predicates = requested_actionability_predicates(predicates);

    loop {
        let probe = probe_actionability(
            tab,
            &ActionabilityRequest {
                selector: &target.selector,
                target_index: interaction_target_index(target),
                predicates: requested_predicates.as_slice(),
                expected_text: None,
                expected_value: None,
            },
        )?;

        if should_scroll_target_into_view(&probe, predicates) {
            scroll_target_into_view(tab, target)?;
            std::thread::sleep(Duration::from_millis(ACTIONABILITY_POLL_INTERVAL_MS));
            continue;
        }

        if predicates
            .iter()
            .all(|predicate| probe.predicate(*predicate) == Some(true))
        {
            return Ok(ActionabilityWaitState::Ready);
        }

        if start.elapsed() >= timeout {
            return Ok(ActionabilityWaitState::TimedOut(probe));
        }

        std::thread::sleep(Duration::from_millis(ACTIONABILITY_POLL_INTERVAL_MS));
    }
}

fn requested_actionability_predicates(
    predicates: &[ActionabilityPredicate],
) -> Vec<ActionabilityPredicate> {
    let mut requested = predicates.to_vec();
    if predicates_require_viewport_scroll(predicates)
        && !requested.contains(&ActionabilityPredicate::InViewport)
    {
        requested.push(ActionabilityPredicate::InViewport);
    }
    requested
}

fn predicates_require_viewport_scroll(predicates: &[ActionabilityPredicate]) -> bool {
    predicates.iter().any(|predicate| {
        matches!(
            predicate,
            ActionabilityPredicate::ReceivesEvents | ActionabilityPredicate::UnobscuredCenter
        )
    })
}

fn should_scroll_target_into_view(
    probe: &ActionabilityProbeResult,
    predicates: &[ActionabilityPredicate],
) -> bool {
    predicates_require_viewport_scroll(predicates)
        && probe.present
        && probe.visible != Some(false)
        && probe.in_viewport == Some(false)
}

fn scroll_target_into_view(tab: &Arc<Tab>, target: &ResolvedTarget) -> Result<()> {
    let config = serde_json::json!({
        "selector": target.selector,
        "target_index": interaction_target_index(target),
    });
    let scroll_js = SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS
        .replace("__SCROLL_TARGET_CONFIG__", &config.to_string());
    tab.evaluate(&scroll_js, false)
        .map_err(|e| BrowserError::ToolExecutionFailed {
            tool: "interaction".to_string(),
            reason: e.to_string(),
        })?;
    Ok(())
}

pub(crate) struct InteractionHandoff {
    pub envelope: DocumentEnvelope,
    pub target_before: TargetEnvelope,
    pub target_after: Option<TargetEnvelope>,
    pub target_status: TargetStatus,
}

pub(crate) fn build_interaction_handoff(
    context: &mut ToolContext,
    tab: &Arc<Tab>,
    target_before: &ResolvedTarget,
) -> Result<InteractionHandoff> {
    let target_before_envelope = target_before.to_target_envelope();
    let (current_document, actionable_matches) = {
        let dom = context.refresh_dom()?;
        (
            dom.document.clone(),
            actionable_targets_for_selector(dom, &target_before.selector),
        )
    };

    let (target_after, target_status) =
        determine_target_after(tab, target_before, &current_document, actionable_matches)?;
    let legacy_target = target_after.clone();

    Ok(InteractionHandoff {
        envelope: DocumentEnvelope {
            document: current_document,
            target: legacy_target,
            snapshot: None,
            nodes: Vec::new(),
            interactive_count: None,
        },
        target_before: target_before_envelope,
        target_after,
        target_status,
    })
}

pub(crate) fn build_actionability_failure(
    tool: &str,
    tab: &Arc<Tab>,
    target: &ResolvedTarget,
    probe: &ActionabilityProbeResult,
    predicates: &[ActionabilityPredicate],
    override_code: Option<&str>,
) -> Result<ToolResult> {
    let failed_predicates = failed_predicates(probe, predicates);
    let (default_code, error) = classify_actionability_failure(probe, predicates);
    build_interaction_failure(
        tool,
        tab,
        target,
        override_code.unwrap_or(default_code).to_string(),
        error,
        failed_predicates,
        probe.diagnostics.clone(),
    )
}

pub(crate) fn build_interaction_failure(
    _tool: &str,
    tab: &Arc<Tab>,
    target: &ResolvedTarget,
    code: String,
    error: String,
    failed_predicates: Vec<String>,
    diagnostics: Option<ActionabilityDiagnostics>,
) -> Result<ToolResult> {
    let current_document = DocumentMetadata::from_tab(tab)?;
    let suggested_tool = if code == "target_detached" {
        "snapshot"
    } else {
        "inspect_node"
    };

    Ok(ToolResult::failure_with(
        error.clone(),
        serde_json::json!({
            "code": code,
            "error": error,
            "document": current_document,
            "target_before": target.to_target_envelope(),
            "failed_predicates": failed_predicates,
            "diagnostics": diagnostics,
            "recovery": {
                "suggested_tool": suggested_tool,
            }
        }),
    ))
}

pub(crate) fn decode_action_result(
    value: Option<serde_json::Value>,
    fallback: serde_json::Value,
) -> Result<serde_json::Value> {
    if let Some(serde_json::Value::String(json_str)) = value {
        serde_json::from_str(&json_str).map_err(BrowserError::from)
    } else {
        Ok(value.unwrap_or(fallback))
    }
}

fn click_actionability_predicates() -> &'static [ActionabilityPredicate] {
    &[
        ActionabilityPredicate::Present,
        ActionabilityPredicate::Visible,
        ActionabilityPredicate::Enabled,
        ActionabilityPredicate::Stable,
        ActionabilityPredicate::ReceivesEvents,
        ActionabilityPredicate::UnobscuredCenter,
    ]
}

fn interaction_target_index(target: &ResolvedTarget) -> Option<usize> {
    target
        .cursor
        .as_ref()
        .map(|cursor| cursor.index)
        .or(target.index)
}

fn failed_predicates(
    probe: &ActionabilityProbeResult,
    predicates: &[ActionabilityPredicate],
) -> Vec<String> {
    let mut failures = predicates
        .iter()
        .filter_map(|predicate| {
            (probe.predicate(*predicate) != Some(true)).then(|| predicate.key().to_string())
        })
        .collect::<Vec<_>>();

    if !probe.present && !failures.iter().any(|predicate| predicate == "present") {
        failures.insert(0, "present".to_string());
    }

    failures
}

fn classify_actionability_failure(
    probe: &ActionabilityProbeResult,
    predicates: &[ActionabilityPredicate],
) -> (&'static str, String) {
    if !probe.present {
        return ("target_detached", "Target is no longer present".to_string());
    }

    for predicate in predicates {
        match predicate {
            ActionabilityPredicate::Visible if probe.visible == Some(false) => {
                return ("target_not_visible", "Target is not visible".to_string());
            }
            ActionabilityPredicate::Enabled if probe.enabled == Some(false) => {
                return ("target_not_enabled", "Target is not enabled".to_string());
            }
            ActionabilityPredicate::Editable if probe.editable == Some(false) => {
                return ("target_not_editable", "Target is not editable".to_string());
            }
            ActionabilityPredicate::Stable if probe.stable == Some(false) => {
                return (
                    "target_not_stable",
                    "Target is not stable enough to interact with".to_string(),
                );
            }
            ActionabilityPredicate::ReceivesEvents if probe.receives_events == Some(false) => {
                return (
                    "target_obscured",
                    "Target is not receiving events".to_string(),
                );
            }
            ActionabilityPredicate::UnobscuredCenter if probe.unobscured_center == Some(false) => {
                return (
                    "target_obscured",
                    "Target is obscured at its interaction point".to_string(),
                );
            }
            _ => {}
        }
    }

    (
        "target_not_stable",
        "Target did not become ready within the bounded auto-wait window".to_string(),
    )
}

fn actionable_targets_for_selector(dom: &DomTree, selector: &str) -> Vec<Cursor> {
    dom.selectors
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            (candidate == selector).then(|| dom.cursor_for_index(index))
        })
        .flatten()
        .collect()
}

fn determine_target_after(
    tab: &Arc<Tab>,
    target_before: &ResolvedTarget,
    current_document: &DocumentMetadata,
    actionable_matches: Vec<Cursor>,
) -> Result<(Option<TargetEnvelope>, TargetStatus)> {
    let current_target = match actionable_matches.as_slice() {
        [cursor] => Some(target_envelope_from_cursor(cursor.clone())),
        _ => None,
    };

    if let Some(before_cursor) = target_before.cursor.as_ref() {
        if before_cursor.node_ref.document_id == current_document.document_id
            && before_cursor.node_ref.revision == current_document.revision
        {
            if let Some(after_target) = current_target.as_ref() {
                if after_target.node_ref == Some(before_cursor.node_ref.clone()) {
                    return Ok((Some(after_target.clone()), TargetStatus::Same));
                }
            }
        }

        if let Some(after_target) = current_target {
            return Ok((Some(after_target), TargetStatus::Rebound));
        }
    } else if let Some(after_target) = current_target {
        return Ok((Some(after_target), TargetStatus::Unknown));
    }

    if actionable_matches.len() > 1 {
        return Ok((None, TargetStatus::Unknown));
    }

    if selector_exists_across_scopes(tab, &target_before.selector)? {
        return Ok((None, TargetStatus::Unknown));
    }

    Ok((None, TargetStatus::Detached))
}

fn target_envelope_from_cursor(cursor: Cursor) -> TargetEnvelope {
    TargetEnvelope {
        method: "cursor".to_string(),
        selector: Some(cursor.selector.clone()),
        index: Some(cursor.index),
        node_ref: Some(cursor.node_ref.clone()),
        cursor: Some(cursor),
    }
}

fn selector_exists_across_scopes(tab: &Arc<Tab>, selector: &str) -> Result<bool> {
    let config = serde_json::json!({ "selector": selector });
    let exists_js =
        TARGET_EXISTS_TEMPLATE_JS.replace("__TARGET_EXISTS_CONFIG__", &config.to_string());
    let result =
        tab.evaluate(&exists_js, false)
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "interaction".to_string(),
                reason: e.to_string(),
            })?;
    let payload = decode_action_result(result.value, serde_json::json!({ "present": false }))?;
    Ok(payload["present"].as_bool().unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::{CLICK_JS, SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS};

    #[test]
    fn test_click_js_prefers_selector_before_target_index() {
        assert!(CLICK_JS.contains("const selectorMatch = config.selector"));
        assert!(CLICK_JS.contains("? querySelectorAcrossScopes(config.selector)"));
        assert!(CLICK_JS.contains("const element = selectorMatch && selectorMatch.isConnected"));
        assert!(CLICK_JS.contains("? searchActionableIndex(config.target_index)"));
    }

    #[test]
    fn test_scroll_target_into_view_js_prefers_selector_before_target_index() {
        assert!(
            SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS.contains("const selectorMatch = config.selector")
        );
        assert!(
            SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS
                .contains("? querySelectorAcrossScopes(config.selector)")
        );
        assert!(
            SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS
                .contains("const element = selectorMatch && selectorMatch.isConnected")
        );
        assert!(
            SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS
                .contains("? searchActionableIndex(config.target_index)")
        );
    }
}
