//! Browser automation tools module
//!
//! This module provides a framework for browser automation tools and
//! includes implementations of common browser operations.

pub(crate) mod actionability;
pub(crate) mod browser_kernel;
pub mod click;
pub mod close;
pub mod close_tab;
pub mod evaluate;
pub mod extract;
pub mod go_back;
pub mod go_forward;
pub mod hover;
mod html_to_markdown;
pub mod input;
pub mod inspect_node;
pub mod markdown;
pub mod navigate;
pub mod new_tab;
pub mod press_key;
pub mod read_links;
mod readability_script;
pub mod screenshot;
pub mod scroll;
pub mod select;
pub(crate) mod services;
pub mod set_viewport;
pub mod snapshot;
pub mod switch_tab;
pub mod tab_list;
mod utils;
pub mod wait;

// Re-export Params types for use by MCP layer
pub use click::ClickParams;
pub use close::CloseParams;
pub use close_tab::CloseTabParams;
pub use evaluate::EvaluateParams;
pub use extract::ExtractParams;
pub use go_back::GoBackParams;
pub use go_forward::GoForwardParams;
pub use hover::HoverParams;
pub use input::InputParams;
pub use inspect_node::{InspectDetail, InspectNodeParams};
pub use markdown::GetMarkdownParams;
pub use navigate::NavigateParams;
pub use new_tab::NewTabParams;
pub use press_key::PressKeyParams;
pub use read_links::ReadLinksParams;
pub use screenshot::{ScreenshotMode, ScreenshotParams, ScreenshotRegion};
pub use scroll::ScrollParams;
pub use select::SelectParams;
pub use set_viewport::SetViewportParams;
pub use snapshot::{SnapshotMode, SnapshotParams};
pub use switch_tab::SwitchTabParams;
pub use tab_list::TabListParams;
pub use wait::WaitCondition;
pub use wait::WaitParams;

pub(crate) mod core;

