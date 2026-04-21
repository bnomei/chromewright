use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelopeOptions, TargetResolution, Tool, ToolContext, ToolResult,
    build_document_envelope, resolve_target,
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
    pub node_ref: Option<crate::dom::NodeRef>,
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
    pub envelope: crate::tools::DocumentEnvelope,
    pub action: String,
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
        } = params;
        let target = {
            let dom = if index.is_some() || node_ref.is_some() {
                Some(context.get_dom()?)
            } else {
                None
            };
            match resolve_target("hover", selector, index, node_ref, dom)? {
                TargetResolution::Resolved(target) => target,
                TargetResolution::Failure(failure) => return Ok(failure),
            }
        };

        // Find the element (to verify it exists)

        // Scroll into view if needed, then hover
        let selector_json =
            serde_json::to_string(&target.selector).expect("serializing CSS selector never fails");
        let hover_js = HOVER_JS.replace("__SELECTOR__", &selector_json);

        let result = context
            .session
            .tab()?
            .evaluate(&hover_js, false)
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "hover".to_string(),
                reason: e.to_string(),
            })?;

        match parse_hover_result(result.value)? {
            HoverParseResult::Success(element) => {
                context.invalidate_dom();
                Ok(ToolResult::success_with(HoverOutput {
                    envelope: build_document_envelope(
                        context,
                        Some(&target),
                        DocumentEnvelopeOptions::minimal(),
                    )?,
                    action: "hover".to_string(),
                    element,
                }))
            }
            HoverParseResult::Failure(reason) => Err(BrowserError::ToolExecutionFailed {
                tool: "hover".to_string(),
                reason,
            }),
        }
    }
}

enum HoverParseResult {
    Success(HoverElement),
    Failure(String),
}

fn parse_hover_result(value: Option<serde_json::Value>) -> Result<HoverParseResult> {
    let result_json = decode_tool_result_json(
        value,
        serde_json::json!({"success": false, "error": "No result returned"}),
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
        Ok(HoverParseResult::Failure(
            result_json["error"]
                .as_str()
                .unwrap_or("Unknown error")
                .to_string(),
        ))
    }
}

fn decode_tool_result_json(
    value: Option<serde_json::Value>,
    fallback: serde_json::Value,
) -> Result<serde_json::Value> {
    if let Some(serde_json::Value::String(json_str)) = value {
        serde_json::from_str(&json_str).map_err(BrowserError::from)
    } else {
        Ok(value.unwrap_or(fallback))
    }
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
            HoverParseResult::Failure(reason) => panic!("unexpected failure: {reason}"),
        }
    }

    #[test]
    fn test_parse_hover_result_failure_uses_error_message() {
        let result = parse_hover_result(Some(serde_json::json!({
            "success": false,
            "error": "Element not found"
        })))
        .expect("hover result should parse");

        match result {
            HoverParseResult::Failure(reason) => assert_eq!(reason, "Element not found"),
            HoverParseResult::Success(_) => panic!("expected failure"),
        }
    }

    #[test]
    fn test_decode_tool_result_json_rejects_invalid_json_string() {
        let error = decode_tool_result_json(
            Some(serde_json::Value::String("not-json".to_string())),
            serde_json::json!({}),
        )
        .expect_err("invalid JSON should fail");

        assert!(matches!(error, BrowserError::JsonError(_)));
    }
}
