use crate::error::Result;
use crate::tools::TargetEnvelope;
use crate::tools::inspect_node::{
    InspectDetail, InspectNodeOutput, InspectNodeParams, InspectNodeProbePayload,
    build_inspect_node_js, decode_probe_payload,
};
use crate::tools::{
    DocumentEnvelopeOptions, TargetResolution, ToolContext, ToolResult, build_document_envelope,
    resolve_target_with_cursor,
};

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

fn downgrade_target_to_selector(target: &mut TargetEnvelope) {
    target.method = "css".to_string();
    target.cursor = None;
    target.node_ref = None;
    target.index = None;
}

fn reconcile_target_with_probe(
    target: &mut TargetEnvelope,
    payload: &InspectNodeProbePayload,
    resolved_revision: &str,
    current_revision: &str,
) {
    if target.cursor.is_none() && target.index.is_none() && target.node_ref.is_none() {
        return;
    }

    if resolved_revision != current_revision {
        downgrade_target_to_selector(target);
        return;
    }

    match (target.index, payload.actionable_index) {
        (Some(expected), Some(actual)) if expected == actual => {}
        (Some(_), _) => downgrade_target_to_selector(target),
        (None, Some(_)) | (None, None) => {}
    }
}

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
    let (target, resolved_revision) = {
        let dom = context.get_dom()?;
        let resolved_revision = dom.document.revision.clone();
        let target = match resolve_target_with_cursor(
            "inspect_node",
            selector,
            index,
            node_ref,
            cursor,
            Some(dom),
        )? {
            TargetResolution::Resolved(target) => target,
            TargetResolution::Failure(failure) => return Ok(context.finish(failure)),
        };

        (target, resolved_revision)
    };

    let target_index = target
        .cursor
        .as_ref()
        .map(|cursor| cursor.index)
        .or(target.index);
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
        return Ok(context.finish(ToolResult::failure_with(
            error.clone(),
            serde_json::json!({
                "code": payload.code.unwrap_or_else(|| "inspect_failed".to_string()),
                "error": error,
                "target": target.to_target_envelope(),
                "boundaries": boundaries,
            }),
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
        &resolved_revision,
        &current_revision,
    );
    let cursor = target_envelope.cursor.clone();

    let missing_fields = missing_required_probe_fields(&payload);
    if !missing_fields.is_empty() {
        let error = "inspect_node returned an incomplete probe payload".to_string();
        return Ok(context.finish(ToolResult::failure_with(
            error.clone(),
            serde_json::json!({
                "code": "inspect_payload_incomplete",
                "error": error,
                "target": target_envelope,
                "context": payload.context,
                "boundary": payload.boundary,
                "missing_fields": missing_fields,
                "recovery": {
                    "suggested_tool": "snapshot",
                }
            }),
        )));
    }

    let identity = payload
        .identity
        .expect("required inspect payload fields should be validated");
    let accessibility = payload
        .accessibility
        .expect("required inspect payload fields should be validated");
    let form_state = payload
        .form_state
        .expect("required inspect payload fields should be validated");
    let layout = payload
        .layout
        .expect("required inspect payload fields should be validated");
    let inspect_context = payload
        .context
        .expect("required inspect payload fields should be validated");

    Ok(context.finish(ToolResult::success_with(InspectNodeOutput {
        action: "inspect_node".to_string(),
        document: envelope.document,
        target: target_envelope,
        cursor,
        identity,
        accessibility,
        form_state,
        layout,
        context: inspect_context,
        boundary: payload.boundary,
        sections: payload.sections,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::{Cursor, NodeRef};

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
            cursor: Some(cursor),
            node_ref: Some(node_ref),
            selector: Some("#save".to_string()),
            index: Some(index),
        }
    }

    #[test]
    fn test_reconcile_target_with_probe_keeps_verified_actionable_handles() {
        let mut target = target_with_cursor(4);
        let payload = InspectNodeProbePayload {
            success: true,
            code: None,
            error: None,
            actionable_index: Some(4),
            identity: None,
            accessibility: None,
            form_state: None,
            layout: None,
            context: None,
            boundary: None,
            boundaries: None,
            sections: None,
        };

        reconcile_target_with_probe(&mut target, &payload, "rev-1", "rev-1");

        assert_eq!(target.method, "cursor");
        assert_eq!(target.index, Some(4));
        assert!(target.cursor.is_some());
    }

    #[test]
    fn test_reconcile_target_with_probe_downgrades_mismatched_actionable_handles() {
        let mut target = target_with_cursor(4);
        let payload = InspectNodeProbePayload {
            success: true,
            code: None,
            error: None,
            actionable_index: Some(9),
            identity: None,
            accessibility: None,
            form_state: None,
            layout: None,
            context: None,
            boundary: None,
            boundaries: None,
            sections: None,
        };

        reconcile_target_with_probe(&mut target, &payload, "rev-1", "rev-1");

        assert_eq!(target.method, "css");
        assert_eq!(target.selector.as_deref(), Some("#save"));
        assert!(target.cursor.is_none());
        assert!(target.node_ref.is_none());
        assert!(target.index.is_none());
    }

    #[test]
    fn test_reconcile_target_with_probe_downgrades_after_revision_change() {
        let mut target = target_with_cursor(4);
        let payload = InspectNodeProbePayload {
            success: true,
            code: None,
            error: None,
            actionable_index: Some(4),
            identity: None,
            accessibility: None,
            form_state: None,
            layout: None,
            context: None,
            boundary: None,
            boundaries: None,
            sections: None,
        };

        reconcile_target_with_probe(&mut target, &payload, "rev-1", "rev-2");

        assert_eq!(target.method, "css");
        assert!(target.cursor.is_none());
        assert!(target.node_ref.is_none());
        assert!(target.index.is_none());
    }
}
