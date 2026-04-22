use crate::error::{BrowserError, Result};
use crate::tools::utils::validate_navigation_url;
use crate::tools::{
    build_document_envelope, DocumentEnvelopeOptions, Tool, ToolContext, ToolResult,
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
    pub tab_id: String,
    pub tab: TabNavigationInfo,
    pub active_tab: TabNavigationInfo,
    pub url: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TabNavigationInfo {
    pub tab_id: String,
    pub index: usize,
    pub active: bool,
    pub title: String,
    pub url: String,
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
        let opened = context.session.open_tab(&normalized_url)?;
        context.invalidate_dom();
        let tab_list = context.session.tab_overview()?;
        let tab = tab_list
            .iter()
            .enumerate()
            .find(|(_, tab)| tab.id == opened.id)
            .map(|(index, tab)| TabNavigationInfo {
                tab_id: tab.id.clone(),
                index,
                active: tab.active,
                title: tab.title.clone(),
                url: tab.url.clone(),
            })
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
            .map(|(index, tab)| TabNavigationInfo {
                tab_id: tab.id.clone(),
                index,
                active: tab.active,
                title: tab.title.clone(),
                url: tab.url.clone(),
            })
            .ok_or_else(|| {
                BrowserError::TabOperationFailed(format!(
                    "Opened tab {} was not active after creation",
                    opened.id
                ))
            })?;
        let envelope = build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;
        Ok(context.finish(ToolResult::success_with(NewTabOutput {
            envelope,
            action: "new_tab".to_string(),
            message: format!("Opened a new tab for {}", normalized_url),
            tab_id: tab.tab_id.clone(),
            tab,
            active_tab,
            url: normalized_url,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::backend::FakeSessionBackend;
    use crate::browser::BrowserSession;

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
        assert_eq!(data["tab_id"].as_str(), Some("tab-2"));
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
