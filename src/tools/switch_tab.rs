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
        // Get all tabs to validate index
        let tabs = context.session.get_tabs()?;

        if params.index >= tabs.len() {
            return Ok(ToolResult::failure(format!(
                "Invalid tab index: {}. Valid range: 0-{}",
                params.index,
                tabs.len() - 1
            )));
        }

        // Get the tab at the specified index
        let target_tab = tabs[params.index].clone();

        // Activate the tab and keep the session hint coherent.
        context.session.activate_tab(&target_tab)?;

        // Get updated tab info
        let title = target_tab.get_title().unwrap_or_default();
        let url = target_tab.get_url();

        // Build tab list summary
        let summary = format_switch_summary(
            params.index,
            &tabs
                .iter()
                .enumerate()
                .map(|(idx, tab)| TabSummaryLine {
                    index: idx,
                    title: tab.get_title().unwrap_or_default(),
                    url: tab.get_url(),
                })
                .collect::<Vec<_>>(),
        );

        Ok(ToolResult::success_with(SwitchTabOutput {
            index: params.index,
            message: summary,
            title,
            url,
        }))
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
}
