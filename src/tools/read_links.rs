use crate::error::Result;
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadLinksParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    /// The visible text content of the link
    pub text: String,
    /// The href attribute of the link
    pub href: String,
}

#[derive(Default)]
pub struct ReadLinksTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadLinksOutput {
    pub links: Vec<Link>,
    pub count: usize,
}

impl Tool for ReadLinksTool {
    type Params = ReadLinksParams;
    type Output = ReadLinksOutput;

    fn name(&self) -> &str {
        "read_links"
    }

    fn execute_typed(
        &self,
        _params: ReadLinksParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        // JavaScript code to extract all links on the page
        // We use JSON.stringify to ensure the result is returned properly
        let js_code = r#"
            JSON.stringify(
                Array.from(document.querySelectorAll('a[href]'))
                    .map(el => ({
                        text: el.innerText || '',
                        href: el.getAttribute('href') || ''
                    }))
                    .filter(link => link.href !== '')
            )
        "#;

        context.record_browser_evaluation();
        let result = context.session.evaluate(js_code, false)?;

        let links = match parse_links_output(result.value) {
            Ok(links) => links,
            Err(reason) => {
                return Ok(context.finish(ToolResult::failure_with(
                    reason.clone(),
                    serde_json::json!({
                        "code": "invalid_links_payload",
                        "error": reason,
                        "recovery": {
                            "suggested_tool": "snapshot",
                        }
                    }),
                )));
            }
        };

        Ok(context.finish(ToolResult::success_with(ReadLinksOutput {
            count: links.len(),
            links,
        })))
    }
}

fn parse_links_output(value: Option<Value>) -> std::result::Result<Vec<Link>, String> {
    match value {
        Some(Value::String(payload)) => serde_json::from_str(&payload)
            .map_err(|error| format!("Failed to parse link extraction result: {}", error)),
        Some(other) => serde_json::from_value(other)
            .map_err(|error| format!("Failed to deserialize link extraction result: {}", error)),
        None => Err("Link extraction returned no data".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{
        FakeSessionBackend, ScriptEvaluation, SessionBackend, TabDescriptor,
    };
    use crate::tools::{OPERATION_METRICS_METADATA_KEY, Tool, ToolContext};
    use crate::{dom::DocumentMetadata, dom::DomTree};
    use std::any::Any;
    use std::time::Duration;

    struct InvalidLinksPayloadBackend;

    impl SessionBackend for InvalidLinksPayloadBackend {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn navigate(&self, _url: &str) -> crate::error::Result<()> {
            unreachable!("navigate is not used in this test")
        }

        fn wait_for_navigation(&self) -> crate::error::Result<()> {
            unreachable!("wait_for_navigation is not used in this test")
        }

        fn wait_for_document_ready_with_timeout(
            &self,
            _timeout: Duration,
        ) -> crate::error::Result<()> {
            unreachable!("wait_for_document_ready_with_timeout is not used in this test")
        }

        fn document_metadata(&self) -> crate::error::Result<DocumentMetadata> {
            unreachable!("document_metadata is not used in this test")
        }

        fn extract_dom(&self) -> crate::error::Result<DomTree> {
            unreachable!("extract_dom is not used in this test")
        }

        fn extract_dom_with_prefix(&self, _prefix: &str) -> crate::error::Result<DomTree> {
            unreachable!("extract_dom_with_prefix is not used in this test")
        }

        fn evaluate(
            &self,
            _script: &str,
            _await_promise: bool,
        ) -> crate::error::Result<ScriptEvaluation> {
            Ok(ScriptEvaluation {
                value: Some(Value::String("not-json".to_string())),
                description: None,
                type_name: Some("String".to_string()),
            })
        }

        fn capture_screenshot(&self, _full_page: bool) -> crate::error::Result<Vec<u8>> {
            unreachable!("capture_screenshot is not used in this test")
        }

        fn press_key(&self, _key: &str) -> crate::error::Result<()> {
            unreachable!("press_key is not used in this test")
        }

        fn list_tabs(&self) -> crate::error::Result<Vec<TabDescriptor>> {
            unreachable!("list_tabs is not used in this test")
        }

        fn active_tab(&self) -> crate::error::Result<TabDescriptor> {
            unreachable!("active_tab is not used in this test")
        }

        fn open_tab(&self, _url: &str) -> crate::error::Result<TabDescriptor> {
            unreachable!("open_tab is not used in this test")
        }

        fn activate_tab(&self, _tab_id: &str) -> crate::error::Result<()> {
            unreachable!("activate_tab is not used in this test")
        }

        fn close_tab(&self, _tab_id: &str, _with_unload: bool) -> crate::error::Result<()> {
            unreachable!("close_tab is not used in this test")
        }

        fn close(&self) -> crate::error::Result<()> {
            unreachable!("close is not used in this test")
        }
    }

    #[test]
    fn test_read_links_tool_records_browser_evaluation_metrics() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = ReadLinksTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(ReadLinksParams {}, &mut context)
            .expect("read_links should succeed");

        assert!(result.success);
        let metrics = result.metadata[OPERATION_METRICS_METADATA_KEY]
            .as_object()
            .expect("metrics metadata should be present");
        assert_eq!(metrics["browser_evaluations"].as_u64(), Some(1));
    }

    #[test]
    fn test_read_links_tool_returns_structured_failure_for_invalid_payload() {
        let session = BrowserSession::with_test_backend(InvalidLinksPayloadBackend);
        let tool = ReadLinksTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(ReadLinksParams {}, &mut context)
            .expect("invalid payload should stay a tool failure");

        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("Failed to parse link extraction result")
        );
        let data = result
            .data
            .expect("invalid payload failure should include details");
        assert_eq!(data["code"].as_str(), Some("invalid_links_payload"));
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("snapshot")
        );
        let metrics = result.metadata[OPERATION_METRICS_METADATA_KEY]
            .as_object()
            .expect("metrics metadata should be present on failures");
        assert_eq!(metrics["browser_evaluations"].as_u64(), Some(1));
    }
}
