use crate::dom::{Cursor, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelope, TargetEnvelope, TargetResolution, Tool, ToolContext, ToolResult,
    actionability::ActionabilityPredicate,
    browser_kernel::render_browser_kernel_script,
    services::interaction::{
        ActionabilityWaitState, DEFAULT_ACTIONABILITY_TIMEOUT_MS, TargetStatus,
        build_actionability_failure, build_interaction_failure, build_interaction_handoff,
        decode_action_result, resolve_interaction_target, wait_for_actionability,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the select tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SelectParams {
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

    /// Value to select in the dropdown
    pub value: String,
}

/// Tool for selecting dropdown options
#[derive(Default)]
pub struct SelectTool;

const SELECT_JS: &str = include_str!("select.js");

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SelectOutput {
    #[serde(flatten)]
    pub envelope: DocumentEnvelope,
    pub action: String,
    pub target_before: TargetEnvelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_after: Option<TargetEnvelope>,
    pub target_status: TargetStatus,
    pub value: String,
    #[serde(rename = "selectedText")]
    pub selected_text: Option<String>,
}

impl Tool for SelectTool {
    type Params = SelectParams;
    type Output = SelectOutput;

    fn name(&self) -> &str {
        "select"
    }

    fn description(&self) -> &str {
        "Choose a dropdown value. Usually after snapshot; next wait or snapshot."
    }

    fn execute_typed(&self, params: SelectParams, context: &mut ToolContext) -> Result<ToolResult> {
        let SelectParams {
            selector,
            index,
            node_ref,
            cursor,
            value,
        } = params;
        let target =
            match resolve_interaction_target("select", selector, index, node_ref, cursor, context)?
            {
                TargetResolution::Resolved(target) => target,
                TargetResolution::Failure(failure) => return Ok(context.finish(failure)),
            };

        let predicates = select_actionability_predicates();
        match wait_for_actionability(
            context,
            &target,
            predicates,
            DEFAULT_ACTIONABILITY_TIMEOUT_MS,
        )? {
            ActionabilityWaitState::Ready => {}
            ActionabilityWaitState::TimedOut(probe) => {
                return build_actionability_failure(
                    "select",
                    context.session,
                    &target,
                    &probe,
                    predicates,
                    None,
                )
                .map(|result| context.finish(result));
            }
        }

        let select_config = serde_json::json!({
            "selector": target.selector,
            "target_index": target.cursor.as_ref().map(|cursor| cursor.index).or(target.index),
            "value": value,
        });
        let select_js = build_select_js(&select_config);

        context.record_browser_evaluation();
        let result = context
            .session
            .evaluate(&select_js, false)
            .map_err(|e| match e {
                BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
                    tool: "select".to_string(),
                    reason,
                },
                other => other,
            })?;

        match parse_select_result(result.value)? {
            SelectParseResult::Success(selected_text) => {
                let handoff = build_interaction_handoff(context, &target)?;
                Ok(context.finish(ToolResult::success_with(SelectOutput {
                    envelope: handoff.envelope,
                    action: "select".to_string(),
                    target_before: handoff.target_before,
                    target_after: handoff.target_after,
                    target_status: handoff.target_status,
                    value,
                    selected_text,
                })))
            }
            SelectParseResult::Failure { code, error } => build_interaction_failure(
                "select",
                context.session,
                &target,
                code,
                error,
                Vec::new(),
                None,
            )
            .map(|result| context.finish(result)),
        }
    }
}

fn build_select_js(config: &serde_json::Value) -> String {
    render_browser_kernel_script(SELECT_JS, "__SELECT_CONFIG__", config)
}

enum SelectParseResult {
    Success(Option<String>),
    Failure { code: String, error: String },
}

fn parse_select_result(value: Option<serde_json::Value>) -> Result<SelectParseResult> {
    let result_json = decode_action_result(
        value,
        serde_json::json!({
            "success": false,
            "code": "target_detached",
            "error": "Element is no longer present"
        }),
    )?;

    if result_json["success"].as_bool() == Some(true) {
        Ok(SelectParseResult::Success(
            result_json["selectedText"]
                .as_str()
                .map(ToString::to_string),
        ))
    } else {
        Ok(SelectParseResult::Failure {
            code: result_json["code"]
                .as_str()
                .unwrap_or("invalid_target")
                .to_string(),
            error: result_json["error"]
                .as_str()
                .unwrap_or("Select failed")
                .to_string(),
        })
    }
}

fn select_actionability_predicates() -> &'static [ActionabilityPredicate] {
    &[
        ActionabilityPredicate::Present,
        ActionabilityPredicate::Visible,
        ActionabilityPredicate::Enabled,
        ActionabilityPredicate::Stable,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_params_css() {
        let json = serde_json::json!({
            "selector": "#country-select",
            "value": "us"
        });

        let params: SelectParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.selector, Some("#country-select".to_string()));
        assert_eq!(params.index, None);
        assert_eq!(params.value, "us");
    }

    #[test]
    fn test_select_params_index() {
        let json = serde_json::json!({
            "index": 5,
            "value": "option2"
        });

        let params: SelectParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.selector, None);
        assert_eq!(params.index, Some(5));
        assert_eq!(params.value, "option2");
    }

    #[test]
    fn test_parse_select_result_success() {
        let result = parse_select_result(Some(serde_json::Value::String(
            r#"{"success":true,"selectedText":"United Kingdom"}"#.to_string(),
        )))
        .expect("select result should parse");

        match result {
            SelectParseResult::Success(selected_text) => {
                assert_eq!(selected_text.as_deref(), Some("United Kingdom"));
            }
            SelectParseResult::Failure { error, .. } => panic!("unexpected failure: {error}"),
        }
    }

    #[test]
    fn test_parse_select_result_failure_uses_code_and_error() {
        let result = parse_select_result(Some(serde_json::json!({
            "success": false,
            "code": "invalid_target",
            "error": "Element is not a SELECT element"
        })))
        .expect("select result should parse");

        match result {
            SelectParseResult::Failure { code, error } => {
                assert_eq!(code, "invalid_target");
                assert_eq!(error, "Element is not a SELECT element");
            }
            SelectParseResult::Success(_) => panic!("expected failure"),
        }
    }

    #[test]
    fn test_decode_tool_result_json_rejects_invalid_json_string() {
        let error = decode_action_result(
            Some(serde_json::Value::String("not-json".to_string())),
            serde_json::json!({}),
        )
        .expect_err("invalid JSON should fail");

        assert!(matches!(error, BrowserError::JsonError(_)));
    }

    #[test]
    fn test_select_js_prefers_selector_before_target_index() {
        let select_js = build_select_js(&serde_json::json!({
            "selector": "#country-select",
            "target_index": 5,
            "value": "us",
        }));

        assert!(select_js.contains("function resolveTargetMatch(config, options)"));
        assert!(select_js.contains("const element = resolveTargetElement(config);"));
        assert!(select_js.contains("querySelectorAcrossScopes("));
        assert!(select_js.contains("searchActionableIndex(config.target_index)"));
    }
}