pub use core::{
    DocumentActionResult, DocumentEnvelope, DocumentResult, DynTool, SnapshotScope, TabSummary,
    TargetEnvelope, TargetedActionResult, Tool, ToolContext, ToolDescriptor, ToolRegistry,
    ToolResult,
};
#[allow(unused_imports)]
pub(crate) use core::{
    DocumentEnvelopeOptions, OPERATION_METRICS_METADATA_KEY, OperationMetrics, ResolvedTarget,
    TargetResolution, actionable_cursor_for_selector, build_document_envelope, duration_micros,
    normalize_tool_outcome, resolve_target_with_cursor, tool_result_from_browser_error,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::dom::{AriaChild, AriaNode, DomTree};
    use crate::error::BrowserError;
    use serde_json::Value;

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success(Some(serde_json::json!({"url": "https://example.com"})));
        assert!(result.success);
        assert!(result.data.is_some());
        assert!(result.error.is_none());
    }

    #[test]
    fn test_tool_result_failure() {
        let result = ToolResult::failure("Test error");
        assert!(!result.success);
        assert!(result.data.is_none());
        assert_eq!(result.error, Some("Test error".to_string()));
    }

    #[test]
    fn test_tool_result_with_metadata() {
        let result = ToolResult::success(None).with_metadata("duration_ms", serde_json::json!(100));

        assert!(result.metadata.contains_key("duration_ms"));
    }

    #[test]
    fn test_tool_context_finish_attaches_operation_metrics() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let mut context = ToolContext::new(&session);
        context.record_browser_evaluation();
        context.record_poll_iteration();

        let result = context.finish(ToolResult::success(Some(serde_json::json!({
            "ok": true
        }))));

        let metrics = result.metadata[OPERATION_METRICS_METADATA_KEY]
            .as_object()
            .expect("metrics metadata should be present");
        assert_eq!(
            metrics["browser_evaluations"].as_u64(),
            Some(1),
            "browser evaluation count should be recorded"
        );
        assert_eq!(
            metrics["poll_iterations"].as_u64(),
            Some(1),
            "poll iterations should be recorded"
        );
        assert!(
            !metrics.contains_key("output_bytes"),
            "output_bytes should be omitted when exact sizing is not requested"
        );
    }

    #[test]
    fn test_build_document_envelope_records_snapshot_operation_metrics() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let dom = sample_dom();
        let mut context = ToolContext::with_dom(&session, dom);

        let envelope = build_document_envelope(&mut context, None, DocumentEnvelopeOptions::full())
            .expect("full envelope should build");
        let result = context.finish(ToolResult::success_with(envelope));

        let metrics = result.metadata[OPERATION_METRICS_METADATA_KEY]
            .as_object()
            .expect("metrics metadata should be present");
        assert!(
            metrics.contains_key("snapshot_render_micros"),
            "snapshot render timing should be recorded"
        );
        assert!(
            !metrics.contains_key("output_bytes"),
            "output_bytes should be omitted when exact sizing is not requested"
        );
    }

    #[test]
    fn test_tool_result_success_with_and_failure_with_store_structured_payloads() {
        let success = ToolResult::success_with(serde_json::json!({"ok": true}));
        assert!(success.success);
        assert_eq!(success.data, Some(serde_json::json!({"ok": true})));
        assert_eq!(success.error, None);

        let failure = ToolResult::failure_with("Boom", serde_json::json!({"code": "boom"}));
        assert!(!failure.success);
        assert_eq!(failure.error.as_deref(), Some("Boom"));
        assert_eq!(failure.data, Some(serde_json::json!({"code": "boom"})));
    }

    #[test]
    fn test_default_registry_excludes_operator_tools() {
        let registry = ToolRegistry::with_defaults();

        assert!(registry.has("snapshot"));
        assert!(registry.has("click"));
        assert!(registry.has("screenshot"));
        assert!(registry.has("set_viewport"));
        assert!(!registry.has("evaluate"));
    }

    #[test]
    fn test_all_tools_registry_includes_operator_tools() {
        let registry = ToolRegistry::with_all_tools();

        assert!(registry.has("snapshot"));
        assert!(registry.has("evaluate"));
        assert!(registry.has("screenshot"));
        assert!(registry.has("set_viewport"));
    }

    #[test]
    fn test_screenshot_schema_exposes_mode_based_managed_artifact_contract() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let descriptor = session
            .tool_registry()
            .descriptors()
            .into_iter()
            .find(|tool| tool.name == "screenshot")
            .expect("screenshot descriptor should exist");

        let params = &descriptor.parameters_schema["properties"];
        assert!(params.get("mode").is_some());
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
        assert!(output.get("clip").is_some());
    }

    #[test]
    fn test_set_viewport_schema_exposes_breakpoint_contract() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let descriptor = session
            .tool_registry()
            .descriptors()
            .into_iter()
            .find(|tool| tool.name == "set_viewport")
            .expect("set_viewport descriptor should exist");

        let params = &descriptor.parameters_schema["properties"];
        assert!(params.get("width").is_some());
        assert!(params.get("height").is_some());
        assert!(params.get("device_scale_factor").is_some());
        assert!(params.get("mobile").is_some());
        assert!(params.get("touch").is_some());
        assert!(params.get("orientation").is_some());
        assert!(params.get("tab_id").is_some());
        assert!(params.get("reset").is_some());

        let output = &descriptor.output_schema["properties"];
        assert!(output.get("tab_id").is_some());
        assert!(output.get("reset").is_some());
        assert!(output.get("emulation").is_some());
        assert!(output.get("viewport_after").is_some());
        assert!(output.get("message").is_some());
    }

    #[test]
    fn test_registry_list_names_count_and_get_are_consistent() {
        let registry = ToolRegistry::with_defaults();
        let names = registry.list_names();

        assert_eq!(registry.count(), names.len());
        assert!(names.contains(&"snapshot".to_string()));
        assert!(registry.get("snapshot").is_some());
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn test_registered_tools_expose_object_input_and_output_schemas() {
        for registry in [
            ToolRegistry::with_defaults(),
            ToolRegistry::with_all_tools(),
        ] {
            for tool in registry.all_tools() {
                let input_schema = tool.parameters_schema();
                assert_eq!(
                    input_schema.get("type").and_then(Value::as_str),
                    Some("object"),
                    "tool '{}' should expose an object input schema",
                    tool.name()
                );

                let output_schema = tool.output_schema();
                assert_eq!(
                    output_schema.get("type").and_then(Value::as_str),
                    Some("object"),
                    "tool '{}' should expose an object output schema",
                    tool.name()
                );
            }
        }
    }

    fn sample_dom() -> DomTree {
        let root = AriaNode::fragment().with_child(AriaChild::Node(Box::new(
            AriaNode::new("button", "Submit")
                .with_index(1)
                .with_box(true, Some("pointer".to_string())),
        )));
        let mut dom = DomTree::new(root);
        dom.document.document_id = "doc-1".to_string();
        dom.document.revision = "main:1".to_string();
        dom.replace_selectors(vec![String::new(), "#submit".to_string()]);
        dom
    }

    #[test]
    fn test_resolve_target_prefers_css_selector() {
        let target = resolve_target_with_cursor(
            "click",
            Some("#submit".to_string()),
            None,
            None,
            None,
            None,
        )
        .expect("selector target should resolve");

        match target {
            TargetResolution::Resolved(target) => {
                assert_eq!(
                    target,
                    ResolvedTarget {
                        method: "css".to_string(),
                        selector: "#submit".to_string(),
                        index: None,
                        node_ref: None,
                        cursor: None,
                    }
                );
                assert_eq!(target.method, "css");
            }
            TargetResolution::Failure(failure) => panic!("unexpected failure: {:?}", failure),
        }
    }

    #[test]
    fn test_resolve_target_resolves_index_via_dom() {
        let dom = sample_dom();
        let target = resolve_target_with_cursor("click", None, Some(1), None, None, Some(&dom))
            .expect("index target should resolve against DOM");

        match target {
            TargetResolution::Resolved(target) => {
                assert_eq!(
                    target,
                    ResolvedTarget {
                        method: "index".to_string(),
                        selector: "#submit".to_string(),
                        index: Some(1),
                        node_ref: Some(crate::dom::NodeRef {
                            document_id: "doc-1".to_string(),
                            revision: "main:1".to_string(),
                            index: 1,
                        }),
                        cursor: Some(crate::dom::Cursor {
                            node_ref: crate::dom::NodeRef {
                                document_id: "doc-1".to_string(),
                                revision: "main:1".to_string(),
                                index: 1,
                            },
                            selector: "#submit".to_string(),
                            index: 1,
                            role: "button".to_string(),
                            name: "Submit".to_string(),
                        }),
                    }
                );
                assert_eq!(target.method, "index");
                let envelope = target.to_target_envelope();
                assert_eq!(
                    envelope
                        .cursor
                        .as_ref()
                        .map(|cursor| cursor.selector.as_str()),
                    Some("#submit")
                );
            }
            TargetResolution::Failure(failure) => panic!("unexpected failure: {:?}", failure),
        }
    }

    #[test]
    fn test_resolve_target_with_cursor_accepts_cursor_input() {
        let dom = sample_dom();
        let cursor = dom.cursor_for_index(1).expect("cursor should exist");
        let target = resolve_target_with_cursor(
            "inspect_node",
            None,
            None,
            None,
            Some(cursor.clone()),
            Some(&dom),
        )
        .expect("cursor target should resolve against DOM");

        match target {
            TargetResolution::Resolved(target) => {
                assert_eq!(target.method, "cursor");
                assert_eq!(target.selector, "#submit");
                assert_eq!(target.cursor.as_ref(), Some(&cursor));
                let envelope = target.to_target_envelope();
                assert_eq!(envelope.method, "cursor");
                assert_eq!(envelope.resolution_status, "exact");
                assert_eq!(envelope.recovered_from, None);
            }
            TargetResolution::Failure(failure) => panic!("unexpected failure: {:?}", failure),
        }
    }

    #[test]
    fn test_resolve_target_with_cursor_enriches_actionable_selector() {
        let dom = sample_dom();
        let target = resolve_target_with_cursor(
            "inspect_node",
            Some("#submit".to_string()),
            None,
            None,
            None,
            Some(&dom),
        )
        .expect("selector target should resolve");

        match target {
            TargetResolution::Resolved(target) => {
                assert_eq!(target.method, "css");
                assert_eq!(target.selector, "#submit");
                assert!(target.cursor.is_some());
            }
            TargetResolution::Failure(failure) => panic!("unexpected failure: {:?}", failure),
        }
    }

    #[test]
    fn test_resolve_target_rejects_invalid_combinations() {
        let both = resolve_target_with_cursor(
            "click",
            Some("#submit".to_string()),
            Some(1),
            None,
            None,
            None,
        )
        .expect("invalid combination should return tool failure");
        assert!(matches!(both, TargetResolution::Failure(_)));

        let neither = resolve_target_with_cursor("click", None, None, None, None, None)
            .expect("missing target should return tool failure");
        assert!(matches!(neither, TargetResolution::Failure(_)));
    }

    #[test]
    fn test_resolve_target_errors_for_missing_index() {
        let dom = sample_dom();
        let result = resolve_target_with_cursor("click", None, Some(9), None, None, Some(&dom));

        assert!(matches!(result, Err(BrowserError::ElementNotFound(_))));
    }

    #[test]
    fn test_resolve_target_rejects_stale_node_ref() {
        let dom = sample_dom();
        let result = resolve_target_with_cursor(
            "click",
            None,
            None,
            Some(crate::dom::NodeRef {
                document_id: "doc-1".to_string(),
                revision: "main:0".to_string(),
                index: 1,
            }),
            None,
            Some(&dom),
        )
        .expect("stale node ref should become tool failure");

        match result {
            TargetResolution::Failure(failure) => {
                let data = failure
                    .data
                    .expect("stale node ref failure should include structured data");
                assert_eq!(data["code"].as_str(), Some("stale_node_ref"));
                assert_eq!(
                    data["details"]["resolution"]["status"].as_str(),
                    Some("unrecoverable_stale")
                );
                assert_eq!(
                    data["details"]["resolution"]["recovered_from"].as_str(),
                    Some("node_ref")
                );
                assert_eq!(
                    data["details"]["resolution"]["selector_rebound_attempted"].as_bool(),
                    Some(false)
                );
                assert_eq!(
                    data["recovery"]["suggested_tool"].as_str(),
                    Some("snapshot")
                );
                assert!(data["recovery"]["suggested_selector"].is_null());
            }
            TargetResolution::Resolved(target) => {
                panic!("unexpected resolved stale node ref target: {target:?}")
            }
        }
    }

    #[test]
    fn test_resolve_target_rebinds_stale_cursor_with_machine_usable_metadata() {
        let dom = sample_dom();
        let mut stale_cursor = dom.cursor_for_index(1).expect("cursor should exist");
        stale_cursor.node_ref.revision = "main:0".to_string();

        let result = resolve_target_with_cursor(
            "inspect_node",
            None,
            None,
            None,
            Some(stale_cursor),
            Some(&dom),
        )
        .expect("stale cursor should resolve");

        match result {
            TargetResolution::Resolved(target) => {
                let envelope = target.to_target_envelope();
                assert_eq!(envelope.method, "cursor");
                assert_eq!(envelope.resolution_status, "selector_rebound");
                assert_eq!(envelope.recovered_from.as_deref(), Some("cursor"));
                assert_eq!(envelope.selector.as_deref(), Some("#submit"));
                assert_eq!(envelope.index, Some(1));
                assert_eq!(
                    envelope
                        .cursor
                        .as_ref()
                        .map(|cursor| cursor.node_ref.revision.as_str()),
                    Some("main:1")
                );
            }
            TargetResolution::Failure(failure) => panic!("unexpected failure: {:?}", failure),
        }
    }

    #[test]
    fn test_resolve_target_reports_recovery_hints_for_unrecoverable_stale_cursor() {
        let dom = sample_dom();
        let mut stale_cursor = dom.cursor_for_index(1).expect("cursor should exist");
        stale_cursor.node_ref.revision = "main:0".to_string();
        stale_cursor.selector = "#missing".to_string();

        let result = resolve_target_with_cursor(
            "inspect_node",
            None,
            None,
            None,
            Some(stale_cursor),
            Some(&dom),
        )
        .expect("stale cursor should become tool failure");

        match result {
            TargetResolution::Failure(failure) => {
                let data = failure
                    .data
                    .expect("stale cursor failure should include structured data");
                assert_eq!(data["code"].as_str(), Some("stale_node_ref"));
                assert_eq!(
                    data["details"]["resolution"]["status"].as_str(),
                    Some("unrecoverable_stale")
                );
                assert_eq!(
                    data["details"]["resolution"]["recovered_from"].as_str(),
                    Some("cursor")
                );
                assert_eq!(
                    data["details"]["resolution"]["selector_rebound_attempted"].as_bool(),
                    Some(true)
                );
                assert_eq!(
                    data["recovery"]["suggested_tool"].as_str(),
                    Some("snapshot")
                );
                assert_eq!(
                    data["recovery"]["suggested_selector"].as_str(),
                    Some("#missing")
                );
            }
            TargetResolution::Resolved(target) => {
                panic!("unexpected resolved stale cursor target: {target:?}")
            }
        }
    }
}
