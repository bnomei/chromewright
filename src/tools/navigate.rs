use crate::error::Result;
use crate::tools::utils::validate_navigation_url;
use crate::tools::{
    DocumentEnvelopeOptions, Tool, ToolContext, ToolResult, build_document_envelope,
    core::DocumentActionResult,
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
    pub result: DocumentActionResult,
    pub url: String,
}

impl Tool for NavigateTool {
    type Params = NavigateParams;
    type Output = NavigateOutput;

    fn name(&self) -> &str {
        "navigate"
    }

    fn description(&self) -> &str {
        "Open a URL. Next: wait or snapshot."
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
        let envelope = build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;
        Ok(context.finish(ToolResult::success_with(NavigateOutput {
            result: DocumentActionResult::new("navigate", envelope.document),
            url: normalized_url,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;

    #[test]
    fn test_navigate_tool_executes_against_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = NavigateTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                NavigateParams {
                    url: "https://example.com/docs".to_string(),
                    wait_for_load: true,
                    allow_unsafe: false,
                },
                &mut context,
            )
            .expect("navigate should succeed");

        assert!(result.success);
        let data = result.data.expect("navigate should include data");
        assert_eq!(data["action"].as_str(), Some("navigate"));
        assert_eq!(data["url"].as_str(), Some("https://example.com/docs"));
        assert_eq!(
            data["document"]["url"].as_str(),
            Some("https://example.com/docs")
        );
        assert_eq!(data["document"]["ready_state"].as_str(), Some("complete"));
    }
}
