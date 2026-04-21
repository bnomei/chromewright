use crate::dom::NodeRef;
use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelopeOptions, TargetResolution, Tool, ToolContext, ToolResult,
    build_document_envelope, resolve_target,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const WAIT_NODE_STATE_JS: &str = r#"
(() => {
  const config = __WAIT_CONFIG__;
  const element = document.querySelector(config.selector);
  if (!element) {
    return JSON.stringify({
      present: false,
      visible: false,
      enabled: false,
      editable: false,
      text: '',
      value: null
    });
  }

  const rect = element.getBoundingClientRect();
  const style = window.getComputedStyle(element);
  const visible = rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
  const disabled = Boolean(element.disabled) || element.getAttribute('aria-disabled') === 'true';
  const editable = !disabled && (
    element.matches('input, textarea, select') ||
    element.isContentEditable
  );

  return JSON.stringify({
    present: true,
    visible,
    enabled: !disabled,
    editable,
    text: (element.innerText || element.textContent || '').trim(),
    value: ('value' in element) ? element.value : null
  });
})()
"#;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaitCondition {
    NavigationSettled,
    Present,
    Visible,
    Enabled,
    Editable,
    TextContains,
    ValueEquals,
    RevisionChanged,
}

fn default_condition() -> WaitCondition {
    WaitCondition::Present
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WaitParams {
    /// CSS selector to wait for
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Element index from the current DOM tree
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Revision-scoped node reference from the snapshot tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    /// Wait predicate to apply
    #[serde(default = "default_condition")]
    pub condition: WaitCondition,

    /// Expected text fragment for `text_contains`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Expected value for `value_equals`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    /// Baseline revision token for `revision_changed`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_revision: Option<String>,

    /// Timeout in milliseconds (default: 30000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    30000
}

#[derive(Default)]
pub struct WaitTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WaitOutput {
    #[serde(flatten)]
    pub envelope: crate::tools::DocumentEnvelope,
    pub action: String,
    pub condition: String,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_revision: Option<String>,
}

impl Tool for WaitTool {
    type Params = WaitParams;
    type Output = WaitOutput;

    fn name(&self) -> &str {
        "wait"
    }

    fn execute_typed(&self, params: WaitParams, context: &mut ToolContext) -> Result<ToolResult> {
        let start = Instant::now();
        let timeout = Duration::from_millis(params.timeout_ms);

        match params.condition {
            WaitCondition::NavigationSettled => {
                context
                    .session
                    .wait_for_document_ready_with_timeout(timeout)?;
                context.invalidate_dom();

                Ok(ToolResult::success_with(WaitOutput {
                    envelope: build_document_envelope(
                        context,
                        None,
                        DocumentEnvelopeOptions::minimal(),
                    )?,
                    action: "wait".to_string(),
                    condition: "navigation_settled".to_string(),
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    since_revision: None,
                }))
            }
            WaitCondition::RevisionChanged => {
                let baseline = match params.since_revision {
                    Some(revision) => revision,
                    None => context.session.document_metadata()?.revision,
                };

                loop {
                    let current_revision = context.session.document_metadata()?.revision;
                    if current_revision != baseline {
                        context.invalidate_dom();
                        return Ok(ToolResult::success_with(WaitOutput {
                            envelope: build_document_envelope(
                                context,
                                None,
                                DocumentEnvelopeOptions::minimal(),
                            )?,
                            action: "wait".to_string(),
                            condition: "revision_changed".to_string(),
                            elapsed_ms: start.elapsed().as_millis() as u64,
                            since_revision: Some(baseline),
                        }));
                    }

                    if start.elapsed() >= timeout {
                        return Err(BrowserError::Timeout(format!(
                            "Document revision did not change from '{}' within {} ms",
                            baseline, params.timeout_ms
                        )));
                    }

                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            condition => {
                let target = {
                    let dom = if params.index.is_some() || params.node_ref.is_some() {
                        Some(context.get_dom()?)
                    } else {
                        None
                    };
                    match resolve_target(
                        "wait",
                        params.selector.clone(),
                        params.index,
                        params.node_ref.clone(),
                        dom,
                    )? {
                        TargetResolution::Resolved(target) => target,
                        TargetResolution::Failure(failure) => return Ok(failure),
                    }
                };

                validate_wait_condition(
                    &condition,
                    params.text.as_deref(),
                    params.value.as_deref(),
                )?;

                loop {
                    let state = evaluate_node_state(context, &target.selector)?;
                    if condition_matches(
                        &condition,
                        &state,
                        params.text.as_deref(),
                        params.value.as_deref(),
                    ) {
                        context.invalidate_dom();
                        return Ok(ToolResult::success_with(WaitOutput {
                            envelope: build_document_envelope(
                                context,
                                Some(&target),
                                DocumentEnvelopeOptions::minimal(),
                            )?,
                            action: "wait".to_string(),
                            condition: condition_name(&condition).to_string(),
                            elapsed_ms: start.elapsed().as_millis() as u64,
                            since_revision: None,
                        }));
                    }

                    if start.elapsed() >= timeout {
                        return Err(BrowserError::Timeout(format!(
                            "Condition '{}' did not match for '{}' within {} ms",
                            condition_name(&condition),
                            target.selector,
                            params.timeout_ms
                        )));
                    }

                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
}

fn validate_wait_condition(
    condition: &WaitCondition,
    text: Option<&str>,
    value: Option<&str>,
) -> Result<()> {
    match condition {
        WaitCondition::TextContains if text.is_none() => Err(BrowserError::InvalidArgument(
            "wait.text is required when condition is 'text_contains'".to_string(),
        )),
        WaitCondition::ValueEquals if value.is_none() => Err(BrowserError::InvalidArgument(
            "wait.value is required when condition is 'value_equals'".to_string(),
        )),
        _ => Ok(()),
    }
}

fn condition_name(condition: &WaitCondition) -> &'static str {
    match condition {
        WaitCondition::NavigationSettled => "navigation_settled",
        WaitCondition::Present => "present",
        WaitCondition::Visible => "visible",
        WaitCondition::Enabled => "enabled",
        WaitCondition::Editable => "editable",
        WaitCondition::TextContains => "text_contains",
        WaitCondition::ValueEquals => "value_equals",
        WaitCondition::RevisionChanged => "revision_changed",
    }
}

fn evaluate_node_state(context: &ToolContext, selector: &str) -> Result<serde_json::Value> {
    let config = serde_json::json!({
        "selector": selector,
    });
    let js = WAIT_NODE_STATE_JS.replace("__WAIT_CONFIG__", &config.to_string());
    let result = context.session.tab()?.evaluate(&js, false).map_err(|e| {
        BrowserError::ToolExecutionFailed {
            tool: "wait".to_string(),
            reason: e.to_string(),
        }
    })?;

    if let Some(serde_json::Value::String(json_str)) = result.value {
        serde_json::from_str(&json_str).map_err(BrowserError::from)
    } else {
        Ok(result.value.unwrap_or(serde_json::Value::Null))
    }
}

fn condition_matches(
    condition: &WaitCondition,
    state: &serde_json::Value,
    expected_text: Option<&str>,
    expected_value: Option<&str>,
) -> bool {
    match condition {
        WaitCondition::NavigationSettled | WaitCondition::RevisionChanged => false,
        WaitCondition::Present => state["present"].as_bool() == Some(true),
        WaitCondition::Visible => state["visible"].as_bool() == Some(true),
        WaitCondition::Enabled => state["enabled"].as_bool() == Some(true),
        WaitCondition::Editable => state["editable"].as_bool() == Some(true),
        WaitCondition::TextContains => state["text"]
            .as_str()
            .map(|text| text.contains(expected_text.unwrap_or_default()))
            .unwrap_or(false),
        WaitCondition::ValueEquals => state["value"]
            .as_str()
            .map(|value| value == expected_value.unwrap_or_default())
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_wait_params_defaults() {
        let params: WaitParams =
            serde_json::from_value(json!({})).expect("params should deserialize");

        assert_eq!(params.condition, WaitCondition::Present);
        assert_eq!(params.timeout_ms, 30_000);
        assert!(params.selector.is_none());
        assert!(params.text.is_none());
        assert!(params.value.is_none());
    }

    #[test]
    fn test_validate_wait_condition_requires_text_and_value() {
        let text_error = validate_wait_condition(&WaitCondition::TextContains, None, None)
            .expect_err("text_contains without text should fail");
        assert!(matches!(text_error, BrowserError::InvalidArgument(_)));
        assert!(text_error.to_string().contains("wait.text"));

        let value_error = validate_wait_condition(&WaitCondition::ValueEquals, None, None)
            .expect_err("value_equals without value should fail");
        assert!(matches!(value_error, BrowserError::InvalidArgument(_)));
        assert!(value_error.to_string().contains("wait.value"));

        validate_wait_condition(&WaitCondition::Present, None, None)
            .expect("present should not require extra arguments");
        validate_wait_condition(&WaitCondition::TextContains, Some("hello"), None)
            .expect("text_contains should accept text");
        validate_wait_condition(&WaitCondition::ValueEquals, None, Some("abc"))
            .expect("value_equals should accept value");
    }

    #[test]
    fn test_condition_name_covers_all_wait_conditions() {
        let cases = [
            (WaitCondition::NavigationSettled, "navigation_settled"),
            (WaitCondition::Present, "present"),
            (WaitCondition::Visible, "visible"),
            (WaitCondition::Enabled, "enabled"),
            (WaitCondition::Editable, "editable"),
            (WaitCondition::TextContains, "text_contains"),
            (WaitCondition::ValueEquals, "value_equals"),
            (WaitCondition::RevisionChanged, "revision_changed"),
        ];

        for (condition, expected) in cases {
            assert_eq!(condition_name(&condition), expected);
        }
    }

    #[test]
    fn test_condition_matches_for_supported_wait_conditions() {
        let state = json!({
            "present": true,
            "visible": true,
            "enabled": true,
            "editable": false,
            "text": "hello world",
            "value": "expected",
        });

        assert!(condition_matches(
            &WaitCondition::Present,
            &state,
            None,
            None
        ));
        assert!(condition_matches(
            &WaitCondition::Visible,
            &state,
            None,
            None
        ));
        assert!(condition_matches(
            &WaitCondition::Enabled,
            &state,
            None,
            None
        ));
        assert!(!condition_matches(
            &WaitCondition::Editable,
            &state,
            None,
            None
        ));
        assert!(condition_matches(
            &WaitCondition::TextContains,
            &state,
            Some("hello"),
            None
        ));
        assert!(!condition_matches(
            &WaitCondition::TextContains,
            &state,
            Some("missing"),
            None
        ));
        assert!(condition_matches(
            &WaitCondition::ValueEquals,
            &state,
            None,
            Some("expected")
        ));
        assert!(!condition_matches(
            &WaitCondition::ValueEquals,
            &state,
            None,
            Some("other")
        ));
        assert!(!condition_matches(
            &WaitCondition::NavigationSettled,
            &state,
            None,
            None
        ));
        assert!(!condition_matches(
            &WaitCondition::RevisionChanged,
            &state,
            None,
            None
        ));
    }
}
