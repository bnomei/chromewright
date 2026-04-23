use crate::dom::{Cursor, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    TargetResolution, Tool, ToolContext, ToolResult,
    actionability::ActionabilityPredicate,
    browser_kernel::render_browser_kernel_script,
    core::PublicTarget,
    core::TargetedActionResult,
    services::interaction::{
        ActionabilityWaitState, DEFAULT_ACTIONABILITY_TIMEOUT_MS, build_actionability_failure,
        build_interaction_failure, build_interaction_handoff, decode_action_result,
        resolve_interaction_target, wait_for_actionability,
    },
};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::OnceLock;

const CLICK_JS: &str = include_str!("click.js");
static CLICK_SHELL: OnceLock<crate::tools::browser_kernel::BrowserKernelTemplateShell> =
    OnceLock::new();

/// Parameters for the click tool
#[derive(Debug, Clone, Serialize)]
pub struct ClickParams {
    /// CSS selector (use either this or index, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Element index from DOM tree (use either this or selector, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Revision-scoped node reference from the snapshot tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    /// Cursor from the snapshot or inspect_node tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictClickParams {
    /// Target to activate.
    pub target: PublicTarget,
}

impl From<StrictClickParams> for ClickParams {
    fn from(params: StrictClickParams) -> Self {
        let (selector, cursor) = params.target.into_selector_or_cursor();
        Self {
            selector,
            index: None,
            node_ref: None,
            cursor,
        }
    }
}

impl<'de> Deserialize<'de> for ClickParams {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictClickParams::deserialize(deserializer).map(Into::into)
    }
}

impl JsonSchema for ClickParams {
    fn schema_name() -> Cow<'static, str> {
        "ClickParams".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        StrictClickParams::json_schema(generator)
    }
}

/// Tool for clicking elements
#[derive(Default)]
pub struct ClickTool;

impl Tool for ClickTool {
    type Params = ClickParams;
    type Output = TargetedActionResult;

    fn name(&self) -> &str {
        "click"
    }

    fn description(&self) -> &str {
        "Activate an element. Usually after snapshot; next wait or snapshot."
    }

    fn execute_typed(&self, params: ClickParams, context: &mut ToolContext) -> Result<ToolResult> {
        let ClickParams {
            selector,
            index,
            node_ref,
            cursor,
        } = params;
        let target = match resolve_interaction_target(
            "click", selector, index, node_ref, cursor, context,
        )? {
            TargetResolution::Resolved(target) => target,
            TargetResolution::Failure(failure) => return Ok(context.finish(failure)),
        };

        let predicates = click_actionability_predicates();
        match wait_for_actionability(
            context,
            &target,
            predicates,
            DEFAULT_ACTIONABILITY_TIMEOUT_MS,
        )? {
            ActionabilityWaitState::Ready => {}
            ActionabilityWaitState::TimedOut(probe) => {
                return build_actionability_failure(
                    "click",
                    context.session,
                    &target,
                    &probe,
                    predicates,
                    None,
                )
                .map(|result| context.finish(result));
            }
        }

        let config = serde_json::json!({
            "selector": target.selector,
            "target_index": target.cursor.as_ref().map(|cursor| cursor.index).or(target.index),
        });
        let click_js = build_click_js(&config);
        context.record_browser_evaluation();
        let result = context
            .session
            .evaluate(&click_js, false)
            .map_err(|e| match e {
                BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
                    tool: "click".to_string(),
                    reason,
                },
                other => other,
            })?;
        let action_result = decode_action_result(
            result.value,
            serde_json::json!({
                "success": false,
                "code": "target_detached",
                "error": "Element is no longer present"
            }),
        )?;

        if action_result["success"].as_bool() != Some(true) {
            let code = action_result["code"]
                .as_str()
                .unwrap_or("target_detached")
                .to_string();
            let error = action_result["error"]
                .as_str()
                .unwrap_or("Click failed")
                .to_string();
            return build_interaction_failure(
                "click",
                context.session,
                &target,
                code,
                error,
                Vec::new(),
                None,
            )
            .map(|result| context.finish(result));
        }

        let handoff = build_interaction_handoff(context, &target)?;
        Ok(
            context.finish(ToolResult::success_with(TargetedActionResult::new(
                "click",
                handoff.document,
                handoff.target_before,
                handoff.target_after,
                handoff.target_status,
            ))),
        )
    }
}

fn build_click_js(config: &serde_json::Value) -> String {
    render_browser_kernel_script(&CLICK_SHELL, CLICK_JS, "__CLICK_CONFIG__", config)
}

