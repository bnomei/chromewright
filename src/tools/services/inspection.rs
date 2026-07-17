//! `inspect_node` orchestration: target resolution, in-page probe, and envelope assembly.

use crate::error::Result;
use crate::tools::ResolvedTarget;
use crate::tools::TargetEnvelope;
use crate::tools::core::structured_tool_failure;
use crate::tools::inspect_node::{
    InspectAccessibility, InspectBoundary, InspectContext, InspectDetail, InspectFormState,
    InspectIdentity, InspectLayout, InspectNodeOutput, InspectNodeParams, InspectNodeProbePayload,
    build_inspect_node_js, decode_probe_payload,
};
use crate::tools::limits::{MAX_INSPECT_CLASSES, MAX_INSPECT_COMPACT_CHARS};
use crate::tools::{
    DocumentActionResult, DocumentEnvelopeOptions, StaleCursorPolicy, TargetResolution,
    ToolContext, ToolResult, build_document_envelope, resolve_target_with_cursor,
};
use std::collections::BTreeSet;

const DEFAULT_STYLE_NAMES: &[&str] = &[
    "display",
    "visibility",
    "pointer-events",
    "position",
    "z-index",
    "opacity",
    "cursor",
    "overflow",
];
const MAX_STYLE_NAMES: usize = 12;

