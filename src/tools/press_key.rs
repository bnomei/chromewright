use crate::dom::{AriaChild, AriaNode, Cursor, DomTree};
use crate::error::{BrowserError, Result};
use crate::tools::{DocumentEnvelope, Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the press_key tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PressKeyParams {
    /// Name of the key to press (e.g., "Enter", "Tab", "Escape", "ArrowDown", "F1", etc.)
    pub key: String,
}

/// Tool for pressing keyboard keys
#[derive(Default)]
pub struct PressKeyTool;

const FOCUS_AFTER_JS: &str = r#"
(() => {
  function normalizeWhitespace(text) {
    return String(text || '').replace(/\s+/g, ' ').trim();
  }

  function getDeepestActiveElement(root) {
    if (!root || typeof root.activeElement === 'undefined') {
      return null;
    }

    let active = root.activeElement;
    if (!active) {
      return null;
    }

    while (active && active.shadowRoot && active.shadowRoot.activeElement) {
      active = active.shadowRoot.activeElement;
    }

    if (active && active.tagName === 'IFRAME') {
      try {
        const frameDoc = active.contentDocument;
        if (frameDoc) {
          const frameActive = getDeepestActiveElement(frameDoc);
          if (frameActive) {
            return frameActive;
          }
        }
      } catch (error) {
        // Cross-origin frame; keep the iframe as the best available focus hint.
      }
    }

    return active;
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
      DIALOG: 'dialog',
      IFRAME: 'iframe'
    };

    return implicitRoles[element.tagName] || 'generic';
  }

  function getAccessibleName(element) {
    const doc = element.ownerDocument;

    const ariaLabel = element.getAttribute('aria-label');
    if (ariaLabel) {
      return ariaLabel;
    }

    const labelledBy = element.getAttribute('aria-labelledby');
    if (labelledBy) {
      const ids = labelledBy.split(/\s+/);
      const texts = ids
        .map((id) => {
          const labelled = doc.getElementById(id);
          return labelled ? labelled.textContent : '';
        })
        .filter(Boolean);
      if (texts.length > 0) {
        return texts.join(' ');
      }
    }

    if (['INPUT', 'TEXTAREA', 'SELECT'].includes(element.tagName)) {
      const id = element.id;
      if (id) {
        const label = doc.querySelector('label[for="' + id + '"]');
        if (label) {
          return label.textContent || '';
        }
      }

      const parentLabel = element.closest('label');
      if (parentLabel) {
        return parentLabel.textContent || '';
      }
    }

    const title = element.getAttribute('title');
    if (title) {
      return title;
    }

    if (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA') {
      const placeholder = element.getAttribute('placeholder');
      if (placeholder) {
        return placeholder;
      }
    }

    if (element.tagName === 'A' || element.tagName === 'BUTTON') {
      const text = element.textContent || '';
      if (text.trim()) {
        return text.trim();
      }
    }

    return '';
  }

  const active = getDeepestActiveElement(document);
  if (!active || ['BODY', 'HTML'].includes(active.tagName)) {
    return null;
  }

  const name = normalizeWhitespace(getAccessibleName(active));
  return {
    tag: active.tagName.toLowerCase(),
    role: getAriaRole(active),
    ...(name ? { name } : {})
  };
})()
"#;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FocusAfter {
    Cursor {
        cursor: Cursor,
    },
    Summary {
        tag: String,
        role: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct FocusSummary {
    tag: String,
    role: String,
    name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PressKeyOutput {
    #[serde(flatten)]
    pub envelope: DocumentEnvelope,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_after: Option<FocusAfter>,
}

impl Tool for PressKeyTool {
    type Params = PressKeyParams;
    type Output = PressKeyOutput;

    fn name(&self) -> &str {
        "press_key"
    }

    fn description(&self) -> &str {
        "Press a keyboard key. Returns focus hints; snapshot only for broader rereads."
    }

    fn execute_typed(
        &self,
        params: PressKeyParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        context.session.press_key(&params.key)?;

        context.invalidate_dom();

        let mut document = None;
        let mut focus_after = None;

        if let Ok(dom) = context.refresh_dom() {
            document = Some(dom.document.clone());
            focus_after = active_cursor(dom).map(|cursor| FocusAfter::Cursor { cursor });
        }

        if focus_after.is_none() {
            focus_after =
                read_focus_summary(context)
                    .ok()
                    .flatten()
                    .map(|summary| FocusAfter::Summary {
                        tag: summary.tag,
                        role: summary.role,
                        name: summary.name,
                    });
        }

        let envelope = DocumentEnvelope {
            document: match document {
                Some(document) => document,
                None => {
                    context.record_browser_evaluation();
                    context.session.document_metadata()?
                }
            },
            target: None,
            snapshot: None,
            nodes: Vec::new(),
            interactive_count: None,
        };

        Ok(context.finish(ToolResult::success_with(PressKeyOutput {
            envelope,
            key: params.key,
            focus_after,
        })))
    }
}

fn active_cursor(dom: &DomTree) -> Option<Cursor> {
    let index = deepest_active_actionable_index(&dom.root)?;
    dom.cursor_for_index(index)
}

fn deepest_active_actionable_index(node: &AriaNode) -> Option<usize> {
    for child in &node.children {
        if let AriaChild::Node(child_node) = child {
            if let Some(index) = deepest_active_actionable_index(child_node) {
                return Some(index);
            }
        }
    }

    (node.active == Some(true)).then_some(node.index).flatten()
}

fn read_focus_summary(context: &mut ToolContext) -> Result<Option<FocusSummary>> {
    context.record_browser_evaluation();
    let result = context
        .session
        .evaluate(FOCUS_AFTER_JS, false)
        .map_err(|e| match e {
            BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
                tool: "press_key".to_string(),
                reason,
            },
            other => other,
        })?;

    Ok(parse_focus_summary(result.value))
}

fn parse_focus_summary(value: Option<serde_json::Value>) -> Option<FocusSummary> {
    match value {
        Some(serde_json::Value::String(json_str)) => {
            serde_json::from_str::<Option<FocusSummary>>(&json_str)
                .ok()
                .flatten()
        }
        Some(other) => serde_json::from_value::<Option<FocusSummary>>(other)
            .ok()
            .flatten(),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_press_key_tool_metadata() {
        let tool = PressKeyTool;
        assert_eq!(tool.name(), "press_key");
        let schema = tool.parameters_schema();
        assert!(schema.is_object());
    }

    #[test]
    fn test_press_key_params_various_keys() {
        let test_keys = vec![
            "Enter",
            "Tab",
            "Escape",
            "Backspace",
            "Delete",
            "ArrowLeft",
            "ArrowRight",
            "ArrowUp",
            "ArrowDown",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "F1",
            "F12",
            "ShiftLeft",
            "MetaLeft",
            "Space",
        ];

        for key in test_keys {
            let json = serde_json::json!({ "key": key });
            let params: PressKeyParams = serde_json::from_value(json).unwrap();
            assert_eq!(params.key, key);
        }
    }

    #[test]
    fn test_active_cursor_prefers_deepest_focused_actionable_node() {
        let mut iframe = AriaNode::new("iframe", "").with_box(true, None);
        iframe.active = Some(true);

        let mut textbox = AriaNode::new("textbox", "Search")
            .with_box(true, None)
            .with_index(0);
        textbox.active = Some(true);

        iframe.children.push(AriaChild::Node(Box::new(textbox)));

        let root = AriaNode::fragment().with_children(vec![AriaChild::Node(Box::new(iframe))]);
        let mut dom = DomTree::new(root);
        dom.document.document_id = "doc-1".to_string();
        dom.document.revision = "rev-1".to_string();
        dom.replace_selectors(vec!["form > input.search".to_string()]);

        let cursor = active_cursor(&dom).expect("active cursor should resolve");
        assert_eq!(cursor.selector, "form > input.search");
        assert_eq!(cursor.role, "textbox");
        assert_eq!(cursor.name, "Search");
        assert_eq!(cursor.node_ref.document_id, "doc-1");
        assert_eq!(cursor.node_ref.revision, "rev-1");
    }

    #[test]
    fn test_parse_focus_summary_accepts_object_payload() {
        let summary = parse_focus_summary(Some(serde_json::json!({
            "tag": "input",
            "role": "textbox",
            "name": "Search"
        })))
        .expect("summary should parse");

        assert_eq!(
            summary,
            FocusSummary {
                tag: "input".to_string(),
                role: "textbox".to_string(),
                name: Some("Search".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_focus_summary_treats_null_string_as_absent() {
        assert_eq!(
            parse_focus_summary(Some(serde_json::Value::String("null".to_string()))),
            None
        );
    }

    #[test]
    fn test_press_key_output_serializes_discriminated_focus_after_shape() {
        let output = PressKeyOutput {
            envelope: DocumentEnvelope {
                document: crate::dom::DocumentMetadata::default(),
                target: None,
                snapshot: None,
                nodes: Vec::new(),
                interactive_count: None,
            },
            key: "Tab".to_string(),
            focus_after: Some(FocusAfter::Summary {
                tag: "input".to_string(),
                role: "textbox".to_string(),
                name: Some("Search".to_string()),
            }),
        };

        let value = serde_json::to_value(output).expect("press_key output should serialize");
        assert_eq!(value["focus_after"]["kind"], serde_json::json!("summary"));
        assert_eq!(value["focus_after"]["tag"], serde_json::json!("input"));
        assert_eq!(value["focus_after"]["role"], serde_json::json!("textbox"));
        assert_eq!(value["focus_after"]["name"], serde_json::json!("Search"));
    }
}
