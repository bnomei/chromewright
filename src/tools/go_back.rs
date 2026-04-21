use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelopeOptions, Tool, ToolContext, ToolResult, build_document_envelope,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the go_back tool (no parameters needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoBackParams {}

/// Tool for navigating back in browser history
#[derive(Default)]
pub struct GoBackTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoBackOutput {
    #[serde(flatten)]
    pub envelope: crate::tools::DocumentEnvelope,
    pub action: String,
}

impl Tool for GoBackTool {
    type Params = GoBackParams;
    type Output = GoBackOutput;

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

        context.invalidate_dom();
        Ok(ToolResult::success_with(GoBackOutput {
            envelope: build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?,
            action: "go_back".to_string(),
        }))
    }
}
