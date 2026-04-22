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

/// Parameters for the hover tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HoverParams {
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

/// Tool for hovering over elements
#[derive(Default)]
pub struct HoverTool;

const HOVER_JS: &str = include_str!("hover.js");

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HoverElement {
    pub tag_name: String,
    pub id: String,
    pub class_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HoverOutput {
    #[serde(flatten)]
    pub envelope: DocumentEnvelope,
    pub action: String,
    pub target_before: TargetEnvelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_after: Option<TargetEnvelope>,
    pub target_status: TargetStatus,
    pub element: HoverElement,
}

impl Tool for HoverTool {
    type Params = HoverParams;
    type Output = HoverOutput;

    fn name(&self) -> &str {
        "hover"
    }

    fn execute_typed(&self, params: HoverParams, context: &mut ToolContext) -> Result<ToolResult> {
        let HoverParams {
            selector,
            index,
            node_ref,
            cursor,
        } = params;
        let target = match resolve_interaction_target(
            "hover", selector, index, node_ref, cursor, context,
        )? {
            TargetResolution::Resolved(target) => target,
            TargetResolution::Failure(failure) => return Ok(failure),
        };

        let tab = context.session.tab()?;
        let predicates = hover_actionability_predicates();
        match wait_for_actionability(&tab, &target, predicates, DEFAULT_ACTIONABILITY_TIMEOUT_MS)? {
            ActionabilityWaitState::Ready => {}
            ActionabilityWaitState::TimedOut(probe) => {
                return build_actionability_failure(
                    "hover", &tab, &target, &probe, predicates, None,
                );
            }
        }

        let hover_config = serde_json::json!({
            "selector": target.selector,
            "target_index": target.cursor.as_ref().map(|cursor| cursor.index).or(target.index),
        });
        let hover_js = HOVER_JS.replace("__HOVER_CONFIG__", &hover_config.to_string());

        let result =
            tab.evaluate(&hover_js, false)
                .map_err(|e| BrowserError::ToolExecutionFailed {
                    tool: "hover".to_string(),
                    reason: e.to_string(),
                })?;

        match parse_hover_result(result.value)? {
            HoverParseResult::Success(element) => {
                let handoff = build_interaction_handoff(context, &tab, &target)?;
                Ok(ToolResult::success_with(HoverOutput {
                    envelope: handoff.envelope,
                    action: "hover".to_string(),
                    target_before: handoff.target_before,
                    target_after: handoff.target_after,
                    target_status: handoff.target_status,
                    element,
                }))
            }
            HoverParseResult::Failure { code, error } => {
                build_interaction_failure("hover", &tab, &target, code, error, Vec::new(), None)
            }
        }
    }
}

enum HoverParseResult {
    Success(HoverElement),
    Failure { code: String, error: String },
}

fn parse_hover_result(value: Option<serde_json::Value>) -> Result<HoverParseResult> {
    let result_json = decode_action_result(
        value,
        serde_json::json!({
            "success": false,
            "code": "target_detached",
            "error": "Element is no longer present"
        }),
    )?;

    if result_json["success"].as_bool() == Some(true) {
        Ok(HoverParseResult::Success(HoverElement {
            tag_name: result_json["tagName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            id: result_json["id"].as_str().unwrap_or_default().to_string(),
            class_name: result_json["className"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        }))
    } else {
        Ok(HoverParseResult::Failure {
            code: result_json["code"]
                .as_str()
                .unwrap_or("target_detached")
                .to_string(),
            error: result_json["error"]
                .as_str()
                .unwrap_or("Hover failed")
                .to_string(),
        })
    }
}

fn hover_actionability_predicates() -> &'static [ActionabilityPredicate] {
    &[
        ActionabilityPredicate::Present,
        ActionabilityPredicate::Visible,
        ActionabilityPredicate::Stable,
        ActionabilityPredicate::ReceivesEvents,
        ActionabilityPredicate::UnobscuredCenter,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hover_result_success() {
        let result = parse_hover_result(Some(serde_json::Value::String(
            r#"{"success":true,"tagName":"BUTTON","id":"save","className":"primary"}"#.to_string(),
        )))
        .expect("hover result should parse");

        match result {
            HoverParseResult::Success(element) => {
                assert_eq!(element.tag_name, "BUTTON");
                assert_eq!(element.id, "save");
                assert_eq!(element.class_name, "primary");
            }
            HoverParseResult::Failure { error, .. } => panic!("unexpected failure: {error}"),
        }
    }

    #[test]
    fn test_parse_hover_result_failure_uses_code_and_error() {
        let result = parse_hover_result(Some(serde_json::json!({
            "success": false,
            "code": "target_detached",
            "error": "Element not found"
        })))
        .expect("hover result should parse");

        match result {
            HoverParseResult::Failure { code, error } => {
                assert_eq!(code, "target_detached");
                assert_eq!(error, "Element not found");
            }
            HoverParseResult::Success(_) => panic!("expected failure"),
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
    fn test_hover_js_prefers_selector_before_target_index() {
        assert!(super::HOVER_JS.contains("const selectorMatch = config.selector"));
        assert!(super::HOVER_JS.contains("? querySelectorAcrossScopes(config.selector)"));
        assert!(
            super::HOVER_JS
                .contains("const element =\n      selectorMatch && selectorMatch.isConnected")
        );
        assert!(super::HOVER_JS.contains("? searchActionableIndex(config.target_index)"));
    }
}
