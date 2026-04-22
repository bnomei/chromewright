use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelopeOptions, Tool, ToolContext, ToolResult, build_document_envelope,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the go_forward tool (no parameters needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoForwardParams {}

/// Tool for navigating forward in browser history
#[derive(Default)]
pub struct GoForwardTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoForwardOutput {
    #[serde(flatten)]
    pub envelope: crate::tools::DocumentEnvelope,
    pub action: String,
}

impl Tool for GoForwardTool {
    type Params = GoForwardParams;
    type Output = GoForwardOutput;

    fn name(&self) -> &str {
        "go_forward"
    }

    fn execute_typed(
        &self,
        _params: GoForwardParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let metrics = context.session.go_forward_with_metrics().map_err(|e| {
            BrowserError::ToolExecutionFailed {
                tool: "go_forward".to_string(),
                reason: e.to_string(),
            }
        })?;
        context.record_browser_evaluations(metrics.browser_evaluations);
        context.record_poll_iterations(metrics.poll_iterations);

        context.invalidate_dom();
        let envelope = build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;
        Ok(context.finish(ToolResult::success_with(GoForwardOutput {
            envelope,
            action: "go_forward".to_string(),
        })))
    }
}
