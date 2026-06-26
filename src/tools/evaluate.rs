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
    /// JSON value returned by the evaluation. `null` when the script returned
    /// JavaScript `null` *or* when no value was produced; use `value_present`
    /// to distinguish the two cases.
    pub result: Value,

    /// `true` when the browser returned an actual value payload. `false` means
    /// the evaluation produced no value (e.g. JavaScript `undefined`, a
    /// by-reference object, or a destroyed/detached context), in which case
    /// `result` is `null` as a placeholder and should not be read as a real
    /// `null` result.
    pub value_present: bool,

    /// CDP remote object type (e.g. `Undefined`, `Object`, `Number`) when
    /// available. Lets callers tell `undefined` apart from `null` results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,

    /// CDP remote object description when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Tool for EvaluateTool {
    type Params = EvaluateParams;
    type Output = EvaluateOutput;

    fn name(&self) -> &str {
        "evaluate"
    }

    fn description(&self) -> &str {
        "Run arbitrary JavaScript when inspect_node is insufficient. Requires confirm_unsafe=true."
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

        let value_present = result.value.is_some();
        let result_value = result.value.unwrap_or(Value::Null);

        Ok(context.finish(ToolResult::success_with(EvaluateOutput {
            result: result_value,
            value_present,
            type_name: result.type_name,
            description: result.description,
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
    fn test_evaluate_tool_requires_confirm_unsafe() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = EvaluateTool;
        let mut context = ToolContext::new(&session);

        let err = tool
            .execute_typed(
                EvaluateParams {
                    code: "document.readyState".to_string(),
                    await_promise: false,
                    confirm_unsafe: false,
                },
                &mut context,
            )
            .expect_err("evaluate should reject missing unsafe confirmation");

        assert!(
            matches!(err, BrowserError::InvalidArgument(message) if message == "evaluate requires confirm_unsafe=true")
        );
    }

    #[test]
    fn test_evaluate_tool_records_browser_evaluation_metrics() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = EvaluateTool;
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

    #[test]
    fn test_evaluate_tool_flags_missing_value() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = EvaluateTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                EvaluateParams {
                    code: "__devana_no_value__".to_string(),
                    await_promise: false,
                    confirm_unsafe: true,
                },
                &mut context,
            )
            .expect("evaluate should succeed");

        assert!(result.success);
        let output = result
            .data
            .as_ref()
            .expect("evaluate should emit data");
        assert_eq!(output["value_present"].as_bool(), Some(false));
        assert!(output["result"].is_null());
        assert_eq!(output["type_name"].as_str(), Some("Undefined"));
    }

    #[test]
    fn test_evaluate_tool_marks_value_present() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = EvaluateTool;
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

        let output = result
            .data
            .as_ref()
            .expect("evaluate should emit data");
        assert_eq!(output["value_present"].as_bool(), Some(true));
        assert_eq!(output["result"].as_str(), Some("complete"));
    }
}
