use crate::browser::BrowserSession;
use crate::dom::{Cursor, DocumentMetadata, DomTree, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelope, ResolvedTarget, TargetEnvelope, TargetResolution, ToolContext, ToolResult,
    actionability::{
        ActionabilityDiagnostics, ActionabilityPredicate, ActionabilityProbeResult,
        ActionabilityRequest, probe_actionability,
    },
    browser_kernel::render_browser_kernel_script,
    duration_micros, resolve_target_with_cursor,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const TARGET_EXISTS_TEMPLATE_JS: &str = include_str!("../target_exists.js");
const SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS: &str = include_str!("../scroll_target_into_view.js");
pub(crate) const DEFAULT_ACTIONABILITY_TIMEOUT_MS: u64 = 5_000;
const ACTIONABILITY_POLL_INTERVAL_MS: u64 = 50;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    Same,
    Rebound,
    Detached,
    Unknown,
}

pub(crate) enum ActionabilityWaitState {
    Ready,
    TimedOut(ActionabilityProbeResult),
}

pub(crate) struct InteractionHandoff {
    pub envelope: DocumentEnvelope,
    pub target_before: TargetEnvelope,
    pub target_after: Option<TargetEnvelope>,
    pub target_status: TargetStatus,
}

pub(crate) fn resolve_interaction_target(
    tool: &str,
    selector: Option<String>,
    index: Option<usize>,
    node_ref: Option<NodeRef>,
    cursor: Option<Cursor>,
    context: &mut ToolContext,
) -> Result<TargetResolution> {
    let dom = Some(context.get_dom()?);
    resolve_target_with_cursor(tool, selector, index, node_ref, cursor, dom)
}

pub(crate) fn wait_for_actionability(
    context: &mut ToolContext,
    target: &ResolvedTarget,
    predicates: &[ActionabilityPredicate],
    timeout_ms: u64,
) -> Result<ActionabilityWaitState> {
    let start = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);
    let requested_predicates = requested_actionability_predicates(predicates);

    loop {
        context.record_poll_iteration();
        context.record_browser_evaluation();
        let probe = probe_actionability(
            context.session,
            &ActionabilityRequest {
                selector: &target.selector,
                target_index: interaction_target_index(target),
                predicates: requested_predicates.as_slice(),
                expected_text: None,
                expected_value: None,
            },
        )?;

        if should_scroll_target_into_view(&probe, predicates) {
            scroll_target_into_view(context, target)?;
            std::thread::sleep(Duration::from_millis(ACTIONABILITY_POLL_INTERVAL_MS));
            continue;
        }

        if predicates
            .iter()
            .all(|predicate| probe.predicate(*predicate) == Some(true))
        {
            return Ok(ActionabilityWaitState::Ready);
        }

        if start.elapsed() >= timeout {
            return Ok(ActionabilityWaitState::TimedOut(probe));
        }

        std::thread::sleep(Duration::from_millis(ACTIONABILITY_POLL_INTERVAL_MS));
    }
}

pub(crate) fn build_interaction_handoff(
    context: &mut ToolContext,
    target_before: &ResolvedTarget,
) -> Result<InteractionHandoff> {
    let started = Instant::now();
    let target_before_envelope = target_before.to_target_envelope();
    let (current_document, actionable_matches) = {
        let dom = context.refresh_dom()?;
        (
            dom.document.clone(),
            actionable_targets_for_selector(dom, &target_before.selector),
        )
    };

    let (target_after, target_status) = determine_target_after(
        context,
        target_before,
        &current_document,
        actionable_matches,
    )?;
    let legacy_target = target_after.clone();
    context.record_handoff_rebuild_micros(duration_micros(started.elapsed()));

    Ok(InteractionHandoff {
        envelope: DocumentEnvelope {
            document: current_document,
            target: legacy_target,
            snapshot: None,
            nodes: Vec::new(),
            interactive_count: None,
        },
        target_before: target_before_envelope,
        target_after,
        target_status,
    })
}

