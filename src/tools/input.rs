use crate::dom::{Cursor, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelope, TargetEnvelope, TargetResolution, Tool, ToolContext, ToolResult,
    actionability::ActionabilityPredicate,
    click::{
        ActionabilityWaitState, DEFAULT_ACTIONABILITY_TIMEOUT_MS, TargetStatus,
        build_actionability_failure, build_interaction_failure, build_interaction_handoff,
        decode_action_result, resolve_interaction_target, wait_for_actionability,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const INPUT_JS: &str = r#"
(() => {
  const config = __INPUT_CONFIG__;

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

  if (typeof element.focus === 'function') {
    element.focus();
  }

  const dispatchInput = () => {
    element.dispatchEvent(new Event('input', { bubbles: true }));
    element.dispatchEvent(new Event('change', { bubbles: true }));
  };

  if ('value' in element) {
    const nextValue = config.clear ? config.text : `${element.value ?? ''}${config.text}`;
    element.value = nextValue;
    dispatchInput();
    return JSON.stringify({
      success: true,
      value: nextValue
    });
  }

  if (element.isContentEditable) {
    const nextValue = config.clear ? config.text : `${element.textContent ?? ''}${config.text}`;
    element.textContent = nextValue;
    dispatchInput();
    return JSON.stringify({
      success: true,
      value: nextValue
    });
  }

  return JSON.stringify({
    success: false,
    code: 'invalid_target',
    error: 'Element does not accept text input'
  });
})()
"#;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InputParams {
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

    /// Text to type into the element
    pub text: String,

    /// Clear existing content first (default: false)
    #[serde(default)]
    pub clear: bool,
}

#[derive(Default)]
pub struct InputTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InputOutput {
    #[serde(flatten)]
    pub envelope: DocumentEnvelope,
    pub action: String,
    pub target_before: TargetEnvelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_after: Option<TargetEnvelope>,
    pub target_status: TargetStatus,
    pub text: String,
    pub clear: bool,
}

impl Tool for InputTool {
    type Params = InputParams;
    type Output = InputOutput;

    fn name(&self) -> &str {
        "input"
    }

    fn execute_typed(&self, params: InputParams, context: &mut ToolContext) -> Result<ToolResult> {
        let InputParams {
            selector,
            index,
            node_ref,
            cursor,
            text,
            clear,
        } = params;
        let target = match resolve_interaction_target(
            "input", selector, index, node_ref, cursor, context,
        )? {
            TargetResolution::Resolved(target) => target,
            TargetResolution::Failure(failure) => return Ok(failure),
        };

        let tab = context.session.tab()?;
        let predicates = input_actionability_predicates();
        match wait_for_actionability(&tab, &target, predicates, DEFAULT_ACTIONABILITY_TIMEOUT_MS)? {
            ActionabilityWaitState::Ready => {}
            ActionabilityWaitState::TimedOut(probe) => {
                return build_actionability_failure(
                    "input", &tab, &target, &probe, predicates, None,
                );
            }
        }

        let input_config = serde_json::json!({
            "selector": target.selector,
            "target_index": target.cursor.as_ref().map(|cursor| cursor.index).or(target.index),
            "text": text,
            "clear": clear,
        });
        let input_js = INPUT_JS.replace("__INPUT_CONFIG__", &input_config.to_string());
        let result =
            tab.evaluate(&input_js, false)
                .map_err(|e| BrowserError::ToolExecutionFailed {
                    tool: "input".to_string(),
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
            return build_interaction_failure(
                "input",
                &tab,
                &target,
                action_result["code"]
                    .as_str()
                    .unwrap_or("invalid_target")
                    .to_string(),
                action_result["error"]
                    .as_str()
                    .unwrap_or("Input failed")
                    .to_string(),
                Vec::new(),
                None,
            );
        }

        let handoff = build_interaction_handoff(context, &tab, &target)?;
        Ok(ToolResult::success_with(InputOutput {
            envelope: handoff.envelope,
            action: "input".to_string(),
            target_before: handoff.target_before,
            target_after: handoff.target_after,
            target_status: handoff.target_status,
            text,
            clear,
        }))
    }
}

fn input_actionability_predicates() -> &'static [ActionabilityPredicate] {
    &[
        ActionabilityPredicate::Present,
        ActionabilityPredicate::Visible,
        ActionabilityPredicate::Enabled,
        ActionabilityPredicate::Editable,
        ActionabilityPredicate::Stable,
    ]
}

#[cfg(test)]
mod tests {
    use super::INPUT_JS;

    #[test]
    fn test_input_js_prefers_selector_before_target_index() {
        assert!(INPUT_JS.contains("const selectorMatch = config.selector"));
        assert!(INPUT_JS.contains("? querySelectorAcrossScopes(config.selector)"));
        assert!(INPUT_JS.contains("const element = selectorMatch && selectorMatch.isConnected"));
        assert!(INPUT_JS.contains("? searchActionableIndex(config.target_index)"));
    }
}
