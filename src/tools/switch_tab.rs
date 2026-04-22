use crate::error::Result;
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the switch_tab tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwitchTabParams {
    /// Tab index to switch to
    pub index: usize,
}

/// Tool for switching to a specific tab
#[derive(Default)]
pub struct SwitchTabTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwitchTabOutput {
    pub index: usize,
    pub title: String,
    pub url: String,
    pub message: String,
}

impl Tool for SwitchTabTool {
    type Params = SwitchTabParams;
    type Output = SwitchTabOutput;

    fn name(&self) -> &str {
        "switch_tab"
    }

    fn execute_typed(
        &self,
        params: SwitchTabParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let tabs = context.session.tab_overview()?;

        if tabs.is_empty() {
            return Ok(context.finish(ToolResult::failure("No tabs available")));
        }

        if params.index >= tabs.len() {
            return Ok(context.finish(ToolResult::failure(format!(
                "Invalid tab index: {}. Valid range: 0-{}",
                params.index,
                tabs.len() - 1
            ))));
        }

        let target_id = tabs[params.index].id.clone();
        context.session.activate_tab_by_id(&target_id)?;
        let tabs = context.session.tab_overview()?;
        let title = tabs[params.index].title.clone();
        let url = tabs[params.index].url.clone();

        let summary = format_switch_summary(
            params.index,
            &tabs
                .iter()
                .enumerate()
                .map(|(idx, tab)| TabSummaryLine {
                    index: idx,
                    title: tab.title.clone(),
                    url: tab.url.clone(),
                })
                .collect::<Vec<_>>(),
        );

        Ok(context.finish(ToolResult::success_with(SwitchTabOutput {
            index: params.index,
            message: summary,
            title,
            url,
        })))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TabSummaryLine {
    index: usize,
    title: String,
    url: String,
}

fn format_switch_summary(index: usize, tabs: &[TabSummaryLine]) -> String {
    let tab_list_str = tabs
        .iter()
        .map(|tab| format!("[{}] {} ({})", tab.index, tab.title, tab.url))
        .collect::<Vec<_>>()
        .join("\n");

    format!("Switched to tab {}\nAll Tabs:\n{}\n", index, tab_list_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;

    #[test]
    fn test_format_switch_summary_lists_all_tabs() {
        let summary = format_switch_summary(
            2,
            &[
                TabSummaryLine {
                    index: 0,
                    title: "First".to_string(),
                    url: "https://first.example".to_string(),
                },
                TabSummaryLine {
                    index: 2,
                    title: "Second".to_string(),
                    url: "https://second.example".to_string(),
                },
            ],
        );

        assert!(summary.contains("Switched to tab 2"));
        assert!(summary.contains("[0] First"));
        assert!(summary.contains("[2] Second"));
    }

    #[test]
    fn test_switch_tab_tool_executes_against_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        let tool = SwitchTabTool::default();
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(SwitchTabParams { index: 0 }, &mut context)
            .expect("switch_tab should succeed");

        assert!(result.success);
        let data = result.data.expect("switch_tab should include data");
        assert_eq!(data["index"].as_u64(), Some(0));
        assert_eq!(data["url"].as_str(), Some("about:blank"));

        let active_index = session
            .tab_overview()
            .expect("tabs should load")
            .iter()
            .position(|tab| tab.active);
        assert_eq!(active_index, Some(0));
    }
}