fn missing_required_probe_fields(payload: &InspectNodeProbePayload) -> Vec<&'static str> {
    let mut missing = Vec::new();

    if payload.identity.is_none() {
        missing.push("identity");
    }
    if payload.accessibility.is_none() {
        missing.push("accessibility");
    }
    if payload.form_state.is_none() {
        missing.push("form_state");
    }
    if payload.layout.is_none() {
        missing.push("layout");
    }
    if payload.context.is_none() {
        missing.push("context");
    }

    missing
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActionableTargetFingerprint {
    index: usize,
    role: String,
    name: String,
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

fn actionable_target_fingerprint(
    target: &ResolvedTarget,
    dom: &crate::dom::DomTree,
) -> Option<ActionableTargetFingerprint> {
    let index = target
        .cursor
        .as_ref()
        .map(|cursor| cursor.index)
        .or(target.index)?;
    let node = dom.find_node_by_index(index)?;

    Some(ActionableTargetFingerprint {
        index,
        role: node.role.clone(),
        name: node.name.clone(),
        tag: node.tag.clone(),
        id: node.id.clone(),
        classes: node.classes.clone(),
    })
}

fn sorted_classes(classes: &[String]) -> BTreeSet<&str> {
    classes.iter().map(String::as_str).collect()
}

fn probe_matches_actionable_target(
    fingerprint: &ActionableTargetFingerprint,
    payload: &InspectNodeProbePayload,
) -> bool {
    let Some(identity) = payload.identity.as_ref() else {
        return false;
    };
    let Some(accessibility) = payload.accessibility.as_ref() else {
        return false;
    };

    payload.actionable_index == Some(fingerprint.index)
        && accessibility.role == fingerprint.role
        && accessibility.name == fingerprint.name
        && fingerprint
            .tag
            .as_deref()
            .is_none_or(|tag| identity.tag == tag)
        && identity.id == fingerprint.id
        && sorted_classes(&identity.classes) == sorted_classes(&fingerprint.classes)
}

fn downgrade_target_to_selector(target: &mut TargetEnvelope, selector: Option<String>) {
    target.method = "css".to_string();
    target.selector = selector.or_else(|| target.selector.clone());
    target.cursor = None;
    target.node_ref = None;
    target.index = None;
}

fn reconcile_target_with_probe(
    target: &mut TargetEnvelope,
    payload: &InspectNodeProbePayload,
    actionable_fingerprint: Option<&ActionableTargetFingerprint>,
    resolved_revision: &str,
    current_revision: &str,
) {
    let resolved_selector = payload
        .resolved_selector
        .clone()
        .or_else(|| target.selector.clone());
    let had_actionable_handles =
        target.cursor.is_some() || target.index.is_some() || target.node_ref.is_some();

    if !had_actionable_handles {
        target.selector = resolved_selector;
        return;
    }

    let target_is_verified = resolved_revision == current_revision
        && actionable_fingerprint
            .is_some_and(|fingerprint| probe_matches_actionable_target(fingerprint, payload));

    if !target_is_verified {
        downgrade_target_to_selector(target, resolved_selector);
        return;
    }

    target.selector = resolved_selector;
}

fn truncate_string(value: &mut String, limit: usize) -> bool {
    if value.chars().count() <= limit {
        return false;
    }

    *value = value.chars().take(limit).collect();
    true
}

fn truncate_option_string(
    value: &mut Option<String>,
    field: &str,
    truncated_fields: &mut Vec<String>,
) {
    if let Some(value) = value.as_mut()
        && truncate_string(value, MAX_INSPECT_COMPACT_CHARS)
    {
        truncated_fields.push(field.to_string());
    }
}

fn truncate_required_string(value: &mut String, field: &str, truncated_fields: &mut Vec<String>) {
    if truncate_string(value, MAX_INSPECT_COMPACT_CHARS) {
        truncated_fields.push(field.to_string());
    }
}

fn truncate_target_fields(target: &mut TargetEnvelope, truncated_fields: &mut Vec<String>) {
    truncate_option_string(&mut target.selector, "target.selector", truncated_fields);
    if let Some(cursor) = target.cursor.as_mut() {
        if truncate_string(&mut cursor.selector, MAX_INSPECT_COMPACT_CHARS) {
            truncated_fields.push("target.cursor.selector".to_string());
        }
        if truncate_string(&mut cursor.role, MAX_INSPECT_COMPACT_CHARS) {
            truncated_fields.push("target.cursor.role".to_string());
        }
        if truncate_string(&mut cursor.name, MAX_INSPECT_COMPACT_CHARS) {
            truncated_fields.push("target.cursor.name".to_string());
        }
    }
}

fn truncate_identity_fields(identity: &mut InspectIdentity, truncated_fields: &mut Vec<String>) {
    truncate_required_string(&mut identity.tag, "identity.tag", truncated_fields);
    truncate_option_string(&mut identity.id, "identity.id", truncated_fields);

    if identity.classes.len() > MAX_INSPECT_CLASSES {
        identity.classes.truncate(MAX_INSPECT_CLASSES);
        truncated_fields.push("identity.classes".to_string());
    }

    for (index, class_name) in identity.classes.iter_mut().enumerate() {
        if truncate_string(class_name, MAX_INSPECT_COMPACT_CHARS) {
            truncated_fields.push(format!("identity.classes[{index}]"));
        }
    }
}

fn truncate_accessibility_fields(
    accessibility: &mut InspectAccessibility,
    truncated_fields: &mut Vec<String>,
) {
    truncate_required_string(
        &mut accessibility.role,
        "accessibility.role",
        truncated_fields,
    );
    truncate_required_string(
        &mut accessibility.name,
        "accessibility.name",
        truncated_fields,
    );
}

fn truncate_form_state_fields(
    form_state: &mut InspectFormState,
    truncated_fields: &mut Vec<String>,
) {
    truncate_option_string(&mut form_state.value, "form_state.value", truncated_fields);
    truncate_option_string(
        &mut form_state.placeholder,
        "form_state.placeholder",
        truncated_fields,
    );
}

fn truncate_layout_fields(layout: &mut InspectLayout, truncated_fields: &mut Vec<String>) {
    truncate_option_string(
        &mut layout.pointer_events,
        "layout.pointer_events",
        truncated_fields,
    );
    truncate_option_string(&mut layout.cursor, "layout.cursor", truncated_fields);
}

fn truncate_context_fields(context: &mut InspectContext, truncated_fields: &mut Vec<String>) {
    truncate_required_string(
        &mut context.document_url,
        "context.document_url",
        truncated_fields,
    );
}

fn truncate_boundary_fields(
    boundary: &mut Option<InspectBoundary>,
    truncated_fields: &mut Vec<String>,
) {
    if let Some(boundary) = boundary.as_mut() {
        truncate_option_string(&mut boundary.url, "boundary.url", truncated_fields);
    }
}

/// Resolve a node (allowing selector rebound), run the in-page inspect probe, and assemble output.
///
/// On revision drift after the probe, handle-like target fields may be downgraded to CSS-only
/// so agents do not reuse unverified cursors.
pub(crate) fn execute_inspect_node(
    params: InspectNodeParams,
    context: &mut ToolContext,
) -> Result<ToolResult> {
    let InspectNodeParams {
        selector,
        index,
        node_ref,
        cursor,
        detail,
        style_names,
    } = params;
    let (target, resolved_revision, actionable_fingerprint) = {
        let dom = context.get_dom()?;
        let resolved_revision = dom.document.revision.clone();
        let target = match resolve_target_with_cursor(
            "inspect_node",
            selector,
            index,
            node_ref,
            cursor,
            Some(dom),
            StaleCursorPolicy::AllowRebind,
        )? {
            TargetResolution::Resolved(target) => target,
            TargetResolution::Failure(failure) => return Ok(context.finish(failure)),
        };

        let actionable_fingerprint = actionable_target_fingerprint(&target, dom);

        (target, resolved_revision, actionable_fingerprint)
    };

    let target_index = target.browser_command_target_index();
    let styles = if style_names.is_empty() {
        DEFAULT_STYLE_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>()
    } else {
        style_names
            .into_iter()
            .take(MAX_STYLE_NAMES)
            .collect::<Vec<_>>()
    };
    let detail_name = match detail {
        InspectDetail::Compact => "compact",
        InspectDetail::Full => "full",
    };
    let config = serde_json::json!({
        "selector": target.selector,
        "target_index": target_index,
        "detail": detail_name,
        "style_names": styles,
    });
    let inspect_js = build_inspect_node_js(&config);
    context.record_browser_evaluation();
    let evaluation = context
        .session
        .evaluate(&inspect_js, false)
        .map_err(|e| match e {
            crate::error::BrowserError::EvaluationFailed(reason) => {
                crate::error::BrowserError::ToolExecutionFailed {
                    tool: "inspect_node".to_string(),
                    reason,
                }
            }
            other => other,
        })?;
    let payload: InspectNodeProbePayload = decode_probe_payload(evaluation.value)?;

    if !payload.success {
        let error = payload
            .error
            .clone()
            .unwrap_or_else(|| "Node inspection failed".to_string());
        let boundaries = payload.boundaries.unwrap_or_default();
        return Ok(context.finish(structured_tool_failure(
            payload.code.unwrap_or_else(|| "inspect_failed".to_string()),
            error,
            None,
            Some(target.to_target_envelope()),
            None,
            Some(serde_json::json!({
                "boundaries": boundaries,
            })),
        )));
    }

    let mut envelope =
        build_document_envelope(context, Some(&target), DocumentEnvelopeOptions::minimal())?;
    let mut target_envelope = envelope
        .target
        .take()
        .expect("inspect_node should keep target");
    let current_revision = envelope.document.revision.clone();
    reconcile_target_with_probe(
        &mut target_envelope,
        &payload,
        actionable_fingerprint.as_ref(),
        &resolved_revision,
        &current_revision,
    );

    let missing_fields = missing_required_probe_fields(&payload);
    if !missing_fields.is_empty() {
        let error = "inspect_node returned an incomplete probe payload".to_string();
        return Ok(context.finish(structured_tool_failure(
            "inspect_payload_incomplete",
            error,
            None,
            Some(target_envelope),
            Some(serde_json::json!({
                "suggested_tool": "snapshot",
            })),
            Some(serde_json::json!({
                "context": payload.context,
                "boundary": payload.boundary,
                "missing_fields": missing_fields,
            })),
        )));
    }

    let mut identity = payload
        .identity
        .expect("required inspect payload fields should be validated");
    let mut accessibility = payload
        .accessibility
        .expect("required inspect payload fields should be validated");
    let mut form_state = payload
        .form_state
        .expect("required inspect payload fields should be validated");
    let mut layout = payload
        .layout
        .expect("required inspect payload fields should be validated");
    let mut inspect_context = payload
        .context
        .expect("required inspect payload fields should be validated");
    let mut boundary = payload.boundary;
    let mut truncated_fields = Vec::new();
    truncate_target_fields(&mut target_envelope, &mut truncated_fields);
    truncate_identity_fields(&mut identity, &mut truncated_fields);
    truncate_accessibility_fields(&mut accessibility, &mut truncated_fields);
    truncate_form_state_fields(&mut form_state, &mut truncated_fields);
    truncate_layout_fields(&mut layout, &mut truncated_fields);
    truncate_context_fields(&mut inspect_context, &mut truncated_fields);
    truncate_boundary_fields(&mut boundary, &mut truncated_fields);

    Ok(context.finish(ToolResult::success_with(InspectNodeOutput {
        result: DocumentActionResult::new("inspect_node", envelope.document),
        target: target_envelope,
        identity,
        accessibility,
        form_state,
        layout,
        context: inspect_context,
        boundary,
        sections: payload.sections,
        truncated_fields,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::{Cursor, NodeRef};
    use crate::tools::inspect_node::{InspectAccessibility, InspectIdentity};

    fn actionable_fingerprint(index: usize) -> ActionableTargetFingerprint {
        ActionableTargetFingerprint {
            index,
            role: "button".to_string(),
            name: "Save".to_string(),
            tag: Some("button".to_string()),
            id: Some("save".to_string()),
            classes: vec!["primary".to_string()],
        }
    }

    fn probe_payload(actionable_index: Option<usize>) -> InspectNodeProbePayload {
        InspectNodeProbePayload {
            success: true,
            code: None,
            error: None,
            actionable_index,
            resolved_selector: Some("#save".to_string()),
            identity: Some(InspectIdentity {
                tag: "button".to_string(),
                id: Some("save".to_string()),
                classes: vec!["primary".to_string()],
            }),
            accessibility: Some(InspectAccessibility {
                role: "button".to_string(),
                name: "Save".to_string(),
                active: None,
                checked: None,
                disabled: None,
                expanded: None,
                pressed: None,
                selected: None,
            }),
            form_state: None,
            layout: None,
            context: None,
            boundary: None,
            boundaries: None,
            sections: None,
        }
    }

    fn target_with_cursor(index: usize) -> TargetEnvelope {
        let node_ref = NodeRef {
            document_id: "doc-1".to_string(),
            revision: "rev-1".to_string(),
            index,
        };
        let cursor = Cursor {
            node_ref: node_ref.clone(),
            selector: "#save".to_string(),
            index,
            role: "button".to_string(),
            name: "Save".to_string(),
        };

        TargetEnvelope {
            method: "cursor".to_string(),
            resolution_status: "exact".to_string(),
            recovered_from: None,
            cursor: Some(cursor),
            node_ref: Some(node_ref),
            selector: Some("#save".to_string()),
            index: Some(index),
        }
    }

    #[test]
    fn test_reconcile_target_with_probe_keeps_verified_actionable_handles() {
        let mut target = target_with_cursor(4);
        let payload = probe_payload(Some(4));

        reconcile_target_with_probe(
            &mut target,
            &payload,
            Some(&actionable_fingerprint(4)),
            "rev-1",
            "rev-1",
        );

        assert_eq!(target.method, "cursor");
        assert_eq!(target.index, Some(4));
        assert!(target.cursor.is_some());
    }

    #[test]
    fn test_reconcile_target_with_probe_downgrades_mismatched_actionable_handles() {
        let mut target = target_with_cursor(4);
        let payload = probe_payload(Some(9));

        reconcile_target_with_probe(
            &mut target,
            &payload,
            Some(&actionable_fingerprint(4)),
            "rev-1",
            "rev-1",
        );

        assert_eq!(target.method, "css");
        assert_eq!(target.selector.as_deref(), Some("#save"));
        assert!(target.cursor.is_none());
        assert!(target.node_ref.is_none());
        assert!(target.index.is_none());
    }

    #[test]
    fn test_reconcile_target_with_probe_downgrades_after_revision_change() {
        let mut target = target_with_cursor(4);
        let payload = probe_payload(Some(4));

        reconcile_target_with_probe(
            &mut target,
            &payload,
            Some(&actionable_fingerprint(4)),
            "rev-1",
            "rev-2",
        );

        assert_eq!(target.method, "css");
        assert!(target.cursor.is_none());
        assert!(target.node_ref.is_none());
        assert!(target.index.is_none());
    }

    #[test]
    fn test_reconcile_target_with_probe_downgrades_on_identity_mismatch() {
        let mut target = target_with_cursor(4);
        let mut payload = probe_payload(Some(4));
        payload.identity = Some(InspectIdentity {
            tag: "button".to_string(),
            id: Some("publish".to_string()),
            classes: vec!["primary".to_string()],
        });

        reconcile_target_with_probe(
            &mut target,
            &payload,
            Some(&actionable_fingerprint(4)),
            "rev-1",
            "rev-1",
        );

        assert_eq!(target.method, "css");
        assert_eq!(target.selector.as_deref(), Some("#save"));
        assert!(target.cursor.is_none());
        assert!(target.node_ref.is_none());
        assert!(target.index.is_none());
    }

    #[test]
    fn test_reconcile_target_with_probe_keeps_selector_only_target_for_non_actionable_probe() {
        let mut target = TargetEnvelope {
            method: "css".to_string(),
            resolution_status: "exact".to_string(),
            recovered_from: None,
            cursor: None,
            node_ref: None,
            selector: Some("h1".to_string()),
            index: None,
        };
        let mut payload = probe_payload(None);
        payload.resolved_selector = Some("h1".to_string());
        payload.identity = Some(InspectIdentity {
            tag: "h1".to_string(),
            id: Some("story-title".to_string()),
            classes: Vec::new(),
        });
        payload.accessibility = Some(InspectAccessibility {
            role: "heading".to_string(),
            name: "Story".to_string(),
            active: None,
            checked: None,
            disabled: None,
            expanded: None,
            pressed: None,
            selected: None,
        });

        reconcile_target_with_probe(&mut target, &payload, None, "rev-1", "rev-1");

        assert_eq!(target.method, "css");
        assert_eq!(target.selector.as_deref(), Some("h1"));
        assert!(target.cursor.is_none());
        assert!(target.node_ref.is_none());
        assert!(target.index.is_none());
    }

    #[test]
    fn test_reconcile_target_with_probe_preserves_rebound_metadata_after_downgrade() {
        let mut target = target_with_cursor(4);
        target.resolution_status = "selector_rebound".to_string();
        target.recovered_from = Some("cursor".to_string());
        let payload = probe_payload(Some(9));

        reconcile_target_with_probe(
            &mut target,
            &payload,
            Some(&actionable_fingerprint(4)),
            "rev-1",
            "rev-1",
        );

        assert_eq!(target.method, "css");
        assert_eq!(target.resolution_status, "selector_rebound");
        assert_eq!(target.recovered_from.as_deref(), Some("cursor"));
        assert!(target.cursor.is_none());
        assert!(target.node_ref.is_none());
        assert!(target.index.is_none());
    }
}
