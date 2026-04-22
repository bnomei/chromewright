use crate::error::{BrowserError, Result};
use headless_chrome::Tab;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const ACTIONABILITY_PROBE_TEMPLATE_JS: &str = r#"
(() => {
  const config = __ACTIONABILITY_CONFIG__;
  const requested = new Set(config.predicates || []);

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
      cursor: style.cursor,
      pointerEvents: style.pointerEvents
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

    function visit(node, frameDepth) {
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
          return {
            element,
            frame_depth: frameDepth
          };
        }
        currentIndex += 1;
      }

      if (element.nodeName === 'SLOT') {
        for (const child of element.assignedNodes()) {
          const match = visit(child, frameDepth);
          if (match) {
            return match;
          }
        }
      } else {
        for (let child = element.firstChild; child; child = child.nextSibling) {
          if (!child.assignedSlot) {
            const match = visit(child, frameDepth);
            if (match) {
              return match;
            }
          }
        }

        if (element.shadowRoot) {
          for (let child = element.shadowRoot.firstChild; child; child = child.nextSibling) {
            const match = visit(child, frameDepth);
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
              const match = visit(frameRoot, frameDepth + 1);
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
    return visit(root, 0);
  }

  function querySelectorAcrossScopes(selector) {
    const visitedDocs = new Set();

    function searchRoot(root, frameDepth) {
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
        return {
          element: directMatch,
          frame_depth: frameDepth
        };
      }

      const elements = root.querySelectorAll ? root.querySelectorAll('*') : [];
      for (const element of elements) {
        if (element.shadowRoot) {
          const shadowMatch = searchRoot(element.shadowRoot, frameDepth);
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
            const frameMatch = searchRoot(frameDoc, frameDepth + 1);
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
    return searchRoot(document, 0);
  }

  function resolveTarget() {
    if (config.selector) {
      const selectorMatch = querySelectorAcrossScopes(config.selector);
      if (selectorMatch && selectorMatch.element && selectorMatch.element.isConnected) {
        return selectorMatch;
      }
    }

    if (typeof config.target_index === 'number') {
      return searchActionableIndex(config.target_index);
    }

    return null;
  }

  function summarizeElement(element) {
    if (!element) {
      return null;
    }

    const classes = typeof element.className === 'string'
      ? element.className.split(/\s+/).map((value) => value.trim()).filter(Boolean)
      : [];

    return {
      tag: element.tagName.toLowerCase(),
      id: element.id || null,
      classes
    };
  }

  function setPredicate(result, key, value) {
    if (requested.has(key)) {
      result[key] = value;
    }
  }

  const result = {
    present: false,
    visible: null,
    enabled: null,
    editable: null,
    stable: null,
    receives_events: null,
    in_viewport: null,
    unobscured_center: null,
    text_contains: null,
    value_equals: null,
    frame_depth: null,
    diagnostics: null
  };

  const match = resolveTarget();
  if (!match || !match.element || !match.element.isConnected) {
    return JSON.stringify(result);
  }

  const element = match.element;
  const frameDepth = match.frame_depth || 0;
  const diagnostics = {};
  result.present = true;
  result.frame_depth = frameDepth;

  const needsLayout =
    requested.has('visible') ||
    requested.has('stable') ||
    requested.has('receives_events') ||
    requested.has('in_viewport') ||
    requested.has('unobscured_center');
  const needsDisabled = requested.has('enabled') || requested.has('editable');
  const needsText = requested.has('text_contains');
  const needsValue = requested.has('value_equals');

  let rect = null;
  let style = null;
  if (needsLayout) {
    rect = element.getBoundingClientRect();
    style = getDocumentView(element.ownerDocument).getComputedStyle(element);
    diagnostics.pointer_events = style.pointerEvents;
  }

  let disabled = null;
  if (needsDisabled) {
    disabled = Boolean(element.disabled) || element.getAttribute('aria-disabled') === 'true';
  }

  if (requested.has('visible')) {
    setPredicate(
      result,
      'visible',
      rect.width > 0 &&
        rect.height > 0 &&
        style.visibility !== 'hidden' &&
        style.display !== 'none'
    );
  }

  if (requested.has('enabled')) {
    setPredicate(result, 'enabled', !disabled);
  }

  if (requested.has('editable')) {
    setPredicate(
      result,
      'editable',
      !disabled && (
        element.matches('input, textarea, select') ||
        element.isContentEditable
      )
    );
  }

  if (requested.has('in_viewport')) {
    const view = getDocumentView(element.ownerDocument);
    setPredicate(
      result,
      'in_viewport',
      rect.bottom > 0 &&
        rect.right > 0 &&
        rect.top < view.innerHeight &&
        rect.left < view.innerWidth
    );
  }

  if (requested.has('stable')) {
    const nextRect = element.getBoundingClientRect();
    setPredicate(
      result,
      'stable',
      Math.abs(rect.x - nextRect.x) < 0.5 &&
        Math.abs(rect.y - nextRect.y) < 0.5 &&
        Math.abs(rect.width - nextRect.width) < 0.5 &&
        Math.abs(rect.height - nextRect.height) < 0.5
    );
  }

  if (requested.has('receives_events') || requested.has('unobscured_center')) {
    const centerX = rect.left + rect.width / 2;
    const centerY = rect.top + rect.height / 2;
    const hitTarget = style.pointerEvents === 'none'
      ? null
      : element.ownerDocument.elementFromPoint(centerX, centerY);
    const receivesEvents = Boolean(hitTarget) && (
      hitTarget === element ||
      element.contains(hitTarget) ||
      hitTarget.contains(element)
    );

    diagnostics.hit_target = summarizeElement(hitTarget);
    setPredicate(result, 'receives_events', receivesEvents);
    setPredicate(result, 'unobscured_center', receivesEvents);
  }

  if (needsText) {
    const text = (element.innerText || element.textContent || '').trim();
    diagnostics.text_length = text.length;
    setPredicate(result, 'text_contains', text.includes(config.text || ''));
  }

  if (needsValue) {
    const value = ('value' in element) ? element.value : null;
    diagnostics.has_value = value !== null;
    setPredicate(result, 'value_equals', value === config.value);
  }

  result.diagnostics = Object.keys(diagnostics).length > 0 ? diagnostics : null;
  return JSON.stringify(result);
})()
"#;

// T001 stages the broader interaction predicate set here so later tasks can
// reuse the same probe without introducing a second readiness model.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ActionabilityPredicate {
    Present,
    Visible,
    Enabled,
    Editable,
    Stable,
    ReceivesEvents,
    InViewport,
    UnobscuredCenter,
    TextContains,
    ValueEquals,
}

impl ActionabilityPredicate {
    pub const fn key(self) -> &'static str {
        match self {
            ActionabilityPredicate::Present => "present",
            ActionabilityPredicate::Visible => "visible",
            ActionabilityPredicate::Enabled => "enabled",
            ActionabilityPredicate::Editable => "editable",
            ActionabilityPredicate::Stable => "stable",
            ActionabilityPredicate::ReceivesEvents => "receives_events",
            ActionabilityPredicate::InViewport => "in_viewport",
            ActionabilityPredicate::UnobscuredCenter => "unobscured_center",
            ActionabilityPredicate::TextContains => "text_contains",
            ActionabilityPredicate::ValueEquals => "value_equals",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ActionabilityRequest<'a> {
    pub selector: &'a str,
    pub target_index: Option<usize>,
    pub predicates: &'a [ActionabilityPredicate],
    pub expected_text: Option<&'a str>,
    pub expected_value: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub(crate) struct ActionabilityProbeResult {
    pub present: bool,
    pub visible: Option<bool>,
    pub enabled: Option<bool>,
    pub editable: Option<bool>,
    pub stable: Option<bool>,
    pub receives_events: Option<bool>,
    pub in_viewport: Option<bool>,
    pub unobscured_center: Option<bool>,
    pub text_contains: Option<bool>,
    pub value_equals: Option<bool>,
    pub frame_depth: Option<usize>,
    pub diagnostics: Option<ActionabilityDiagnostics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub(crate) struct ActionabilityDiagnostics {
    pub pointer_events: Option<String>,
    pub hit_target: Option<ActionabilityElementSummary>,
    pub text_length: Option<usize>,
    pub has_value: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub(crate) struct ActionabilityElementSummary {
    pub tag: String,
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
}

impl ActionabilityProbeResult {
    pub(crate) fn predicate(&self, predicate: ActionabilityPredicate) -> Option<bool> {
        match predicate {
            ActionabilityPredicate::Present => Some(self.present),
            ActionabilityPredicate::Visible => self.visible,
            ActionabilityPredicate::Enabled => self.enabled,
            ActionabilityPredicate::Editable => self.editable,
            ActionabilityPredicate::Stable => self.stable,
            ActionabilityPredicate::ReceivesEvents => self.receives_events,
            ActionabilityPredicate::InViewport => self.in_viewport,
            ActionabilityPredicate::UnobscuredCenter => self.unobscured_center,
            ActionabilityPredicate::TextContains => self.text_contains,
            ActionabilityPredicate::ValueEquals => self.value_equals,
        }
    }
}

pub(crate) fn build_actionability_probe_js(request: &ActionabilityRequest<'_>) -> String {
    let config = serde_json::json!({
        "selector": request.selector,
        "target_index": request.target_index,
        "predicates": request
            .predicates
            .iter()
            .map(|predicate| predicate.key())
            .collect::<Vec<_>>(),
        "text": request.expected_text,
        "value": request.expected_value,
    });

    ACTIONABILITY_PROBE_TEMPLATE_JS.replace("__ACTIONABILITY_CONFIG__", &config.to_string())
}

pub(crate) fn probe_actionability(
    tab: &Arc<Tab>,
    request: &ActionabilityRequest<'_>,
) -> Result<ActionabilityProbeResult> {
    let js = build_actionability_probe_js(request);
    let result = tab
        .evaluate(&js, false)
        .map_err(|e| BrowserError::ToolExecutionFailed {
            tool: "actionability".to_string(),
            reason: e.to_string(),
        })?;

    if let Some(serde_json::Value::String(json_str)) = result.value {
        serde_json::from_str(&json_str).map_err(BrowserError::from)
    } else {
        serde_json::from_value(result.value.unwrap_or(serde_json::Value::Null))
            .map_err(BrowserError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_build_actionability_probe_js_keeps_wait_predicates_narrow() {
        let present_js = build_actionability_probe_js(&ActionabilityRequest {
            selector: "#node",
            target_index: None,
            predicates: &[ActionabilityPredicate::Present],
            expected_text: None,
            expected_value: None,
        });
        let present_config = extract_embedded_config(&present_js);
        assert_eq!(present_config["predicates"], serde_json::json!(["present"]));
        assert_eq!(present_config["text"], Value::Null);
        assert_eq!(present_config["value"], Value::Null);

        let visible_js = build_actionability_probe_js(&ActionabilityRequest {
            selector: "#node",
            target_index: None,
            predicates: &[ActionabilityPredicate::Visible],
            expected_text: None,
            expected_value: None,
        });
        let visible_config = extract_embedded_config(&visible_js);
        assert_eq!(visible_config["predicates"], serde_json::json!(["visible"]));
        assert_eq!(visible_config["text"], Value::Null);
        assert_eq!(visible_config["value"], Value::Null);

        let text_js = build_actionability_probe_js(&ActionabilityRequest {
            selector: "#node",
            target_index: None,
            predicates: &[ActionabilityPredicate::TextContains],
            expected_text: Some("hello"),
            expected_value: None,
        });
        let text_config = extract_embedded_config(&text_js);
        assert_eq!(
            text_config["predicates"],
            serde_json::json!(["text_contains"])
        );
        assert_eq!(text_config["text"].as_str(), Some("hello"));
        assert_eq!(text_config["value"], Value::Null);

        let value_js = build_actionability_probe_js(&ActionabilityRequest {
            selector: "#node",
            target_index: None,
            predicates: &[ActionabilityPredicate::ValueEquals],
            expected_text: None,
            expected_value: Some("expected"),
        });
        let value_config = extract_embedded_config(&value_js);
        assert_eq!(
            value_config["predicates"],
            serde_json::json!(["value_equals"])
        );
        assert_eq!(value_config["text"], Value::Null);
        assert_eq!(value_config["value"].as_str(), Some("expected"));
    }

    #[test]
    fn test_build_actionability_probe_js_supports_interaction_predicates_and_target_index() {
        let js = build_actionability_probe_js(&ActionabilityRequest {
            selector: "#inside",
            target_index: Some(4),
            predicates: &[
                ActionabilityPredicate::Visible,
                ActionabilityPredicate::Enabled,
                ActionabilityPredicate::Stable,
                ActionabilityPredicate::ReceivesEvents,
                ActionabilityPredicate::InViewport,
                ActionabilityPredicate::UnobscuredCenter,
            ],
            expected_text: None,
            expected_value: None,
        });

        let config = extract_embedded_config(&js);
        assert_eq!(config["selector"].as_str(), Some("#inside"));
        assert_eq!(config["target_index"].as_u64(), Some(4));
        assert!(js.contains("searchActionableIndex"));
        assert!(js.contains("querySelectorAcrossScopes"));
        assert!(js.contains("const selectorMatch = querySelectorAcrossScopes(config.selector);"));
        assert!(js.contains("return selectorMatch;"));
        assert!(js.contains("element.ownerDocument.elementFromPoint"));
        assert!(js.contains("hit_target"));
    }

    #[test]
    fn test_actionability_probe_result_reports_requested_predicates() {
        let result = ActionabilityProbeResult {
            present: true,
            visible: Some(true),
            enabled: Some(false),
            editable: Some(false),
            stable: Some(true),
            receives_events: Some(false),
            in_viewport: Some(true),
            unobscured_center: Some(false),
            text_contains: Some(true),
            value_equals: Some(false),
            frame_depth: Some(1),
            diagnostics: None,
        };

        assert_eq!(
            result.predicate(ActionabilityPredicate::Present),
            Some(true)
        );
        assert_eq!(
            result.predicate(ActionabilityPredicate::Visible),
            Some(true)
        );
        assert_eq!(
            result.predicate(ActionabilityPredicate::Enabled),
            Some(false)
        );
        assert_eq!(result.predicate(ActionabilityPredicate::Stable), Some(true));
        assert_eq!(
            result.predicate(ActionabilityPredicate::ReceivesEvents),
            Some(false)
        );
        assert_eq!(
            result.predicate(ActionabilityPredicate::TextContains),
            Some(true)
        );
    }

    fn extract_embedded_config(js: &str) -> Value {
        let marker = "const config = ";
        let start = js
            .find(marker)
            .map(|index| index + marker.len())
            .expect("config marker should exist");
        let end = js[start..]
            .find(';')
            .map(|offset| start + offset)
            .expect("config assignment should end with a semicolon");
        serde_json::from_str(&js[start..end]).expect("embedded config should be valid JSON")
    }
}
