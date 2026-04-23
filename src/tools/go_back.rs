use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelopeOptions, Tool, ToolContext, ToolResult, build_document_envelope,
    core::DocumentActionResult,
};
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
    type Output = DocumentActionResult;

    fn name(&self) -> &str {
        "go_back"
    }

    fn description(&self) -> &str {
        "Go back in history. Next: wait or snapshot."
    }

    fn execute_typed(
        &self,
        _params: GoBackParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let metrics = context.session.go_back_with_metrics().map_err(|e| {
            BrowserError::ToolExecutionFailed {
                tool: "go_back".to_string(),
                reason: e.to_string(),
            }
        })?;
        context.record_browser_evaluations(metrics.browser_evaluations);
        context.record_poll_iterations(metrics.poll_iterations);

        context.invalidate_dom();
        let envelope = build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;
        Ok(
            context.finish(ToolResult::success_with(DocumentActionResult::new(
                "go_back",
                envelope.document,
            ))),
        )
    }
}
