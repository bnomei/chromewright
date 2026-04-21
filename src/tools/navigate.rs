use crate::error::Result;
use crate::tools::utils::validate_navigation_url;
use crate::tools::{
    DocumentEnvelopeOptions, Tool, ToolContext, ToolResult, build_document_envelope,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the navigate tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigateParams {
    /// URL to navigate to
    pub url: String,

    /// Wait for navigation to complete (default: true)
    #[serde(default = "default_wait")]
    pub wait_for_load: bool,

    /// Allow non-web/unsafe absolute schemes such as data: or file:
    #[serde(default)]
    pub allow_unsafe: bool,
}

fn default_wait() -> bool {
    true
}

/// Tool for navigating to a URL
#[derive(Default)]
pub struct NavigateTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigateOutput {
    #[serde(flatten)]
    pub envelope: crate::tools::DocumentEnvelope,
    pub action: String,
    pub url: String,
}

impl Tool for NavigateTool {
    type Params = NavigateParams;
    type Output = NavigateOutput;

    fn name(&self) -> &str {
        "navigate"
    }

    fn execute_typed(
        &self,
        params: NavigateParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        // Normalize the URL
        let normalized_url = validate_navigation_url(&params.url, params.allow_unsafe)?;

        // Navigate to normalized URL
        context.session.navigate(&normalized_url)?;

        // Wait for navigation if requested
        if params.wait_for_load {
            context.session.wait_for_navigation()?;
        }

        context.invalidate_dom();
        Ok(ToolResult::success_with(NavigateOutput {
            envelope: build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?,
            action: "navigate".to_string(),
            url: normalized_url,
        }))
    }
}
