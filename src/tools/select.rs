use crate::error::{BrowserError, Result};
use crate::tools::{
    TargetResolution, Tool, ToolContext, ToolResult, build_document_envelope, resolve_target,
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

impl Tool for SelectTool {
    type Params = SelectParams;

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

        // Parse the JSON string returned by JavaScript
        let result_json: serde_json::Value = if let Some(serde_json::Value::String(json_str)) =
            result.value
        {
            serde_json::from_str(&json_str)
                .unwrap_or(serde_json::json!({"success": false, "error": "Failed to parse result"}))
        } else {
            result
                .value
                .unwrap_or(serde_json::json!({"success": false, "error": "No result returned"}))
        };

        if result_json["success"].as_bool() == Some(true) {
            context.refresh_dom()?;
            let mut payload =
                serde_json::to_value(build_document_envelope(context, Some(&target), true)?)?;
            if let serde_json::Value::Object(ref mut map) = payload {
                map.insert("action".to_string(), serde_json::json!("select"));
                map.insert("value".to_string(), serde_json::json!(value));
                map.insert(
                    "selectedText".to_string(),
                    result_json["selectedText"].clone(),
                );
            }
            Ok(ToolResult::success_with(payload))
        } else {
            Err(BrowserError::ToolExecutionFailed {
                tool: "select".to_string(),
                reason: result_json["error"]
                    .as_str()
                    .unwrap_or("Unknown error")
                    .to_string(),
            })
        }
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
}
