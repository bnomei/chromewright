use crate::error::{BrowserError, Result};
use crate::tools::{Tool, ToolContext, ToolResult, build_document_envelope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the go_forward tool (no parameters needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoForwardParams {}

/// Tool for navigating forward in browser history
#[derive(Default)]
pub struct GoForwardTool;

impl Tool for GoForwardTool {
    type Params = GoForwardParams;

    fn name(&self) -> &str {
        "go_forward"
    }

    fn execute_typed(
        &self,
        _params: GoForwardParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        context
            .session
            .go_forward()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "go_forward".to_string(),
                reason: e.to_string(),
            })?;

        context.refresh_dom()?;
        let mut payload = serde_json::to_value(build_document_envelope(context, None, true)?)?;
        if let serde_json::Value::Object(ref mut map) = payload {
            map.insert("action".to_string(), serde_json::json!("go_forward"));
        }

        Ok(ToolResult::success_with(payload))
    }
}
