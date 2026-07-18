//! MCP tool that navigates browser history forward and returns a document envelope.
//!
//! Landings on non-web schemes require `allow_unsafe`; the DOM cache is invalidated after success.

use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelopeOptions, Tool, ToolContext, ToolResult, build_document_envelope,
    core::DocumentActionResult,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// History-forward options; set allow_unsafe when the next entry may use a non-web scheme.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoForwardParams {
    /// Allow landing on non-web schemes (`data:`, `file:`, …); rejected landings are reverted.
    #[serde(default)]
    pub allow_unsafe: bool,
}

/// Steps forward in session history and returns a minimal document envelope.
#[derive(Default)]
pub struct GoForwardTool;

impl Tool for GoForwardTool {
    type Params = GoForwardParams;
    type Output = DocumentActionResult;

    fn name(&self) -> &str {
        "go_forward"
    }

    fn description(&self) -> &str {
        "Go forward in history. Next: wait or snapshot."
    }

    fn execute_typed(
        &self,
        params: GoForwardParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let metrics = context
            .session
            .go_forward_with_metrics(params.allow_unsafe)
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "go_forward".to_string(),
                reason: e.to_string(),
            })?;
        context.record_browser_evaluations(metrics.browser_evaluations);
        context.record_poll_iterations(metrics.poll_iterations);

        context.invalidate_dom();
        let envelope = build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;
        Ok(
            context.finish(ToolResult::success_with(DocumentActionResult::new(
                "go_forward",
                envelope.document,
            ))),
        )
    }
}
