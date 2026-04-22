use crate::dom::{Cursor, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelope, TargetEnvelope, TargetResolution, Tool, ToolContext, ToolResult,
    actionability::ActionabilityPredicate, browser_kernel::render_browser_kernel_script,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const CLICK_JS: &str = include_str!("click.js");

pub(crate) use crate::tools::services::interaction::{
    ActionabilityWaitState, DEFAULT_ACTIONABILITY_TIMEOUT_MS, TargetStatus,
    build_actionability_failure, build_interaction_failure, build_interaction_handoff,
    decode_action_result, resolve_interaction_target, wait_for_actionability,
};

/// Parameters for the click tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

/// Tool for clicking elements
#[derive(Default)]
pub struct ClickTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClickOutput {
    #[serde(flatten)]
    pub envelope: DocumentEnvelope,
    pub action: String,
    pub target_before: TargetEnvelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_after: Option<TargetEnvelope>,
    pub target_status: TargetStatus,
}

impl Tool for ClickTool {
    type Params = ClickParams;
    type Output = ClickOutput;

    fn name(&self) -> &str {
        "click"
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
        Ok(context.finish(ToolResult::success_with(ClickOutput {
            envelope: handoff.envelope,
            action: "click".to_string(),
            target_before: handoff.target_before,
            target_after: handoff.target_after,
            target_status: handoff.target_status,
        })))
    }
}

fn build_click_js(config: &serde_json::Value) -> String {
    render_browser_kernel_script(CLICK_JS, "__CLICK_CONFIG__", config)
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
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::tools::{OPERATION_METRICS_METADATA_KEY, Tool, ToolContext};

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
        let tool = super::ClickTool::default();
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
}