pub(crate) fn build_actionability_failure(
    tool: &str,
    session: &BrowserSession,
    target: &ResolvedTarget,
    probe: &ActionabilityProbeResult,
    predicates: &[ActionabilityPredicate],
    override_code: Option<&str>,
) -> Result<ToolResult> {
    let failed_predicates = failed_predicates(probe, predicates);
    let (default_code, error) = classify_actionability_failure(probe, predicates);
    build_interaction_failure(
        tool,
        session,
        target,
        override_code.unwrap_or(default_code).to_string(),
        error,
        failed_predicates,
        probe.diagnostics.clone(),
    )
}

pub(crate) fn build_interaction_failure(
    _tool: &str,
    session: &BrowserSession,
    target: &ResolvedTarget,
    code: String,
    error: String,
    failed_predicates: Vec<String>,
    diagnostics: Option<ActionabilityDiagnostics>,
) -> Result<ToolResult> {
    let current_document = session.document_metadata()?;
    let suggested_tool = if code == "target_detached" {
        "snapshot"
    } else {
        "inspect_node"
    };

    Ok(ToolResult::failure_with(
        error.clone(),
        serde_json::json!({
            "code": code,
            "error": error,
            "document": current_document,
            "target_before": target.to_target_envelope(),
            "failed_predicates": failed_predicates,
            "diagnostics": diagnostics,
            "recovery": {
                "suggested_tool": suggested_tool,
            }
        }),
    ))
}

pub(crate) fn decode_action_result(
    value: Option<serde_json::Value>,
    fallback: serde_json::Value,
) -> Result<serde_json::Value> {
    if let Some(serde_json::Value::String(json_str)) = value {
        serde_json::from_str(&json_str).map_err(BrowserError::from)
    } else {
        Ok(value.unwrap_or(fallback))
    }
}

fn requested_actionability_predicates(
    predicates: &[ActionabilityPredicate],
) -> Vec<ActionabilityPredicate> {
    let mut requested = predicates.to_vec();
    if predicates_require_viewport_scroll(predicates)
        && !requested.contains(&ActionabilityPredicate::InViewport)
    {
        requested.push(ActionabilityPredicate::InViewport);
    }
    requested
}

fn predicates_require_viewport_scroll(predicates: &[ActionabilityPredicate]) -> bool {
    predicates.iter().any(|predicate| {
        matches!(
            predicate,
            ActionabilityPredicate::ReceivesEvents | ActionabilityPredicate::UnobscuredCenter
        )
    })
}

fn should_scroll_target_into_view(
    probe: &ActionabilityProbeResult,
    predicates: &[ActionabilityPredicate],
) -> bool {
    predicates_require_viewport_scroll(predicates)
        && probe.present
        && probe.visible != Some(false)
        && probe.in_viewport == Some(false)
}

fn scroll_target_into_view(context: &mut ToolContext, target: &ResolvedTarget) -> Result<()> {
    let config = serde_json::json!({
        "selector": target.selector,
        "target_index": interaction_target_index(target),
    });
    let scroll_js = build_scroll_target_into_view_js(&config);
    context.record_browser_evaluation();
    context
        .session
        .evaluate(&scroll_js, false)
        .map_err(|e| match e {
            BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
                tool: "interaction".to_string(),
                reason,
            },
            other => other,
        })?;
    Ok(())
}

fn interaction_target_index(target: &ResolvedTarget) -> Option<usize> {
    target
        .cursor
        .as_ref()
        .map(|cursor| cursor.index)
        .or(target.index)
}

fn failed_predicates(
    probe: &ActionabilityProbeResult,
    predicates: &[ActionabilityPredicate],
) -> Vec<String> {
    let mut failures = predicates
        .iter()
        .filter_map(|predicate| {
            (probe.predicate(*predicate) != Some(true)).then(|| predicate.key().to_string())
        })
        .collect::<Vec<_>>();

    if !probe.present && !failures.iter().any(|predicate| predicate == "present") {
        failures.insert(0, "present".to_string());
    }

    failures
}

