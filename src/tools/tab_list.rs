use crate::error::Result;
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Information about a browser tab
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TabInfo {
    /// Tab index
    pub index: usize,
    /// Whether this is the active tab
    pub active: bool,
    /// Tab title
    pub title: String,
    /// Tab URL
    pub url: String,
}

/// Parameters for the tab_list tool (no parameters needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TabListParams {}

/// Tool for listing all browser tabs
#[derive(Default)]
pub struct TabListTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TabListOutput {
    pub tab_list: Vec<TabInfo>,
    pub count: usize,
    pub summary: String,
}

impl Tool for TabListTool {
    type Params = TabListParams;
    type Output = TabListOutput;

    fn name(&self) -> &str {
        "tab_list"
    }

    fn execute_typed(
        &self,
        _params: TabListParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        // Get all tabs
        let tabs = context.session.get_tabs()?;
        let active_tab = context.session.tab()?;

        // Build tab info list
        let mut tab_list = Vec::new();
        for (index, tab) in tabs.iter().enumerate() {
            // Check if this is the active tab by comparing Arc pointers
            let is_active = std::sync::Arc::ptr_eq(tab, &active_tab);

            // Get tab title (fallback to empty string on error)
            let title = tab.get_title().unwrap_or_default();

            // Get tab URL (not a Result, returns String directly)
            let url = tab.get_url();

            tab_list.push(TabInfo {
                index,
                active: is_active,
                title,
                url,
            });
        }

        // Build summary text
        let active_index = tab_list.iter().position(|t| t.active).unwrap_or(0);

        let summary = summarize_tab_list(&tab_list, active_index);

        Ok(ToolResult::success_with(TabListOutput {
            count: tab_list.len(),
            summary,
            tab_list,
        }))
    }
}

fn summarize_tab_list(tab_list: &[TabInfo], active_index: usize) -> String {
    if tab_list.is_empty() {
        return "No tabs available".to_string();
    }

    let active_info = &tab_list[active_index];
    let all_tabs_str = tab_list
        .iter()
        .map(|tab| format!("[{}] Title: {} (URL: {})", tab.index, tab.title, tab.url))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "Current Tab: [{}] {}\nAll Tabs:\n{}",
        active_index, active_info.title, all_tabs_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarize_tab_list_includes_active_tab_and_all_tabs() {
        let summary = summarize_tab_list(
            &[
                TabInfo {
                    index: 0,
                    active: false,
                    title: "First".to_string(),
                    url: "https://first.example".to_string(),
                },
                TabInfo {
                    index: 1,
                    active: true,
                    title: "Second".to_string(),
                    url: "https://second.example".to_string(),
                },
            ],
            1,
        );

        assert!(summary.contains("Current Tab: [1] Second"));
        assert!(summary.contains("[0] Title: First"));
        assert!(summary.contains("[1] Title: Second"));
    }

    #[test]
    fn test_summarize_tab_list_handles_empty_list() {
        assert_eq!(summarize_tab_list(&[], 0), "No tabs available");
    }
}
