use crate::error::Result;
use crate::tools::core::structured_tool_failure;
use crate::tools::{
    DocumentActionResult, DocumentEnvelopeOptions, TabSummary, Tool, ToolContext, ToolResult,
    build_document_envelope,
};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;

/// Parameters for the switch_tab tool
#[derive(Debug, Clone, Serialize)]
pub struct SwitchTabParams {
    /// Tab index to switch to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Stable tab identifier to switch to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictSwitchTabParams {
    /// Stable tab identifier to activate.
    pub tab_id: String,
}

impl From<StrictSwitchTabParams> for SwitchTabParams {
    fn from(params: StrictSwitchTabParams) -> Self {
        Self {
            index: None,
            tab_id: Some(params.tab_id),
        }
    }
}

impl<'de> Deserialize<'de> for SwitchTabParams {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictSwitchTabParams::deserialize(deserializer).map(Into::into)
    }
}

impl JsonSchema for SwitchTabParams {
    fn schema_name() -> Cow<'static, str> {
        "SwitchTabParams".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        StrictSwitchTabParams::json_schema(generator)
    }
}

/// Tool for switching to a specific tab
#[derive(Default)]
pub struct SwitchTabTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwitchTabOutput {
    #[serde(flatten)]
    pub result: DocumentActionResult,
    pub tab: TabSummary,
    pub active_tab: TabSummary,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SwitchTabRequest {
    Index(usize),
    TabId(String),
}

impl Tool for SwitchTabTool {
    type Params = SwitchTabParams;
    type Output = SwitchTabOutput;

    fn name(&self) -> &str {
        "switch_tab"
    }

    fn description(&self) -> &str {
        "Activate a tab by stable tab_id. Usually after tab_list; run snapshot before DOM actions."
    }

