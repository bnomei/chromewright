use crate::error::{BrowserError, Result};
use crate::tools::{Tool, ToolContext, ToolResult, resolve_target};
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
        let ClickParams { selector, index } = params;
        let target = {
            let dom = if index.is_some() {
                Some(context.get_dom()?)
            } else {
                None
            };
            resolve_target("click", selector, index, dom)?
        };

        let tab = context.session.tab()?;
        let element = context.session.find_element(&tab, &target.selector)?;
        element
            .click()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "click".to_string(),
                reason: e.to_string(),
            })?;

        let result = if let Some(index) = target.index {
            serde_json::json!({
                "index": index,
                "selector": target.selector,
                "method": target.method(),
            })
        } else {
            serde_json::json!({
                "selector": target.selector,
                "method": target.method(),
            })
        };

        Ok(ToolResult::success_with(result))
    }
}
