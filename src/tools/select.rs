use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelopeOptions, TargetResolution, Tool, ToolContext, ToolResult,
    build_document_envelope, resolve_target,
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
    pub node_ref: Option<crate::dom::NodeRef>,

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
    pub envelope: crate::tools::DocumentEnvelope,
    pub action: String,
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

    fn execute_typed(&self, params: SelectParams, context: &mut ToolContext) -> Result<ToolResult> {
        let SelectParams {
            selector,
            index,
            node_ref,
            value,
        } = params;
        let target = {
            let dom = if index.is_some() || node_ref.is_some() {
                Some(context.get_dom()?)
            } else {
                None
            };
            match resolve_target("select", selector, index, node_ref, dom)? {
                TargetResolution::Resolved(target) => target,
                TargetResolution::Failure(failure) => return Ok(failure),
            }
        };

        let select_config = serde_json::json!({
            "selector": target.selector,
            "value": value,
        });
        let select_js = SELECT_JS.replace("__SELECT_CONFIG__", &select_config.to_string());

        let result = context
            .session
            .tab()?
            .evaluate(&select_js, false)
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "select".to_string(),
                reason: e.to_string(),
            })?;

        match parse_select_result(result.value)? {
            SelectParseResult::Success(selected_text) => {
                context.invalidate_dom();
                Ok(ToolResult::success_with(SelectOutput {
                    envelope: build_document_envelope(
                        context,
                        Some(&target),
                        DocumentEnvelopeOptions::minimal(),
                    )?,
                    action: "select".to_string(),
                    value,
                    selected_text,
                }))
            }
            SelectParseResult::Failure(reason) => Err(BrowserError::ToolExecutionFailed {
                tool: "select".to_string(),
                reason,
            }),
        }
    }
}

enum SelectParseResult {
    Success(Option<String>),
    Failure(String),
}

fn parse_select_result(value: Option<serde_json::Value>) -> Result<SelectParseResult> {
    let result_json = decode_tool_result_json(
        value,
        serde_json::json!({"success": false, "error": "No result returned"}),
    )?;

    if result_json["success"].as_bool() == Some(true) {
        Ok(SelectParseResult::Success(
            result_json["selectedText"]
                .as_str()
                .map(ToString::to_string),
        ))
    } else {
        Ok(SelectParseResult::Failure(
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
            SelectParseResult::Failure(reason) => panic!("unexpected failure: {reason}"),
        }
    }

    #[test]
    fn test_parse_select_result_failure_uses_error_message() {
        let result = parse_select_result(Some(serde_json::json!({
            "success": false,
            "error": "Element is not a SELECT element"
        })))
        .expect("select result should parse");

        match result {
            SelectParseResult::Failure(reason) => {
                assert_eq!(reason, "Element is not a SELECT element");
            }
            SelectParseResult::Success(_) => panic!("expected failure"),
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
