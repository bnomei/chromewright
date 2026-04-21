use crate::error::{BrowserError, Result};
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvaluateParams {
    /// JavaScript code to execute
    pub code: String,

    /// Wait for promise resolution (default: false)
    #[serde(default)]
    pub await_promise: bool,

    /// Explicit acknowledgement that this operator tool executes arbitrary JavaScript.
    #[serde(default)]
    pub confirm_unsafe: bool,
}

#[derive(Default)]
pub struct EvaluateTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvaluateOutput {
    pub result: Value,
}

impl Tool for EvaluateTool {
    type Params = EvaluateParams;
    type Output = EvaluateOutput;

    fn name(&self) -> &str {
        "evaluate"
    }

    fn execute_typed(
        &self,
        params: EvaluateParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        if !params.confirm_unsafe {
            return Err(BrowserError::InvalidArgument(
                "evaluate requires confirm_unsafe=true".to_string(),
            ));
        }

        let result = context
            .session
            .tab()?
            .evaluate(&params.code, params.await_promise)
            .map_err(|e| BrowserError::EvaluationFailed(e.to_string()))?;

        let result_value = result.value.unwrap_or(Value::Null);

        Ok(ToolResult::success_with(EvaluateOutput {
            result: result_value,
        }))
    }
}