fn click_actionability_predicates() -> &'static [ActionabilityPredicate] {
    &[
        ActionabilityPredicate::Present,
        ActionabilityPredicate::Visible,
        ActionabilityPredicate::Enabled,
        ActionabilityPredicate::Stable,
        ActionabilityPredicate::ReceivesEvents,
        ActionabilityPredicate::UnobscuredCenter,
    ]
}

#[cfg(test)]
mod tests {
    use super::ClickParams;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::tools::{OPERATION_METRICS_METADATA_KEY, Tool, ToolContext};
    use schemars::schema_for;
    use serde_json::json;

    use super::build_click_js;

    #[test]
    fn test_click_js_prefers_selector_before_target_index() {
        let click_js = build_click_js(&serde_json::json!({
            "selector": "#save",
            "target_index": 2,
        }));

        assert!(click_js.contains("function resolveTargetMatch(config, options)"));
        assert!(click_js.contains("const element = resolveTargetElement(config);"));
        assert!(click_js.contains("querySelectorAcrossScopes("));
        assert!(click_js.contains("searchActionableIndex(config.target_index)"));
    }

    #[test]
    fn test_click_tool_executes_against_fake_backend_and_attaches_metrics() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = super::ClickTool;
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                super::ClickParams {
                    selector: Some("#fake-target".to_string()),
                    index: None,
                    node_ref: None,
                    cursor: None,
                },
                &mut context,
            )
            .expect("click should succeed");

        assert!(result.success);
        assert!(result.metadata.contains_key(OPERATION_METRICS_METADATA_KEY));
        let metrics = result.metadata[OPERATION_METRICS_METADATA_KEY]
            .as_object()
            .expect("metrics metadata should be present");
        assert!(
            metrics["browser_evaluations"].as_u64().unwrap_or_default() > 0,
            "click should record browser evaluations"
        );
    }

    #[test]
    fn test_click_params_deserializes_strict_target_selector() {
        let params: ClickParams = serde_json::from_value(json!({
            "target": {
                "kind": "selector",
                "selector": "#save"
            }
        }))
        .expect("strict selector target should deserialize");

        assert_eq!(params.selector.as_deref(), Some("#save"));
        assert_eq!(params.index, None);
        assert_eq!(params.node_ref, None);
        assert_eq!(params.cursor, None);
    }

    #[test]
    fn test_click_params_deserializes_plain_string_target_selector() {
        let params: ClickParams = serde_json::from_value(json!({
            "target": "#save"
        }))
        .expect("plain string selector target should deserialize");

        assert_eq!(params.selector.as_deref(), Some("#save"));
        assert_eq!(params.index, None);
        assert_eq!(params.node_ref, None);
        assert_eq!(params.cursor, None);
    }

    #[test]
    fn test_click_params_rejects_legacy_public_target_fields() {
        let error = serde_json::from_value::<ClickParams>(json!({
            "selector": "#save"
        }))
        .expect_err("legacy selector field should be rejected");
        assert!(error.to_string().contains("unknown field `selector`"));

        let error = serde_json::from_value::<ClickParams>(json!({
            "target": {
                "kind": "selector",
                "selector": "#save"
            },
            "index": 1
        }))
        .expect_err("legacy index field should be rejected");
        assert!(error.to_string().contains("unknown field `index`"));

        let error = serde_json::from_value::<ClickParams>(json!({
            "target": {
                "kind": "selector",
                "selector": "#save"
            },
            "node_ref": {
                "document_id": "doc-1",
                "revision": "main:1",
                "index": 1
            }
        }))
        .expect_err("legacy node_ref field should be rejected");
        assert!(error.to_string().contains("unknown field `node_ref`"));
    }

    #[test]
    fn test_click_params_schema_exposes_only_target_property() {
        let schema = schema_for!(ClickParams);
        let schema_json = serde_json::to_value(&schema).expect("schema should serialize");
        let properties = schema_json
            .get("properties")
            .and_then(|value| value.as_object())
            .expect("click params schema should expose properties");

        assert!(properties.contains_key("target"));
        assert!(!properties.contains_key("selector"));
        assert!(!properties.contains_key("index"));
        assert!(!properties.contains_key("node_ref"));
        assert!(!properties.contains_key("cursor"));
        assert_eq!(
            schema_json
                .get("required")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.as_str()),
            Some("target")
        );

        let target_schema = properties
            .get("target")
            .expect("target property should be present");
        let target_json =
            serde_json::to_string(target_schema).expect("target schema should serialize");
        assert!(target_json.contains("$ref") || target_json.contains("oneOf"));

        let full_schema_json =
            serde_json::to_string(&schema_json).expect("full click schema should serialize");
        assert!(full_schema_json.contains("\"kind\""));
        assert!(full_schema_json.contains("\"selector\""));
        assert!(full_schema_json.contains("\"cursor\""));
    }
}
