use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentEnvelopeOptions, ToolContext, ToolResult,
    actionability::{
        ActionabilityPredicate, ActionabilityProbeResult, ActionabilityRequest, probe_actionability,
    },
    build_document_envelope,
    services::interaction::{
        ActionabilityWaitState, build_interaction_handoff, resolve_interaction_target,
        wait_for_actionability,
    },
    wait::{WaitCondition, WaitOutput, WaitParams},
};
use std::time::{Duration, Instant};

pub(crate) fn execute_wait(params: WaitParams, context: &mut ToolContext) -> Result<ToolResult> {
    let start = Instant::now();
    let timeout = Duration::from_millis(params.timeout_ms);

    match params.condition {
        WaitCondition::NavigationSettled => {
            context
                .session
                .wait_for_document_ready_with_timeout(timeout)?;
            context.invalidate_dom();
            let envelope =
                build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;

            Ok(context.finish(ToolResult::success_with(WaitOutput {
                envelope,
                action: "wait".to_string(),
                condition: "navigation_settled".to_string(),
                elapsed_ms: start.elapsed().as_millis() as u64,
                target_before: None,
                target_after: None,
                target_status: None,
                since_revision: None,
            })))
        }
        WaitCondition::RevisionChanged => {
            let baseline = match params.since_revision {
                Some(revision) => revision,
                None => {
                    context.record_browser_evaluation();
                    context.session.document_metadata()?.revision
                }
            };

            loop {
                context.record_poll_iteration();
                context.record_browser_evaluation();
                let current_revision = context.session.document_metadata()?.revision;
                if current_revision != baseline {
                    context.invalidate_dom();
                    let envelope =
                        build_document_envelope(context, None, DocumentEnvelopeOptions::minimal())?;
                    return Ok(context.finish(ToolResult::success_with(WaitOutput {
                        envelope,
                        action: "wait".to_string(),
                        condition: "revision_changed".to_string(),
                        elapsed_ms: start.elapsed().as_millis() as u64,
                        target_before: None,
                        target_after: None,
                        target_status: None,
                        since_revision: Some(baseline),
                    })));
                }

                if start.elapsed() >= timeout {
                    return Err(BrowserError::Timeout(format!(
                        "Document revision did not change from '{}' within {} ms",
                        baseline, params.timeout_ms
                    )));
                }

                std::thread::sleep(Duration::from_millis(50));
            }
        }
        condition => {
            let target = match resolve_interaction_target(
                "wait",
                params.selector.clone(),
                params.index,
                params.node_ref.clone(),
                params.cursor.clone(),
                context,
            )? {
                crate::tools::TargetResolution::Resolved(target) => target,
                crate::tools::TargetResolution::Failure(failure) => {
                    return Ok(context.finish(failure));
                }
            };

            validate_wait_condition(&condition, params.text.as_deref(), params.value.as_deref())?;
            let predicates = wait_condition_predicates(&condition);

            if wait_condition_uses_interaction_scroll(&condition) {
                match wait_for_actionability(context, &target, predicates, params.timeout_ms)? {
                    ActionabilityWaitState::Ready => {
                        let handoff = build_interaction_handoff(context, &target)?;
                        return Ok(context.finish(ToolResult::success_with(WaitOutput {
                            envelope: handoff.envelope,
                            action: "wait".to_string(),
                            condition: condition_name(&condition).to_string(),
                            elapsed_ms: start.elapsed().as_millis() as u64,
                            target_before: Some(handoff.target_before),
                            target_after: handoff.target_after,
                            target_status: Some(handoff.target_status),
                            since_revision: None,
                        })));
                    }
                    ActionabilityWaitState::TimedOut(_) => {
                        return Err(BrowserError::Timeout(format!(
                            "Condition '{}' did not match for '{}' within {} ms",
                            condition_name(&condition),
                            target.selector,
                            params.timeout_ms
                        )));
                    }
                }
            }

            let target_index = target
                .cursor
                .as_ref()
                .map(|cursor| cursor.index)
                .or(target.index);

            loop {
                context.record_poll_iteration();
                context.record_browser_evaluation();
                let probe = evaluate_wait_probe(
                    &condition,
                    context.session,
                    &target.selector,
                    target_index,
                    params.text.as_deref(),
                    params.value.as_deref(),
                )?;

                if wait_condition_matches(&condition, predicates, &probe) {
                    let handoff = build_interaction_handoff(context, &target)?;
                    return Ok(context.finish(ToolResult::success_with(WaitOutput {
                        envelope: handoff.envelope,
                        action: "wait".to_string(),
                        condition: condition_name(&condition).to_string(),
                        elapsed_ms: start.elapsed().as_millis() as u64,
                        target_before: Some(handoff.target_before),
                        target_after: handoff.target_after,
                        target_status: Some(handoff.target_status),
                        since_revision: None,
                    })));
                }

                if start.elapsed() >= timeout {
                    return Err(BrowserError::Timeout(format!(
                        "Condition '{}' did not match for '{}' within {} ms",
                        condition_name(&condition),
                        target.selector,
                        params.timeout_ms
                    )));
                }

                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

pub(crate) fn wait_condition_predicates(
    condition: &WaitCondition,
) -> &'static [ActionabilityPredicate] {
    match condition {
        WaitCondition::NavigationSettled | WaitCondition::RevisionChanged => &[],
        WaitCondition::Present => &[ActionabilityPredicate::Present],
        WaitCondition::Visible => &[ActionabilityPredicate::Visible],
        WaitCondition::Enabled => &[ActionabilityPredicate::Enabled],
        WaitCondition::Editable => &[ActionabilityPredicate::Editable],
        WaitCondition::Actionable => &[
            ActionabilityPredicate::Present,
            ActionabilityPredicate::Visible,
            ActionabilityPredicate::Enabled,
            ActionabilityPredicate::Stable,
            ActionabilityPredicate::ReceivesEvents,
            ActionabilityPredicate::UnobscuredCenter,
        ],
        WaitCondition::Stable => &[ActionabilityPredicate::Stable],
        WaitCondition::ReceivesEvents => &[ActionabilityPredicate::ReceivesEvents],
        WaitCondition::TextContains => &[ActionabilityPredicate::TextContains],
        WaitCondition::ValueEquals => &[ActionabilityPredicate::ValueEquals],
    }
}

pub(crate) fn validate_wait_condition(
    condition: &WaitCondition,
    text: Option<&str>,
    value: Option<&str>,
) -> Result<()> {
    match condition {
        WaitCondition::TextContains if text.is_none() => Err(BrowserError::InvalidArgument(
            "wait.text is required when condition is 'text_contains'".to_string(),
        )),
        WaitCondition::ValueEquals if value.is_none() => Err(BrowserError::InvalidArgument(
            "wait.value is required when condition is 'value_equals'".to_string(),
        )),
        _ => Ok(()),
    }
}

pub(crate) fn condition_name(condition: &WaitCondition) -> &'static str {
    match condition {
        WaitCondition::NavigationSettled => "navigation_settled",
        WaitCondition::Present => "present",
        WaitCondition::Visible => "visible",
        WaitCondition::Enabled => "enabled",
        WaitCondition::Editable => "editable",
        WaitCondition::Actionable => "actionable",
        WaitCondition::Stable => "stable",
        WaitCondition::ReceivesEvents => "receives_events",
        WaitCondition::TextContains => "text_contains",
        WaitCondition::ValueEquals => "value_equals",
        WaitCondition::RevisionChanged => "revision_changed",
    }
}

pub(crate) fn wait_condition_uses_interaction_scroll(condition: &WaitCondition) -> bool {
    matches!(
        condition,
        WaitCondition::Actionable | WaitCondition::ReceivesEvents
    )
}

pub(crate) fn evaluate_wait_probe(
    condition: &WaitCondition,
    session: &crate::browser::BrowserSession,
    selector: &str,
    target_index: Option<usize>,
    expected_text: Option<&str>,
    expected_value: Option<&str>,
) -> Result<ActionabilityProbeResult> {
    probe_actionability(
        session,
        &ActionabilityRequest {
            selector,
            target_index,
            predicates: wait_condition_predicates(condition),
            expected_text,
            expected_value,
        },
    )
}

pub(crate) fn wait_condition_matches(
    _condition: &WaitCondition,
    predicates: &[ActionabilityPredicate],
    probe: &ActionabilityProbeResult,
) -> bool {
    predicates
        .iter()
        .all(|predicate| probe.predicate(*predicate) == Some(true))
}
