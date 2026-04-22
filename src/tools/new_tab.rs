use crate::error::Result;
use crate::tools::utils::validate_navigation_url;
use crate::tools::{
    DocumentEnvelopeOptions, Tool, ToolContext, ToolResult, build_document_envelope,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the new_tab tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NewTabParams {
    /// URL to open in the new tab
    pub url: String,

    /// Allow non-web/unsafe absolute schemes such as data: or file:
    #[serde(default)]
    pub allow_unsafe: bool,
}

/// Tool for opening a new tab
#[derive(Default)]
pub struct NewTabTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NewTabOutput {
    #[serde(flatten)]
    pub envelope: crate::tools::DocumentEnvelope,
    pub action: String,
    pub url: String,
    pub message: String,
}

impl Tool for NewTabTool {
    type Params = NewTabParams;
    type Output = NewTabOutput;

    fn name(&self) -> &str {
        "new_tab"
    }

    fn description(&self) -> &str {
        "Open a URL in a new tab. Next: tab_list, switch_tab, or snapshot."
    }

    fn execute_typed(&self, params: NewTabParams, context: &mut ToolContext) -> Result<ToolResult> {
        let normalized_url = validate_navigation_url(&params.url, params.allow_unsafe)?;
        context.session.open_tab_entry(&normalized_url)?;
        context.invalidate_dom();
        let envelope = build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;
        Ok(context.finish(ToolResult::success_with(NewTabOutput {
            envelope,
            action: "new_tab".to_string(),
            message: format!("Opened a new tab for {}", normalized_url),
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
    fn test_new_tab_tool_executes_against_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = NewTabTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                NewTabParams {
                    url: "https://second.example".to_string(),
                    allow_unsafe: false,
                },
                &mut context,
            )
            .expect("new_tab should succeed");

        assert!(result.success);
        let data = result.data.expect("new_tab should include data");
        assert_eq!(data["action"].as_str(), Some("new_tab"));
        assert_eq!(data["url"].as_str(), Some("https://second.example"));
        assert_eq!(
            data["document"]["url"].as_str(),
            Some("https://second.example")
        );
        assert_eq!(session.tab_overview().expect("tabs should load").len(), 2);
    }
}
