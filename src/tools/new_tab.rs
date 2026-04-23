use crate::error::{BrowserError, Result};
use crate::tools::utils::validate_navigation_url;
use crate::tools::{
    DocumentActionResult, DocumentEnvelopeOptions, TabSummary, Tool, ToolContext, ToolResult,
    build_document_envelope,
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
    pub result: DocumentActionResult,
    pub tab: TabSummary,
    pub active_tab: TabSummary,
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
        "Open a URL in a new active tab. Then snapshot, or use tab_list/switch_tab if tabs changed."
    }

    fn execute_typed(&self, params: NewTabParams, context: &mut ToolContext) -> Result<ToolResult> {
        let normalized_url = validate_navigation_url(&params.url, params.allow_unsafe)?;
        let opened = context.session.open_tab(&normalized_url)?;
        context.invalidate_dom();
        let tab_list = context.session.tab_overview()?;
        let tab = tab_list
            .iter()
            .enumerate()
            .find(|(_, tab)| tab.id == opened.id)
            .map(|(index, tab)| TabSummary::from_browser_tab(index, tab))
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(format!(
                    "Opened tab {} was not present in the tab overview",
                    opened.id
                ))
            })?;
        let active_tab = tab_list
            .iter()
            .enumerate()
            .find(|(_, tab)| tab.active)
            .map(|(index, tab)| TabSummary::from_browser_tab(index, tab))
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(format!(
                    "Opened tab {} was not active after creation",
                    opened.id
                ))
            })?;
        let envelope = build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;
        Ok(context.finish(ToolResult::success_with(NewTabOutput {
            result: DocumentActionResult::new("new_tab", envelope.document),
            message: format!("Opened a new tab for {}", normalized_url),
            tab,
            active_tab,
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
        let tool = NewTabTool;
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
        assert_eq!(data["tab"]["tab_id"].as_str(), Some("tab-2"));
        assert_eq!(data["tab"]["index"].as_u64(), Some(1));
        assert_eq!(data["tab"]["active"].as_bool(), Some(true));
        assert_eq!(data["active_tab"]["tab_id"].as_str(), Some("tab-2"));
        assert_eq!(
            data["document"]["url"].as_str(),
            Some("https://second.example")
        );
        assert_eq!(session.tab_overview().expect("tabs should load").len(), 2);
    }
}
