//! MCP (Model Context Protocol) server implementation for browser automation
//!
//! This module provides rmcp-compatible tools by wrapping the existing tool implementations.

pub mod handler;
pub use handler::BrowserServer;

use crate::tools::ToolResult as InternalToolResult;
#[cfg(test)]
use crate::tools::{ToolContext, normalize_tool_outcome};
use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content, Meta},
};

fn merge_error_details(
    existing: Option<serde_json::Value>,
    extras: serde_json::Map<String, serde_json::Value>,
) -> Option<serde_json::Value> {
    if extras.is_empty() {
        return existing;
    }

    match existing {
        Some(serde_json::Value::Object(mut details)) => {
            details.extend(extras);
            Some(serde_json::Value::Object(details))
        }
        Some(other) => Some(serde_json::json!({
            "payload": other,
            "extra": extras,
        })),
        None => Some(serde_json::Value::Object(extras)),
    }
}

fn normalize_structured_error(
    error_msg: String,
    data: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut normalized = serde_json::Map::new();
    let mut extras = serde_json::Map::new();
    let mut details = None;

    match data {
        Some(serde_json::Value::Object(mut object)) => {
            let code = object
                .remove("code")
                .unwrap_or_else(|| serde_json::Value::String("tool_error".to_string()));
            let error = object
                .remove("error")
                .unwrap_or_else(|| serde_json::Value::String(error_msg.clone()));
            let document = object.remove("document");
            let target = object.remove("target");
            let recovery = object.remove("recovery");
            details = object.remove("details");
            extras = object;

            normalized.insert("code".to_string(), code);
            normalized.insert("error".to_string(), error);
            if let Some(document) = document
                && !document.is_null()
            {
                normalized.insert("document".to_string(), document);
            }
            if let Some(target) = target
                && !target.is_null()
            {
                normalized.insert("target".to_string(), target);
            }
            if let Some(recovery) = recovery
                && !recovery.is_null()
            {
                normalized.insert("recovery".to_string(), recovery);
            }
        }
        Some(other) => {
            normalized.insert(
                "code".to_string(),
                serde_json::Value::String("tool_error".to_string()),
            );
            normalized.insert(
                "error".to_string(),
                serde_json::Value::String(error_msg.clone()),
            );
            details = Some(other);
        }
        None => {
            normalized.insert(
                "code".to_string(),
                serde_json::Value::String("tool_error".to_string()),
            );
            normalized.insert(
                "error".to_string(),
                serde_json::Value::String(error_msg.clone()),
            );
        }
    }

    if let Some(details) = merge_error_details(details, extras)
        && !details.is_null()
    {
        normalized.insert("details".to_string(), details);
    }

    serde_json::Value::Object(normalized)
}

fn with_metadata(
    result: CallToolResult,
    metadata: std::collections::HashMap<String, serde_json::Value>,
) -> CallToolResult {
    if metadata.is_empty() {
        return result;
    }

    result.with_meta(Some(Meta(metadata.into_iter().collect())))
}

/// Convert internal ToolResult to MCP CallToolResult
pub(crate) fn convert_result(result: InternalToolResult) -> Result<CallToolResult, McpError> {
    let InternalToolResult {
        success,
        data,
        error,
        metadata,
    } = result;

    if success {
        let result = if let Some(data) = data {
            CallToolResult::structured(data)
        } else {
            CallToolResult::success(vec![Content::text("Success")])
        };

        Ok(with_metadata(result, metadata))
    } else {
        let error_msg = error.unwrap_or_else(|| "Unknown error".to_string());
        let structured_error = normalize_structured_error(error_msg, data);
        let result = CallToolResult::structured_error(structured_error);

        Ok(with_metadata(result, metadata))
    }
}

#[cfg(test)]
fn convert_tool_outcome(
    outcome: crate::error::Result<InternalToolResult>,
    context: &ToolContext<'_>,
) -> Result<CallToolResult, McpError> {
    let result = normalize_tool_outcome(outcome, context).map_err(mcp_internal_error)?;
    convert_result(result)
}

