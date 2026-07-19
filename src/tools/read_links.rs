//! MCP tool that lists page anchors with raw href and resolved absolute URLs.
//!
//! Enforces link-count and field-length budgets before returning a document result.

use crate::error::Result;
use crate::tools::core::structured_tool_failure;
use crate::tools::limits::{MAX_READ_LINK_FIELD_CHARS, MAX_READ_LINKS_COUNT};
use crate::tools::{DocumentResult, Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Empty params; link listing always scans the active document.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadLinksParams {}

/// Single anchor entry with visible text, raw href, and absolute resolved_url.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    /// The visible text content of the link
    pub text: String,
    /// The href attribute of the link
    pub href: String,
    /// The fully resolved absolute URL for the link target
    pub resolved_url: String,
}

/// Lists anchor links on the page with resolved absolute URLs for navigation planning.
#[derive(Default)]
pub struct ReadLinksTool;

/// Document result with collected links and total count.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadLinksOutput {
    /// Document identity for the page that was scanned.
    #[serde(flatten)]
    pub result: DocumentResult,
    /// Bounded list of anchors with raw and resolved URLs.
    pub links: Vec<Link>,
    /// Number of links returned (`links.len()` after budgets).
    pub count: usize,
}

impl Tool for ReadLinksTool {
    type Params = ReadLinksParams;
    type Output = ReadLinksOutput;

    fn name(&self) -> &str {
        "read_links"
    }

    fn description(&self) -> &str {
        "List raw href and resolved_url links. Next: click or navigate."
    }

    fn execute_typed(
        &self,
        _params: ReadLinksParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        // JavaScript code to extract all links on the page
        // We use JSON.stringify to ensure the result is returned properly
        let js_code = format!(
            r#"
            JSON.stringify((() => {{
                const maxLinks = {MAX_READ_LINKS_COUNT};
                const maxFieldChars = {MAX_READ_LINK_FIELD_CHARS};
                const links = [];
                for (const el of Array.from(document.querySelectorAll('a[href]'))) {{
                    const link = {{
                        text: el.innerText || '',
                        href: el.getAttribute('href') || '',
                        resolved_url: el.href || ''
                    }};
                    if (link.href === '') {{
                        continue;
                    }}
                    for (const [field, value] of Object.entries(link)) {{
                        if (String(value).length > maxFieldChars) {{
                            return {{
                                success: false,
                                code: 'resource_limit_exceeded',
                                error: `read_links field ${{field}} exceeds the ${{maxFieldChars}} character limit`,
                                resource: 'read_links_field_chars',
                                field,
                                char_count: String(value).length
                            }};
                        }}
                    }}
                    links.push(link);
                    if (links.length > maxLinks) {{
                        return {{
                            success: false,
                            code: 'resource_limit_exceeded',
                            error: `read_links found more than ${{maxLinks}} links`,
                            resource: 'read_links_count',
                            count: links.length
                        }};
                    }}
                }}
                return {{
                    success: true,
                    links
                }};
            }})())
        "#
        );

        context.record_browser_evaluation();
        let result = context.session.evaluate(&js_code, false)?;

        let links = match parse_links_output(result.value) {
            Ok(links) => links,
            Err(ReadLinksFailure::InvalidPayload(reason)) => {
                return Ok(context.finish(structured_tool_failure(
                    "invalid_links_payload",
                    reason,
                    None,
                    None,
                    Some(serde_json::json!({
                        "suggested_tool": "snapshot",
                    })),
                    None,
                )));
            }
            Err(ReadLinksFailure::ResourceLimit {
                resource,
                error,
                actual,
                field,
            }) => {
                return Ok(context.finish(structured_tool_failure(
                    "resource_limit_exceeded",
                    error,
                    None,
                    None,
                    Some(serde_json::json!({
                        "suggested_tool": "snapshot",
                    })),
                    Some(serde_json::json!({
                        "resource": resource,
                        "limit": read_links_limit_for_resource(resource),
                        "actual": actual,
                        "field": field,
                    })),
                )));
            }
        };

        context.record_browser_evaluation();
        let document = context.session.document_metadata()?;

        Ok(context.finish(ToolResult::success_with(ReadLinksOutput {
            result: DocumentResult::new(document),
            count: links.len(),
            links,
        })))
    }
}