fn classify_actionability_failure(
    probe: &ActionabilityProbeResult,
    predicates: &[ActionabilityPredicate],
) -> (&'static str, String) {
    if !probe.present {
        return ("target_detached", "Target is no longer present".to_string());
    }

    for predicate in predicates {
        match predicate {
            ActionabilityPredicate::Visible if probe.visible == Some(false) => {
                return ("target_not_visible", "Target is not visible".to_string());
            }
            ActionabilityPredicate::Enabled if probe.enabled == Some(false) => {
                return ("target_not_enabled", "Target is not enabled".to_string());
            }
            ActionabilityPredicate::Editable if probe.editable == Some(false) => {
                return ("target_not_editable", "Target is not editable".to_string());
            }
            ActionabilityPredicate::Stable if probe.stable == Some(false) => {
                return (
                    "target_not_stable",
                    "Target is not stable enough to interact with".to_string(),
                );
            }
            ActionabilityPredicate::ReceivesEvents if probe.receives_events == Some(false) => {
                return (
                    "target_obscured",
                    "Target is not receiving events".to_string(),
                );
            }
            ActionabilityPredicate::UnobscuredCenter if probe.unobscured_center == Some(false) => {
                return (
                    "target_obscured",
                    "Target is obscured at its interaction point".to_string(),
                );
            }
            _ => {}
        }
    }

    (
        "target_not_stable",
        "Target did not become ready within the bounded auto-wait window".to_string(),
    )
}

fn actionable_targets_for_selector(dom: &DomTree, selector: &str) -> Vec<Cursor> {
    dom.cursors_for_selector(selector)
}

fn determine_target_after(
    context: &mut ToolContext,
    target_before: &ResolvedTarget,
    current_document: &DocumentMetadata,
    actionable_matches: Vec<Cursor>,
) -> Result<(Option<TargetEnvelope>, TargetStatus)> {
    let current_target = match actionable_matches.as_slice() {
        [cursor] => Some(target_envelope_from_cursor(cursor.clone())),
        _ => None,
    };

    if let Some(before_cursor) = target_before.cursor.as_ref() {
        if before_cursor.node_ref.document_id == current_document.document_id
            && before_cursor.node_ref.revision == current_document.revision
        {
            if let Some(after_target) = current_target.as_ref() {
                if after_target.node_ref == Some(before_cursor.node_ref.clone()) {
                    return Ok((Some(after_target.clone()), TargetStatus::Same));
                }
            }
        }

        if let Some(after_target) = current_target {
            return Ok((Some(after_target), TargetStatus::Rebound));
        }
    } else if let Some(after_target) = current_target {
        return Ok((Some(after_target), TargetStatus::Unknown));
    }

    if actionable_matches.len() > 1 {
        return Ok((None, TargetStatus::Unknown));
    }

    if selector_exists_across_scopes(context, &target_before.selector)? {
        return Ok((None, TargetStatus::Unknown));
    }

    Ok((None, TargetStatus::Detached))
}

fn target_envelope_from_cursor(cursor: Cursor) -> TargetEnvelope {
    TargetEnvelope {
        method: "cursor".to_string(),
        selector: Some(cursor.selector.clone()),
        index: Some(cursor.index),
        node_ref: Some(cursor.node_ref.clone()),
        cursor: Some(cursor),
    }
}

fn selector_exists_across_scopes(context: &mut ToolContext, selector: &str) -> Result<bool> {
    let config = serde_json::json!({ "selector": selector });
    let exists_js = build_target_exists_js(&config);
    context.record_browser_evaluation();
    let result = context
        .session
        .evaluate(&exists_js, false)
        .map_err(|e| match e {
            BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
                tool: "interaction".to_string(),
                reason,
            },
            other => other,
        })?;
    let payload = decode_action_result(result.value, serde_json::json!({})).map_err(|error| {
        BrowserError::ToolExecutionFailed {
            tool: "interaction".to_string(),
            reason: format!("Failed to parse target_exists result: {}", error),
        }
    })?;

    payload
        .get("present")
        .and_then(|value| value.as_bool())
        .ok_or_else(|| BrowserError::ToolExecutionFailed {
            tool: "interaction".to_string(),
            reason: format!(
                "target_exists returned an invalid payload: expected boolean field 'present', got {}",
                value_kind(payload.get("present").unwrap_or(&serde_json::Value::Null))
            ),
        })
}

