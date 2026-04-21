use crate::error::{BrowserError, Result};
use crate::tools::{
    TargetResolution, Tool, ToolContext, ToolResult, build_document_envelope, resolve_target,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the click tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClickParams {
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

/// Tool for clicking elements
#[derive(Default)]
pub struct ClickTool;

impl Tool for ClickTool {
    type Params = ClickParams;

    fn name(&self) -> &str {
        "click"
    }

    fn execute_typed(&self, params: ClickParams, context: &mut ToolContext) -> Result<ToolResult> {
        let ClickParams {
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
            match resolve_target("click", selector, index, node_ref, dom)? {
                TargetResolution::Resolved(target) => target,
                TargetResolution::Failure(failure) => return Ok(failure),
            }
        };

        let tab = context.session.tab()?;
        let element = context.session.find_element(&tab, &target.selector)?;
        element
            .click()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "click".to_string(),
                reason: e.to_string(),
            })?;

        context.refresh_dom()?;
        let mut result = serde_json::to_value(build_document_envelope(context, Some(&target), true)?)?;
        if let serde_json::Value::Object(ref mut map) = result {
            map.insert("action".to_string(), serde_json::json!("click"));
        }

        Ok(ToolResult::success_with(result))
    }
}