#[derive(Debug)]
enum ReadLinksFailure {
    InvalidPayload(String),
    ResourceLimit {
        resource: &'static str,
        error: String,
        actual: String,
        field: Option<String>,
    },
}

fn read_links_limit_for_resource(resource: &str) -> String {
    match resource {
        "read_links_count" => format!("{MAX_READ_LINKS_COUNT} links"),
        "read_links_field_chars" => format!("{MAX_READ_LINK_FIELD_CHARS} characters"),
        _ => String::new(),
    }
}

fn validate_link_limits(links: &[Link]) -> std::result::Result<(), ReadLinksFailure> {
    if links.len() > MAX_READ_LINKS_COUNT {
        return Err(ReadLinksFailure::ResourceLimit {
            resource: "read_links_count",
            error: format!(
                "read_links found {} links, exceeding the {MAX_READ_LINKS_COUNT} link limit",
                links.len()
            ),
            actual: format!("{} links", links.len()),
            field: None,
        });
    }

    for link in links {
        for (field, value) in [
            ("text", link.text.as_str()),
            ("href", link.href.as_str()),
            ("resolved_url", link.resolved_url.as_str()),
        ] {
            let char_count = value.chars().count();
            if char_count > MAX_READ_LINK_FIELD_CHARS {
                return Err(ReadLinksFailure::ResourceLimit {
                    resource: "read_links_field_chars",
                    error: format!(
                        "read_links field {field} is {char_count} characters, exceeding the {MAX_READ_LINK_FIELD_CHARS} character limit"
                    ),
                    actual: format!("{char_count} characters"),
                    field: Some(field.to_string()),
                });
            }
        }
    }

    Ok(())
}

fn parse_links_output(value: Option<Value>) -> std::result::Result<Vec<Link>, ReadLinksFailure> {
    match value {
        Some(Value::String(payload)) => {
            let value = serde_json::from_str::<Value>(&payload).map_err(|error| {
                ReadLinksFailure::InvalidPayload(format!(
                    "Failed to parse link extraction result: {}",
                    error
                ))
            })?;
            parse_links_value(value)
        }
        Some(other) => parse_links_value(other),
        None => Err(ReadLinksFailure::InvalidPayload(
            "Link extraction returned no data".to_string(),
        )),
    }
}

