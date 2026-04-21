use crate::error::{BrowserError, Result};
use crate::tools::{Tool, ToolContext, ToolResult, build_document_envelope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the go_back tool (no parameters needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoBackParams {}

/// Tool for navigating back in browser history
#[derive(Default)]
pub struct GoBackTool;

impl Tool for GoBackTool {
    type Params = GoBackParams;

    fn name(&self) -> &str {
        "go_back"
    }

    fn execute_typed(
        &self,
        _params: GoBackParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        context
            .session
            .go_back()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "go_back".to_string(),
                reason: e.to_string(),
            })?;

        context.refresh_dom()?;
        let mut payload = serde_json::to_value(build_document_envelope(context, None, true)?)?;
        if let serde_json::Value::Object(ref mut map) = payload {
            map.insert("action".to_string(), serde_json::json!("go_back"));
        }

        Ok(ToolResult::success_with(payload))
    }
}
