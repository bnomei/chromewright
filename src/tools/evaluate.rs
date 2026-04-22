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

        context.record_browser_evaluation();
        let result = context
            .session
            .evaluate(&params.code, params.await_promise)?;

        let result_value = result.value.unwrap_or(Value::Null);

        Ok(context.finish(ToolResult::success_with(EvaluateOutput {
            result: result_value,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::tools::{OPERATION_METRICS_METADATA_KEY, Tool, ToolContext};

    #[test]
    fn test_evaluate_tool_records_browser_evaluation_metrics() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = EvaluateTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                EvaluateParams {
                    code: "document.readyState".to_string(),
                    await_promise: false,
                    confirm_unsafe: true,
                },
                &mut context,
            )
            .expect("evaluate should succeed");

        assert!(result.success);
        let metrics = result.metadata[OPERATION_METRICS_METADATA_KEY]
            .as_object()
            .expect("metrics metadata should be present");
        assert_eq!(metrics["browser_evaluations"].as_u64(), Some(1));
    }
}
