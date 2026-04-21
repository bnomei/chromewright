use crate::error::{BrowserError, Result};
use crate::tools::{
    TargetResolution, Tool, ToolContext, ToolResult, build_document_envelope, resolve_target,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    pub node_ref: Option<crate::dom::NodeRef>,

    /// Text to type into the element
    pub text: String,

    /// Clear existing content first (default: false)
    #[serde(default)]
    pub clear: bool,
}

#[derive(Default)]
pub struct InputTool;

impl Tool for InputTool {
    type Params = InputParams;

    fn name(&self) -> &str {
        "input"
    }

    fn execute_typed(&self, params: InputParams, context: &mut ToolContext) -> Result<ToolResult> {
        let InputParams {
            selector,
            index,
            node_ref,
            text,
            clear,
        } = params;
        let target = {
            let dom = if index.is_some() || node_ref.is_some() {
                Some(context.get_dom()?)
            } else {
                None
            };
            match resolve_target("input", selector, index, node_ref, dom)? {
                TargetResolution::Resolved(target) => target,
                TargetResolution::Failure(failure) => return Ok(failure),
            }
        };

        let tab = context.session.tab()?;
        let element = context.session.find_element(&tab, &target.selector)?;

        if clear {
            element.click().ok(); // Focus
            // Clear with Ctrl+A and Delete
            tab.press_key("End").ok();
            for _ in 0..text.len() + 100 {
                tab.press_key("Backspace").ok();
            }
        }

        element
            .type_into(&text)
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "input".to_string(),
                reason: e.to_string(),
            })?;

        context.refresh_dom()?;
        let mut result_json =
            serde_json::to_value(build_document_envelope(context, Some(&target), true)?)?;
        if let serde_json::Value::Object(ref mut map) = result_json {
            map.insert("action".to_string(), serde_json::json!("input"));
            map.insert("text".to_string(), serde_json::json!(text));
            map.insert("clear".to_string(), serde_json::json!(clear));
        }

        Ok(ToolResult::success_with(result_json))
    }
}
