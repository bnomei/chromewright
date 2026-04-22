use crate::error::Result;
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
    let target = {
        let dom = context.get_dom()?;
        match resolve_target_with_cursor(
            "inspect_node",
            selector,
            index,
            node_ref,
            cursor,
            Some(dom),
        )? {
            TargetResolution::Resolved(target) => target,
            TargetResolution::Failure(failure) => return Ok(context.finish(failure)),
        }
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
    if target_envelope.cursor.is_none() {
        target_envelope.cursor = match payload.actionable_index {
            Some(index) => context.get_dom()?.cursor_for_index(index),
            None => None,
        };
    }
    let cursor = target_envelope.cursor.clone();
    if cursor.is_none() {
        let error = if payload
            .context
            .as_ref()
            .map(|context| context.frame_depth > 0)
            .unwrap_or(false)
        {
            "inspect_node matched an element inside an iframe, but it is not cursor-addressable yet"
                .to_string()
        } else {
            "inspect_node requires a selector that resolves to an actionable cursor".to_string()
        };
        let code = if payload
            .context
            .as_ref()
            .map(|context| context.frame_depth > 0)
            .unwrap_or(false)
        {
            "unsupported_frame_context"
        } else {
            "selector_not_cursor_addressable"
        };

        return Ok(context.finish(ToolResult::failure_with(
            error.clone(),
            serde_json::json!({
                "code": code,
                "error": error,
                "target": target_envelope,
                "context": payload.context,
                "boundary": payload.boundary,
            }),
        )));
    }

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
