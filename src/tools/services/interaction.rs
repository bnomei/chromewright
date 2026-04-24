use crate::browser::BrowserSession;
pub use crate::contract::TargetStatus;
use crate::dom::{Cursor, DocumentMetadata, DomTree, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::core::structured_tool_failure;
use crate::tools::{
    ResolvedTarget, TargetEnvelope, TargetResolution, ToolContext, ToolResult,
    actionability::{
        ActionabilityDiagnostics, ActionabilityPredicate, ActionabilityProbeResult,
        ActionabilityRequest, probe_actionability,
    },
    browser_kernel::render_browser_kernel_script,
    duration_micros, resolve_target_with_cursor,
};
use serde::Deserialize;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS: &str = include_str!("../scroll_target_into_view.js");
static SCROLL_TARGET_INTO_VIEW_SHELL: OnceLock<
    crate::tools::browser_kernel::BrowserKernelTemplateShell,
> = OnceLock::new();
const SELECTOR_IDENTITY_TEMPLATE_JS: &str = r#"
(() => {
  const config = __SELECTOR_IDENTITY_CONFIG__;

  __BROWSER_KERNEL__

  function countSelectorMatchesAcrossScopes(selector) {
    const visitedDocs = new Set();
    let count = 0;

    function searchRoot(root) {
      if (!root || typeof root.querySelectorAll !== 'function') {
        return;
      }

      let matches = [];
      try {
        matches = root.querySelectorAll(selector);
      } catch (error) {
        const normalized = normalizeSimpleIdSelector(selector);
        if (!normalized) {
          return;
        }

        try {
          matches = root.querySelectorAll(normalized);
        } catch (fallbackError) {
          return;
        }
      }

      count += matches.length;
      if (count > 1) {
        return;
      }

      const elements = root.querySelectorAll ? root.querySelectorAll('*') : [];
      for (const element of elements) {
        if (element.shadowRoot) {
          searchRoot(element.shadowRoot);
          if (count > 1) {
            return;
          }
        }

        if (element.tagName === 'IFRAME') {
          try {
            const frameDoc = element.contentDocument;
            if (!frameDoc || visitedDocs.has(frameDoc)) {
              continue;
            }

            visitedDocs.add(frameDoc);
            searchRoot(frameDoc);
            if (count > 1) {
              return;
            }
          } catch (error) {
            // Cross-origin frame; selector identity stops at the iframe boundary.
          }
        }
      }
    }

    visitedDocs.add(document);
    searchRoot(document);
    return count;
  }

  const matchCount = config.selector ? countSelectorMatchesAcrossScopes(config.selector) : 0;

  return JSON.stringify({
    present: matchCount > 0,
    unique: matchCount === 1
  });
})()
"#;
static SELECTOR_IDENTITY_SHELL: OnceLock<crate::tools::browser_kernel::BrowserKernelTemplateShell> =
    OnceLock::new();
pub(crate) const DEFAULT_ACTIONABILITY_TIMEOUT_MS: u64 = 5_000;
const ACTIONABILITY_POLL_INTERVAL_MS: u64 = 50;

pub(crate) enum ActionabilityWaitState {
    Ready,
    TimedOut(ActionabilityProbeResult),
}

pub(crate) struct InteractionHandoff {
    pub document: DocumentMetadata,
    pub target_before: TargetEnvelope,
    pub target_after: Option<TargetEnvelope>,
    pub target_status: TargetStatus,
}

#[derive(Debug, Clone, Deserialize)]
struct SelectorIdentityProbeResult {
    present: bool,
    unique: bool,
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
    context.record_handoff_rebuild_micros(duration_micros(started.elapsed()));

    Ok(InteractionHandoff {
        document: current_document,
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

    Ok(structured_tool_failure(
        code,
        error,
        Some(current_document),
        Some(target.to_target_envelope()),
        Some(serde_json::json!({
            "suggested_tool": suggested_tool,
        })),
        Some(serde_json::json!({
            "failed_predicates": failed_predicates,
            "diagnostics": diagnostics,
        })),
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
        .filter(|predicate| probe.predicate(**predicate) != Some(true))
        .map(|predicate| predicate.key().to_string())
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
    if actionable_matches.len() > 1 {
        return Ok((None, TargetStatus::Unknown));
    }

    if let Some(cursor) = actionable_matches.into_iter().next() {
        let after_target = target_envelope_from_cursor(cursor);
        let status = classify_target_status(target_before, current_document, &after_target);
        return Ok((Some(after_target), status));
    }

    let identity = probe_selector_identity(context, &target_before.selector)?;
    if !identity.present {
        return Ok((None, TargetStatus::Detached));
    }

    if !identity.unique {
        return Ok((None, TargetStatus::Unknown));
    }

    let after_target = selector_target_envelope(&target_before.selector);
    let status = classify_target_status(target_before, current_document, &after_target);
    Ok((Some(after_target), status))
}

fn target_envelope_from_cursor(cursor: Cursor) -> TargetEnvelope {
    TargetEnvelope {
        method: "cursor".to_string(),
        resolution_status: "exact".to_string(),
        recovered_from: None,
        selector: Some(cursor.selector.clone()),
        index: Some(cursor.index),
        node_ref: Some(cursor.node_ref.clone()),
        cursor: Some(cursor),
    }
}

fn selector_target_envelope(selector: &str) -> TargetEnvelope {
    TargetEnvelope {
        method: "css".to_string(),
        resolution_status: "exact".to_string(),
        recovered_from: None,
        cursor: None,
        node_ref: None,
        selector: Some(selector.to_string()),
        index: None,
    }
}

fn classify_target_status(
    target_before: &ResolvedTarget,
    current_document: &DocumentMetadata,
    after_target: &TargetEnvelope,
) -> TargetStatus {
    let before_node_ref = target_before
        .cursor
        .as_ref()
        .map(|cursor| &cursor.node_ref)
        .or(target_before.node_ref.as_ref());

    let Some(before_node_ref) = before_node_ref else {
        return TargetStatus::Unknown;
    };

    if before_node_ref.document_id != current_document.document_id {
        return TargetStatus::Unknown;
    }

    if before_node_ref.revision == current_document.revision {
        return match after_target.node_ref.as_ref() {
            Some(after_node_ref) if after_node_ref == before_node_ref => TargetStatus::Same,
            Some(_) => TargetStatus::Unknown,
            None => TargetStatus::Same,
        };
    }

    TargetStatus::Rebound
}

fn probe_selector_identity(
    context: &mut ToolContext,
    selector: &str,
) -> Result<SelectorIdentityProbeResult> {
    let config = serde_json::json!({ "selector": selector });
    let probe_js = build_selector_identity_js(&config);
    context.record_browser_evaluation();
    let result = context
        .session
        .evaluate(&probe_js, false)
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
            reason: format!("Failed to parse selector identity result: {}", error),
        }
    })?;

    let present = payload
        .get("present")
        .and_then(|value| value.as_bool())
        .ok_or_else(|| BrowserError::ToolExecutionFailed {
            tool: "interaction".to_string(),
            reason: format!(
                "selector identity probe returned an invalid payload: expected boolean field 'present', got {}",
                value_kind(payload.get("present").unwrap_or(&serde_json::Value::Null))
            ),
        })?;
    let unique = payload
        .get("unique")
        .and_then(|value| value.as_bool())
        .ok_or_else(|| BrowserError::ToolExecutionFailed {
            tool: "interaction".to_string(),
            reason: format!(
                "selector identity probe returned an invalid payload: expected boolean field 'unique', got {}",
                value_kind(payload.get("unique").unwrap_or(&serde_json::Value::Null))
            ),
        })?;

    Ok(SelectorIdentityProbeResult { present, unique })
}

fn build_scroll_target_into_view_js(config: &serde_json::Value) -> String {
    render_browser_kernel_script(
        &SCROLL_TARGET_INTO_VIEW_SHELL,
        SCROLL_TARGET_INTO_VIEW_TEMPLATE_JS,
        "__SCROLL_TARGET_CONFIG__",
        config,
    )
}

fn build_selector_identity_js(config: &serde_json::Value) -> String {
    render_browser_kernel_script(
        &SELECTOR_IDENTITY_SHELL,
        SELECTOR_IDENTITY_TEMPLATE_JS,
        "__SELECTOR_IDENTITY_CONFIG__",
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
    use crate::tools::core::{TargetRecoveredFrom, encode_selector_rebound_method};
    use crate::{dom::DocumentMetadata, dom::DomTree};
    use serde_json::Value;
    use std::time::Duration;

    struct StaticInteractionBackend {
        value: Value,
    }

    impl SessionBackend for StaticInteractionBackend {
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
            Ok(DocumentMetadata {
                document_id: "doc-1".to_string(),
                revision: "rev-2".to_string(),
                ..DocumentMetadata::default()
            })
        }

        fn extract_dom(&self) -> Result<DomTree> {
            unreachable!("extract_dom is not used in this test")
        }

        fn extract_dom_with_prefix(&self, _prefix: &str) -> Result<DomTree> {
            unreachable!("extract_dom_with_prefix is not used in this test")
        }

        fn evaluate(&self, _script: &str, _await_promise: bool) -> Result<ScriptEvaluation> {
            Ok(ScriptEvaluation {
                value: Some(self.value.clone()),
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
            Ok(vec![TabDescriptor {
                id: "tab-1".to_string(),
                title: "Test Tab".to_string(),
                url: "about:blank".to_string(),
            }])
        }

        fn active_tab(&self) -> Result<TabDescriptor> {
            Ok(TabDescriptor {
                id: "tab-1".to_string(),
                title: "Test Tab".to_string(),
                url: "about:blank".to_string(),
            })
        }

        fn open_tab(&self, _url: &str) -> Result<TabDescriptor> {
            Ok(TabDescriptor {
                id: "tab-1".to_string(),
                title: "Test Tab".to_string(),
                url: "about:blank".to_string(),
            })
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
    fn test_probe_selector_identity_rejects_invalid_present_payload() {
        let session = BrowserSession::with_test_backend(StaticInteractionBackend {
            value: serde_json::json!({
                "present": "yes",
                "unique": true,
            }),
        });
        let mut context = ToolContext::new(&session);

        let error = probe_selector_identity(&mut context, "#fake-target")
            .expect_err("invalid target_exists payload should fail");

        match error {
            BrowserError::ToolExecutionFailed { tool, reason } => {
                assert_eq!(tool, "interaction");
                assert!(reason.contains("selector identity probe returned an invalid payload"));
                assert!(reason.contains("expected boolean field 'present'"));
                assert!(reason.contains("got string"));
            }
            other => panic!("unexpected target_exists error: {other:?}"),
        }
    }

    #[test]
    fn test_probe_selector_identity_rejects_invalid_unique_payload() {
        let session = BrowserSession::with_test_backend(StaticInteractionBackend {
            value: serde_json::json!({
                "present": true,
                "unique": "yes",
            }),
        });
        let mut context = ToolContext::new(&session);

        let error = probe_selector_identity(&mut context, "#fake-target")
            .expect_err("invalid unique payload should fail");

        match error {
            BrowserError::ToolExecutionFailed { tool, reason } => {
                assert_eq!(tool, "interaction");
                assert!(reason.contains("selector identity probe returned an invalid payload"));
                assert!(reason.contains("expected boolean field 'unique'"));
                assert!(reason.contains("got string"));
            }
            other => panic!("unexpected target_exists error: {other:?}"),
        }
    }

    #[test]
    fn test_determine_target_after_reuses_unique_selector_for_non_actionable_rebound() {
        let session = BrowserSession::with_test_backend(StaticInteractionBackend {
            value: serde_json::json!({
                "present": true,
                "unique": true,
            }),
        });
        let mut context = ToolContext::new(&session);
        let target_before = resolved_target(
            "#save",
            Some(NodeRef {
                document_id: "doc-1".to_string(),
                revision: "rev-1".to_string(),
                index: 3,
            }),
        );
        let current_document = DocumentMetadata {
            document_id: "doc-1".to_string(),
            revision: "rev-2".to_string(),
            ..DocumentMetadata::default()
        };

        let (target_after, status) =
            determine_target_after(&mut context, &target_before, &current_document, Vec::new())
                .expect("selector identity probe should succeed");

        assert_eq!(status, TargetStatus::Rebound);
        let target_after = target_after.expect("unique selector should yield target_after");
        assert_eq!(target_after.method, "css");
        assert_eq!(target_after.selector.as_deref(), Some("#save"));
        assert_eq!(target_after.resolution_status, "exact");
        assert_eq!(target_after.recovered_from, None);
        assert!(target_after.cursor.is_none());
        assert!(target_after.node_ref.is_none());
        assert!(target_after.index.is_none());
    }

    #[test]
    fn test_determine_target_after_marks_ambiguous_non_actionable_selector_unknown() {
        let session = BrowserSession::with_test_backend(StaticInteractionBackend {
            value: serde_json::json!({
                "present": true,
                "unique": false,
            }),
        });
        let mut context = ToolContext::new(&session);
        let target_before = resolved_target(
            "#save",
            Some(NodeRef {
                document_id: "doc-1".to_string(),
                revision: "rev-1".to_string(),
                index: 3,
            }),
        );
        let current_document = DocumentMetadata {
            document_id: "doc-1".to_string(),
            revision: "rev-2".to_string(),
            ..DocumentMetadata::default()
        };

        let (target_after, status) =
            determine_target_after(&mut context, &target_before, &current_document, Vec::new())
                .expect("selector identity probe should succeed");

        assert_eq!(status, TargetStatus::Unknown);
        assert!(target_after.is_none());
    }

    #[test]
    fn test_determine_target_after_marks_same_revision_cursor_mismatch_unknown() {
        let session =
            BrowserSession::with_test_backend(StaticInteractionBackend { value: Value::Null });
        let mut context = ToolContext::new(&session);
        let target_before = resolved_target(
            "#save",
            Some(NodeRef {
                document_id: "doc-1".to_string(),
                revision: "rev-1".to_string(),
                index: 1,
            }),
        );
        let current_document = DocumentMetadata {
            document_id: "doc-1".to_string(),
            revision: "rev-1".to_string(),
            ..DocumentMetadata::default()
        };
        let actionable_matches = vec![Cursor {
            node_ref: NodeRef {
                document_id: "doc-1".to_string(),
                revision: "rev-1".to_string(),
                index: 4,
            },
            selector: "#save".to_string(),
            index: 4,
            role: "button".to_string(),
            name: "Save".to_string(),
        }];

        let (target_after, status) = determine_target_after(
            &mut context,
            &target_before,
            &current_document,
            actionable_matches,
        )
        .expect("actionable match should classify");

        assert_eq!(status, TargetStatus::Unknown);
        assert_eq!(
            target_after
                .and_then(|target| target.node_ref)
                .map(|node| node.index),
            Some(4)
        );
    }

    #[test]
    fn test_build_interaction_failure_keeps_rebound_target_before_metadata() {
        let session =
            BrowserSession::with_test_backend(StaticInteractionBackend { value: Value::Null });
        let target = resolved_target_with_method(
            encode_selector_rebound_method("cursor", TargetRecoveredFrom::Cursor),
            "#save",
            Some(NodeRef {
                document_id: "doc-1".to_string(),
                revision: "rev-1".to_string(),
                index: 3,
            }),
        );

        let failure = build_interaction_failure(
            "click",
            &session,
            &target,
            "target_not_visible".to_string(),
            "Target is not visible".to_string(),
            vec!["visible".to_string()],
            None,
        )
        .expect("interaction failure should build");

        assert!(!failure.success);
        let data = failure.data.expect("failure data should be present");
        assert_eq!(
            data["target"]["resolution_status"].as_str(),
            Some("selector_rebound")
        );
        assert_eq!(data["target"]["recovered_from"].as_str(), Some("cursor"));
        assert_eq!(data["target"]["selector"].as_str(), Some("#save"));
        assert_eq!(
            data["details"]["failed_predicates"][0].as_str(),
            Some("visible")
        );
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("inspect_node")
        );
    }

    fn resolved_target(selector: &str, node_ref: Option<NodeRef>) -> ResolvedTarget {
        resolved_target_with_method("css".to_string(), selector, node_ref)
    }

    fn resolved_target_with_method(
        method: String,
        selector: &str,
        node_ref: Option<NodeRef>,
    ) -> ResolvedTarget {
        let cursor = node_ref.clone().map(|node_ref| Cursor {
            index: node_ref.index,
            node_ref,
            selector: selector.to_string(),
            role: "button".to_string(),
            name: "Save".to_string(),
        });

        ResolvedTarget {
            method,
            selector: selector.to_string(),
            index: None,
            node_ref,
            cursor,
        }
    }
}