fn parse_links_value(value: Value) -> std::result::Result<Vec<Link>, ReadLinksFailure> {
    if let Some(array) = value.as_array() {
        let links: Vec<Link> =
            serde_json::from_value(Value::Array(array.clone())).map_err(|error| {
                ReadLinksFailure::InvalidPayload(format!(
                    "Failed to deserialize link extraction result: {}",
                    error
                ))
            })?;
        validate_link_limits(&links)?;
        return Ok(links);
    }

    let Some(object) = value.as_object() else {
        return Err(ReadLinksFailure::InvalidPayload(
            "Link extraction returned an unexpected payload".to_string(),
        ));
    };

    if object.get("success").and_then(Value::as_bool) == Some(false)
        && object.get("code").and_then(Value::as_str) == Some("resource_limit_exceeded")
    {
        let resource = match object.get("resource").and_then(Value::as_str) {
            Some("read_links_count") => "read_links_count",
            Some("read_links_field_chars") => "read_links_field_chars",
            _ => "read_links_count",
        };
        let actual = match resource {
            "read_links_count" => format!(
                "{} links",
                object
                    .get("count")
                    .and_then(Value::as_u64)
                    .unwrap_or(MAX_READ_LINKS_COUNT.saturating_add(1) as u64)
            ),
            "read_links_field_chars" => format!(
                "{} characters",
                object
                    .get("char_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(MAX_READ_LINK_FIELD_CHARS.saturating_add(1) as u64)
            ),
            _ => String::new(),
        };
        return Err(ReadLinksFailure::ResourceLimit {
            resource,
            error: object
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("read_links exceeded a resource limit")
                .to_string(),
            actual,
            field: object
                .get("field")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }

    let links_value = object.get("links").cloned().ok_or_else(|| {
        ReadLinksFailure::InvalidPayload(
            "Link extraction returned a structured payload without links".to_string(),
        )
    })?;
    let links: Vec<Link> = serde_json::from_value(links_value).map_err(|error| {
        ReadLinksFailure::InvalidPayload(format!(
            "Failed to deserialize link extraction result: {}",
            error
        ))
    })?;
    validate_link_limits(&links)?;
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{
        FakeSessionBackend, ScriptEvaluation, SessionBackend, TabDescriptor,
    };
    use crate::tools::limits::{MAX_READ_LINK_FIELD_CHARS, MAX_READ_LINKS_COUNT};
    use crate::tools::{OPERATION_METRICS_METADATA_KEY, Tool, ToolContext};
    use crate::{dom::DocumentMetadata, dom::DomTree};
    use std::time::Duration;

    struct InvalidLinksPayloadBackend;

    impl SessionBackend for InvalidLinksPayloadBackend {
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
            Ok(vec![TabDescriptor {
                id: "tab-1".to_string(),
                title: "Test Tab".to_string(),
                url: "about:blank".to_string(),
            }])
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
        let tool = ReadLinksTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(ReadLinksParams {}, &mut context)
            .expect("read_links should succeed");

        assert!(result.success);
        let metrics = result.metadata[OPERATION_METRICS_METADATA_KEY]
            .as_object()
            .expect("metrics metadata should be present");
        assert_eq!(metrics["browser_evaluations"].as_u64(), Some(2));
    }

    #[test]
    fn test_read_links_tool_returns_structured_failure_for_invalid_payload() {
        let session = BrowserSession::with_test_backend(InvalidLinksPayloadBackend);
        let tool = ReadLinksTool;
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
        assert!(data["details"].is_null() || data["details"].as_object().is_none());
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("snapshot")
        );
        let metrics = result.metadata[OPERATION_METRICS_METADATA_KEY]
            .as_object()
            .expect("metrics metadata should be present on failures");
        assert_eq!(metrics["browser_evaluations"].as_u64(), Some(1));
    }

    #[test]
    fn test_parse_links_output_deserializes_resolved_urls() {
        let links = parse_links_output(Some(serde_json::json!([
            {
                "text": "Guide",
                "href": "guide/getting-started",
                "resolved_url": "https://example.test/articles/guide/getting-started"
            },
            {
                "text": "Rust",
                "href": "https://www.rust-lang.org/",
                "resolved_url": "https://www.rust-lang.org/"
            }
        ])))
        .expect("payload with resolved URLs should deserialize");

        assert_eq!(links.len(), 2);
        assert_eq!(links[0].href, "guide/getting-started");
        assert_eq!(
            links[0].resolved_url,
            "https://example.test/articles/guide/getting-started"
        );
        assert_eq!(links[1].href, "https://www.rust-lang.org/");
        assert_eq!(links[1].resolved_url, "https://www.rust-lang.org/");
    }

    #[test]
    fn test_parse_links_output_rejects_link_count_over_limit() {
        let payload = (0..=MAX_READ_LINKS_COUNT)
            .map(|index| {
                serde_json::json!({
                    "text": format!("Link {index}"),
                    "href": format!("/link-{index}"),
                    "resolved_url": format!("https://example.test/link-{index}")
                })
            })
            .collect::<Vec<_>>();

        let error = parse_links_output(Some(Value::Array(payload)))
            .expect_err("too many links should fail closed");

        match error {
            ReadLinksFailure::ResourceLimit { resource, .. } => {
                assert_eq!(resource, "read_links_count");
            }
            ReadLinksFailure::InvalidPayload(reason) => {
                panic!("unexpected invalid links payload: {reason}");
            }
        }
    }

    #[test]
    fn test_parse_links_output_rejects_field_over_limit() {
        let error = parse_links_output(Some(serde_json::json!([
            {
                "text": "x".repeat(MAX_READ_LINK_FIELD_CHARS + 1),
                "href": "/oversized",
                "resolved_url": "https://example.test/oversized"
            }
        ])))
        .expect_err("oversized link fields should fail closed");

        match error {
            ReadLinksFailure::ResourceLimit {
                resource, field, ..
            } => {
                assert_eq!(resource, "read_links_field_chars");
                assert_eq!(field.as_deref(), Some("text"));
            }
            ReadLinksFailure::InvalidPayload(reason) => {
                panic!("unexpected invalid links payload: {reason}");
            }
        }
    }
}