pub(crate) fn mcp_internal_error(error: impl std::fmt::Display) -> McpError {
    McpError::internal_error(error.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::{convert_result, convert_tool_outcome};
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::error::BrowserError;
    use crate::mcp::BrowserServer;
    use crate::tools::{
        OPERATION_METRICS_METADATA_KEY, ToolContext, ToolResult as InternalToolResult,
    };
    use serde_json::json;
    use std::collections::HashMap;

    const MAX_TOOL_DESCRIPTION_LEN: usize = 96;

    #[test]
    fn test_convert_result_preserves_structured_success() {
        let result = convert_result(
            InternalToolResult::success(Some(json!({
                "url": "https://example.com",
                "title": "Example Domain",
            })))
            .with_metadata("duration_ms", json!(12)),
        )
        .expect("success result should convert");

        assert_eq!(
            result.structured_content,
            Some(json!({
                "url": "https://example.com",
                "title": "Example Domain",
            }))
        );
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            result.meta.as_ref().unwrap().0.get("duration_ms"),
            Some(&json!(12))
        );

        let text = result
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|content| content.text.as_str())
            .expect("structured result should keep text fallback");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(text).expect("text fallback should be JSON"),
            json!({
                "url": "https://example.com",
                "title": "Example Domain",
            })
        );
    }

    #[test]
    fn test_convert_result_uses_structured_tool_error() {
        let result = convert_result(
            InternalToolResult::failure("Element not found").with_metadata("tool", json!("click")),
        )
        .expect("tool failures should stay in CallToolResult");

        assert_eq!(
            result.structured_content,
            Some(json!({
                "code": "tool_error",
                "error": "Element not found",
            }))
        );
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result.meta.as_ref().unwrap().0.get("tool"),
            Some(&json!("click"))
        );
    }

    #[test]
    fn test_convert_result_without_data_keeps_text_success() {
        let result = convert_result(InternalToolResult::success(None))
            .expect("empty success should convert");

        assert_eq!(result.structured_content, None);
        assert_eq!(result.is_error, Some(false));

        let text = result
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|content| content.text.as_ref());
        assert_eq!(text, Some("Success"));
    }

    #[test]
    fn test_convert_tool_outcome_preserves_structured_tool_failures_from_browser_errors() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let mut context = ToolContext::new(&session);
        context.record_browser_evaluation();

        let result = convert_tool_outcome(
            Err(BrowserError::ToolExecutionFailed {
                tool: "close".to_string(),
                reason: "Session close encountered 1 error(s)".to_string(),
            }),
            &context,
        )
        .expect("tool execution failures should stay structured");

        assert_eq!(
            result.structured_content,
            Some(json!({
                "code": "tool_execution_failed",
                "error": "Session close encountered 1 error(s)",
                "details": {
                    "tool": "close",
                },
            }))
        );
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result
                .meta
                .as_ref()
                .and_then(|meta| meta.0.get(OPERATION_METRICS_METADATA_KEY))
                .and_then(|metrics| metrics.get("browser_evaluations"))
                .and_then(|value| value.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn test_convert_tool_outcome_keeps_internal_errors_for_browser_infra_failures() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let context = ToolContext::new(&session);

        let error = convert_tool_outcome(
            Err(BrowserError::ChromeError(
                "CDP connection dropped".to_string(),
            )),
            &context,
        )
        .expect_err("browser infrastructure failures should remain internal");

        assert!(error.to_string().contains("CDP connection dropped"));
    }

    #[test]
    fn test_mcp_tools_advertise_output_schemas() {
        let tools = BrowserServer::from_session(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        ))
        .list_mcp_tools();

        assert!(!tools.is_empty(), "expected MCP tools to be registered");

        let mut missing_output_schema = Vec::new();
        let mut non_object_output_schema = Vec::new();

        for tool in tools {
            match tool.output_schema.as_ref() {
                None => missing_output_schema.push(tool.name.to_string()),
                Some(schema) => {
                    if schema.get("type").and_then(|value| value.as_str()) != Some("object") {
                        non_object_output_schema.push(tool.name.to_string());
                    }
                }
            }
        }

        assert!(
            missing_output_schema.is_empty(),
            "MCP tools missing output_schema: {missing_output_schema:?}"
        );
        assert!(
            non_object_output_schema.is_empty(),
            "MCP tools with non-object output_schema: {non_object_output_schema:?}"
        );
    }

    #[test]
    fn test_live_mcp_input_schemas_match_registered_tool_descriptors() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let descriptor_schemas: HashMap<String, serde_json::Value> = session
            .tool_registry()
            .descriptors()
            .into_iter()
            .map(|descriptor| (descriptor.name.to_string(), descriptor.parameters_schema))
            .collect();

        let tools = BrowserServer::from_session(session).list_mcp_tools();

        for tool in tools {
            let expected = descriptor_schemas
                .get(tool.name.as_ref())
                .unwrap_or_else(|| panic!("missing descriptor schema for {}", tool.name));
            let advertised = serde_json::Value::Object(tool.input_schema.as_ref().clone());
            assert_eq!(
                &advertised, expected,
                "live MCP input schema drifted from registered descriptor for {}",
                tool.name
            );
        }
    }

    #[test]
    fn test_mcp_tool_descriptions_are_concise() {
        let tools = BrowserServer::from_session(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        ))
        .list_mcp_tools();

        let problems: Vec<String> = tools
            .iter()
            .filter_map(|tool| {
                let description = tool.description.as_deref().unwrap_or("").trim();
                let char_count = description.chars().count();

                if description.is_empty() {
                    Some(format!("{} is missing a description", tool.name))
                } else if char_count > MAX_TOOL_DESCRIPTION_LEN {
                    Some(format!(
                        "{} description is {} chars: {}",
                        tool.name, char_count, description
                    ))
                } else {
                    None
                }
            })
            .collect();

        assert!(
            problems.is_empty(),
            "MCP tool descriptions must stay concise: {problems:?}"
        );
    }

    #[test]
    fn test_mcp_tool_descriptions_keep_orchestration_hints() {
        let descriptions: HashMap<String, String> = BrowserServer::from_session(
            BrowserSession::with_test_backend(FakeSessionBackend::new()),
        )
        .list_mcp_tools()
        .into_iter()
        .map(|tool| {
            (
                tool.name.to_string(),
                tool.description.as_deref().unwrap_or("").to_string(),
            )
        })
        .collect();

        let expectations = [
            ("get_markdown", ["snapshot"].as_slice()),
            ("extract", ["markdown"].as_slice()),
            ("read_links", ["click", "navigate"].as_slice()),
            (
                "snapshot",
                ["viewport", "inspect_node", "click", "wait"].as_slice(),
            ),
            (
                "inspect_node",
                ["target", "selector", "cursor", "snapshot"].as_slice(),
            ),
            ("click", ["snapshot", "wait"].as_slice()),
            ("input", ["snapshot", "press_key"].as_slice()),
            ("select", ["snapshot", "wait"].as_slice()),
            ("new_tab", ["tab_list", "switch_tab"].as_slice()),
            ("tab_list", ["switch_tab"].as_slice()),
            ("switch_tab", ["tab_list", "snapshot"].as_slice()),
            (
                "screenshot",
                ["managed", "target", "mode", "scale"].as_slice(),
            ),
        ];

        for (tool_name, keywords) in expectations {
            let description = descriptions
                .get(tool_name)
                .unwrap_or_else(|| panic!("missing description for {tool_name}"));

            for keyword in keywords {
                assert!(
                    description.contains(keyword),
                    "{tool_name} description should mention '{keyword}', got: {description}"
                );
            }
        }
    }

    #[test]
    fn test_mcp_surface_exports_default_extraction_tools() {
        let tool_names: Vec<String> = BrowserServer::from_session(
            BrowserSession::with_test_backend(FakeSessionBackend::new()),
        )
        .list_mcp_tools()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();

        assert!(
            tool_names.iter().any(|name| name == "extract"),
            "extract should be exported via MCP"
        );
        assert!(
            tool_names.iter().any(|name| name == "read_links"),
            "read_links should be exported via MCP"
        );
        assert!(
            tool_names.iter().any(|name| name == "screenshot"),
            "screenshot should be exported via MCP"
        );
    }

    #[test]
    fn test_mcp_surface_reflects_operator_tool_registration() {
        let default_tool_names: Vec<String> = BrowserServer::from_session(
            BrowserSession::with_test_backend(FakeSessionBackend::new()),
        )
        .list_mcp_tools()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();

        assert!(!default_tool_names.iter().any(|name| name == "evaluate"));
        assert!(default_tool_names.iter().any(|name| name == "screenshot"));

        let mut session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        session.tool_registry_mut().register_operator_tools();

        let tool_names: Vec<String> = BrowserServer::from_session(session)
            .list_mcp_tools()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();

        assert!(tool_names.iter().any(|name| name == "evaluate"));
        assert!(tool_names.iter().any(|name| name == "screenshot"));
    }

    #[test]
    fn test_screenshot_schema_exposes_managed_artifact_contract() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let descriptor = session
            .tool_registry()
            .descriptors()
            .into_iter()
            .find(|tool| tool.name == "screenshot")
            .expect("screenshot descriptor should exist");

        assert!(descriptor.description.contains("managed"));
        assert!(descriptor.description.contains("mode"));
        assert!(descriptor.description.contains("scale"));
        assert!(!descriptor.description.contains("confirm_unsafe"));
        assert!(!descriptor.description.contains("path"));

        let params = &descriptor.parameters_schema["properties"];
        assert!(params.get("mode").is_some());
        assert!(params.get("scale").is_some());
        assert!(params.get("tab_id").is_some());
        assert!(params.get("target").is_some());
        assert!(params.get("region").is_some());
        assert!(params.get("path").is_none());
        assert!(params.get("full_page").is_none());
        assert!(params.get("confirm_unsafe").is_none());

        let output = &descriptor.output_schema["properties"];
        assert!(output.get("artifact_uri").is_some());
        assert!(output.get("artifact_path").is_some());
        assert!(output.get("format").is_some());
        assert!(output.get("mime_type").is_some());
        assert!(output.get("byte_count").is_some());
        assert!(output.get("width").is_some());
        assert!(output.get("height").is_some());
        assert!(output.get("css_width").is_some());
        assert!(output.get("css_height").is_some());
        assert!(output.get("device_pixel_ratio").is_some());
        assert!(output.get("pixel_scale").is_some());
        assert!(output.get("revealed_from_offscreen").is_some());
        assert!(output.get("clip").is_some());
    }

    #[test]
    fn test_live_mcp_target_schemas_advertise_string_and_structured_target_forms() {
        let tools: HashMap<String, rmcp::model::Tool> = BrowserServer::from_session(
            BrowserSession::with_test_backend(FakeSessionBackend::new()),
        )
        .list_mcp_tools()
        .into_iter()
        .map(|tool| (tool.name.to_string(), tool))
        .collect();

        for tool_name in ["inspect_node", "screenshot", "click"] {
            let tool = tools
                .get(tool_name)
                .unwrap_or_else(|| panic!("missing MCP tool {tool_name}"));
            let schema = serde_json::Value::Object(tool.input_schema.as_ref().clone());
            let properties = schema["properties"]
                .as_object()
                .unwrap_or_else(|| panic!("{tool_name} should advertise object properties"));
            let target = properties
                .get("target")
                .unwrap_or_else(|| panic!("{tool_name} should advertise target"));

            let serialized =
                serde_json::to_string(&schema).expect("schema should serialize for assertions");
            assert!(
                serialized.contains("\"type\":\"string\""),
                "{tool_name} schema should advertise plain selector string targeting"
            );
            assert!(
                serialized.contains("\"kind\""),
                "{tool_name} schema should advertise the target kind discriminator"
            );
            assert!(
                serialized.contains("\"selector\""),
                "{tool_name} schema should advertise selector targeting"
            );
            assert!(
                serialized.contains("\"cursor\""),
                "{tool_name} schema should advertise cursor targeting"
            );
            assert!(
                target.get("$ref").is_some()
                    || target.get("oneOf").is_some()
                    || target.get("anyOf").is_some()
                    || target.get("any_of").is_some(),
                "{tool_name} target should stay schema-driven rather than a bare scalar field"
            );
        }
    }

    #[test]
    fn test_snapshot_schema_exposes_mode_and_scope_contract() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let descriptor = session
            .tool_registry()
            .descriptors()
            .into_iter()
            .find(|tool| tool.name == "snapshot")
            .expect("snapshot descriptor should exist");

        let params = &descriptor.parameters_schema["properties"];
        assert!(params.get("mode").is_some());
        assert!(params.get("incremental").is_none());

        let output = &descriptor.output_schema["properties"];
        assert!(output.get("scope").is_some());
        assert!(output.get("global_interactive_count").is_some());
        let output_schema =
            serde_json::to_string(&descriptor.output_schema).expect("schema should serialize");
        assert!(output_schema.contains("locality_fallback_reason"));
    }

    #[test]
    fn test_inspect_node_schema_exposes_target_resolution_metadata() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let descriptor = session
            .tool_registry()
            .descriptors()
            .into_iter()
            .find(|tool| tool.name == "inspect_node")
            .expect("inspect_node descriptor should exist");

        let schema =
            serde_json::to_string(&descriptor.output_schema).expect("schema should serialize");
        assert!(schema.contains("resolution_status"));
        assert!(schema.contains("recovered_from"));
        assert!(schema.contains("cursor"));
        assert!(schema.contains("node_ref"));
    }
}