    fn execute_typed(
        &self,
        params: SwitchTabParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let tabs = context.session.tab_overview()?;
        let requested_index = params.index;
        let requested_tab_id = params.tab_id.clone();
        let request = match parse_switch_request(params) {
            Ok(request) => request,
            Err(failure) => return Ok(context.finish(failure)),
        };

        if tabs.is_empty() {
            return Ok(context.finish(switch_tab_failure(
                "no_tabs",
                "No tabs available".to_string(),
                requested_index,
                requested_tab_id,
                tabs.len(),
            )));
        }

        let (target_index, target_id) = match request {
            SwitchTabRequest::Index(index) => {
                if index >= tabs.len() {
                    return Ok(context.finish(switch_tab_failure(
                        "invalid_tab_index",
                        format!(
                            "Invalid tab index: {}. Valid range: 0-{}",
                            index,
                            tabs.len() - 1
                        ),
                        Some(index),
                        None,
                        tabs.len(),
                    )));
                }

                (index, tabs[index].id.clone())
            }
            SwitchTabRequest::TabId(tab_id) => {
                let Some(index) = tabs.iter().position(|tab| tab.id == tab_id) else {
                    return Ok(context.finish(switch_tab_failure_with_available_ids(
                        "invalid_tab_id",
                        format!("No tab found for id {}", tab_id),
                        None,
                        Some(tab_id),
                        tabs.len(),
                        &tabs,
                    )));
                };

                (index, tabs[index].id.clone())
            }
        };

        context.session.activate_tab_by_id(&target_id)?;
        context.invalidate_dom();
        let tabs = context.session.tab_overview()?;
        let active_index = tabs
            .iter()
            .position(|tab| tab.active)
            .unwrap_or(target_index);
        let tab = TabSummary::from_browser_tab(target_index, &tabs[target_index]);
        let active_tab = TabSummary::from_browser_tab(active_index, &tabs[active_index]);

        let summary = format_switch_summary(
            active_index,
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
        let envelope = build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;

        Ok(context.finish(ToolResult::success_with(SwitchTabOutput {
            result: DocumentActionResult::new("switch_tab", envelope.document),
            message: summary,
            tab,
            active_tab,
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

fn parse_switch_request(
    params: SwitchTabParams,
) -> std::result::Result<SwitchTabRequest, ToolResult> {
    match (params.index, params.tab_id) {
        (Some(index), None) => Ok(SwitchTabRequest::Index(index)),
        (None, Some(tab_id)) if !tab_id.trim().is_empty() => Ok(SwitchTabRequest::TabId(tab_id)),
        (Some(index), Some(tab_id)) => Err(switch_tab_failure(
            "invalid_target_request",
            "Provide exactly one of index or tab_id".to_string(),
            Some(index),
            Some(tab_id),
            0,
        )),
        (None, None) => Err(switch_tab_failure(
            "invalid_target_request",
            "Provide exactly one of index or tab_id".to_string(),
            None,
            None,
            0,
        )),
        (None, Some(tab_id)) => Err(switch_tab_failure(
            "invalid_target_request",
            "tab_id must not be empty".to_string(),
            None,
            Some(tab_id),
            0,
        )),
    }
}

fn switch_tab_failure(
    code: &str,
    error: String,
    requested_index: Option<usize>,
    requested_tab_id: Option<String>,
    tab_count: usize,
) -> ToolResult {
    let valid_min = (tab_count > 0).then_some(0usize);
    let valid_max = tab_count.checked_sub(1);

    structured_tool_failure(
        code,
        error,
        None,
        None,
        Some(json!({
            "suggested_tool": "tab_list",
        })),
        Some(json!({
            "requested_index": requested_index,
            "requested_tab_id": requested_tab_id,
            "tab_count": tab_count,
            "valid_min": valid_min,
            "valid_max": valid_max,
        })),
    )
}

fn switch_tab_failure_with_available_ids(
    code: &str,
    error: String,
    requested_index: Option<usize>,
    requested_tab_id: Option<String>,
    tab_count: usize,
    tabs: &[crate::browser::TabInfo],
) -> ToolResult {
    let mut failure = switch_tab_failure(code, error, requested_index, requested_tab_id, tab_count);
    if let Some(data) = failure.data.as_mut() {
        data["details"]["available_tab_ids"] =
            json!(tabs.iter().map(|tab| tab.id.clone()).collect::<Vec<_>>());
    }
    failure
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use schemars::schema_for;
    use serde_json::json;

    #[test]
    fn test_switch_tab_params_public_surface_is_tab_id_only() {
        let params: SwitchTabParams = serde_json::from_value(json!({
            "tab_id": "tab-2"
        }))
        .expect("strict switch_tab params should deserialize");
        assert_eq!(params.tab_id.as_deref(), Some("tab-2"));
        assert_eq!(params.index, None);

        let error = serde_json::from_value::<SwitchTabParams>(json!({
            "index": 1
        }))
        .expect_err("legacy index field should be rejected");
        assert!(error.to_string().contains("unknown field `index`"));

        let schema = schema_for!(SwitchTabParams);
        let schema_json = serde_json::to_value(&schema).expect("schema should serialize");
        let properties = schema_json
            .get("properties")
            .and_then(|value| value.as_object())
            .expect("switch_tab params schema should expose properties");
        assert!(properties.contains_key("tab_id"));
        assert!(!properties.contains_key("index"));
    }

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

        let tool = SwitchTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: Some(0),
                    tab_id: None,
                },
                &mut context,
            )
            .expect("switch_tab should succeed");

        assert!(result.success);
        let data = result.data.expect("switch_tab should include data");
        assert_eq!(data["tab"]["tab_id"].as_str(), Some("tab-1"));
        assert_eq!(data["tab"]["index"].as_u64(), Some(0));
        assert_eq!(data["tab"]["url"].as_str(), Some("about:blank"));
        assert_eq!(data["active_tab"]["tab_id"].as_str(), Some("tab-1"));

        let active_index = session
            .tab_overview()
            .expect("tabs should load")
            .iter()
            .position(|tab| tab.active);
        assert_eq!(active_index, Some(0));
    }

    #[test]
    fn test_switch_tab_tool_executes_against_fake_backend_by_tab_id() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        let tool = SwitchTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: None,
                    tab_id: Some("tab-1".to_string()),
                },
                &mut context,
            )
            .expect("switch_tab by tab_id should succeed");

        assert!(result.success);
        let data = result.data.expect("switch_tab should include data");
        assert_eq!(data["tab"]["tab_id"].as_str(), Some("tab-1"));
        assert_eq!(data["active_tab"]["tab_id"].as_str(), Some("tab-1"));
        assert_eq!(data["tab"]["index"].as_u64(), Some(0));
    }

    #[test]
    fn test_switch_tab_tool_returns_structured_failure_when_no_tabs_are_available() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session.close().expect("session close should succeed");

        let tool = SwitchTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: Some(0),
                    tab_id: None,
                },
                &mut context,
            )
            .expect("switch_tab should return a structured failure");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("No tabs available"));
        let data = result
            .data
            .expect("switch_tab failure should include details");
        assert_eq!(data["code"].as_str(), Some("no_tabs"));
        assert_eq!(data["details"]["requested_index"].as_u64(), Some(0));
        assert!(data["details"]["requested_tab_id"].is_null());
        assert_eq!(data["details"]["tab_count"].as_u64(), Some(0));
        assert!(data["details"]["valid_min"].is_null());
        assert!(data["details"]["valid_max"].is_null());
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("tab_list")
        );
    }

    #[test]
    fn test_switch_tab_tool_returns_structured_failure_for_invalid_index() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        let tool = SwitchTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: Some(999),
                    tab_id: None,
                },
                &mut context,
            )
            .expect("switch_tab should return a structured failure");

        assert!(!result.success);
        assert_eq!(
            result.error.as_deref(),
            Some("Invalid tab index: 999. Valid range: 0-1")
        );
        let data = result
            .data
            .expect("switch_tab failure should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_tab_index"));
        assert_eq!(data["details"]["requested_index"].as_u64(), Some(999));
        assert!(data["details"]["requested_tab_id"].is_null());
        assert_eq!(data["details"]["tab_count"].as_u64(), Some(2));
        assert_eq!(data["details"]["valid_min"].as_u64(), Some(0));
        assert_eq!(data["details"]["valid_max"].as_u64(), Some(1));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("tab_list")
        );
    }

    #[test]
    fn test_switch_tab_tool_returns_structured_failure_for_invalid_tab_id() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session
            .open_tab_entry("https://second.example")
            .expect("second tab should open");

        let tool = SwitchTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: None,
                    tab_id: Some("tab-999".to_string()),
                },
                &mut context,
            )
            .expect("invalid tab_id should stay a structured failure");

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("No tab found for id tab-999"));
        let data = result
            .data
            .expect("switch_tab failure should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_tab_id"));
        assert!(data["details"]["requested_index"].is_null());
        assert_eq!(
            data["details"]["requested_tab_id"].as_str(),
            Some("tab-999")
        );
        assert_eq!(data["details"]["tab_count"].as_u64(), Some(2));
        assert_eq!(
            data["details"]["available_tab_ids"][0].as_str(),
            Some("tab-1")
        );
        assert_eq!(
            data["details"]["available_tab_ids"][1].as_str(),
            Some("tab-2")
        );
    }

    #[test]
    fn test_switch_tab_tool_requires_exactly_one_target_handle() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());

        let tool = SwitchTabTool;
        let mut context = ToolContext::new(&session);
        let result = tool
            .execute_typed(
                SwitchTabParams {
                    index: Some(0),
                    tab_id: Some("tab-1".to_string()),
                },
                &mut context,
            )
            .expect("ambiguous target request should stay a structured failure");

        assert!(!result.success);
        assert_eq!(
            result.error.as_deref(),
            Some("Provide exactly one of index or tab_id")
        );
        let data = result
            .data
            .expect("switch_tab failure should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_target_request"));
        assert_eq!(data["details"]["requested_index"].as_u64(), Some(0));
        assert_eq!(data["details"]["requested_tab_id"].as_str(), Some("tab-1"));
    }
}