fn build_scroll_target_into_view_js(config: &serde_json::Value) -> String {
    render_browser_kernel_script(
        SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS,
        "__SCROLL_TARGET_CONFIG__",
        config,
    )
}

fn build_target_exists_js(config: &serde_json::Value) -> String {
    render_browser_kernel_script(
        TARGET_EXISTS_TEMPLATE_JS,
        "__TARGET_EXISTS_CONFIG__",
        config,
    )
}

fn value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{ScriptEvaluation, SessionBackend, TabDescriptor};
    use crate::{dom::DocumentMetadata, dom::DomTree};
    use std::any::Any;
    use std::time::Duration;

    struct InvalidTargetExistsPayloadBackend;

    impl SessionBackend for InvalidTargetExistsPayloadBackend {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn navigate(&self, _url: &str) -> Result<()> {
            unreachable!("navigate is not used in this test")
        }

        fn wait_for_navigation(&self) -> Result<()> {
            unreachable!("wait_for_navigation is not used in this test")
        }

        fn wait_for_document_ready_with_timeout(&self, _timeout: Duration) -> Result<()> {
            unreachable!("wait_for_document_ready_with_timeout is not used in this test")
        }

        fn document_metadata(&self) -> Result<DocumentMetadata> {
            unreachable!("document_metadata is not used in this test")
        }

        fn extract_dom(&self) -> Result<DomTree> {
            unreachable!("extract_dom is not used in this test")
        }

        fn extract_dom_with_prefix(&self, _prefix: &str) -> Result<DomTree> {
            unreachable!("extract_dom_with_prefix is not used in this test")
        }

        fn evaluate(&self, _script: &str, _await_promise: bool) -> Result<ScriptEvaluation> {
            Ok(ScriptEvaluation {
                value: Some(serde_json::json!({
                    "present": "yes",
                })),
                description: None,
                type_name: Some("Object".to_string()),
            })
        }

        fn capture_screenshot(&self, _full_page: bool) -> Result<Vec<u8>> {
            unreachable!("capture_screenshot is not used in this test")
        }

        fn press_key(&self, _key: &str) -> Result<()> {
            unreachable!("press_key is not used in this test")
        }

        fn list_tabs(&self) -> Result<Vec<TabDescriptor>> {
            unreachable!("list_tabs is not used in this test")
        }

        fn active_tab(&self) -> Result<TabDescriptor> {
            unreachable!("active_tab is not used in this test")
        }

        fn open_tab(&self, _url: &str) -> Result<TabDescriptor> {
            unreachable!("open_tab is not used in this test")
        }

        fn activate_tab(&self, _tab_id: &str) -> Result<()> {
            unreachable!("activate_tab is not used in this test")
        }

        fn close_tab(&self, _tab_id: &str, _with_unload: bool) -> Result<()> {
            unreachable!("close_tab is not used in this test")
        }

        fn close(&self) -> Result<()> {
            unreachable!("close is not used in this test")
        }
    }

    #[test]
    fn test_selector_exists_across_scopes_rejects_invalid_payload() {
        let session = BrowserSession::with_test_backend(InvalidTargetExistsPayloadBackend);
        let mut context = ToolContext::new(&session);

        let error = selector_exists_across_scopes(&mut context, "#fake-target")
            .expect_err("invalid target_exists payload should fail");

        match error {
            BrowserError::ToolExecutionFailed { tool, reason } => {
                assert_eq!(tool, "interaction");
                assert!(reason.contains("target_exists returned an invalid payload"));
                assert!(reason.contains("expected boolean field 'present'"));
                assert!(reason.contains("got string"));
            }
            other => panic!("unexpected target_exists error: {other:?}"),
        }
    }
}
