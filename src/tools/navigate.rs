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

impl Tool for NavigateTool {
    type Params = NavigateParams;

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
        let mut payload = serde_json::to_value(build_document_envelope(
            context,
            None,
            DocumentEnvelopeOptions::minimal(),
        )?)?;
        if let serde_json::Value::Object(ref mut map) = payload {
            map.insert("action".to_string(), serde_json::json!("navigate"));
            map.insert("url".to_string(), serde_json::json!(normalized_url));
        }

        Ok(ToolResult::success_with(payload))
    }
}
