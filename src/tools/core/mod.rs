use crate::browser::backend::{ATTACH_PAGE_TARGET_LOST_CODE, ATTACH_SESSION_PAGE_TARGET_LOSS_KIND};
use crate::browser::{BrowserSession, SnapshotCacheEntry, SnapshotCacheScope};
use crate::contract::ViewportMetrics;
pub use crate::contract::{
    DocumentActionResult, DocumentEnvelope, DocumentResult, PublicTarget, SnapshotMode,
    SnapshotScope, TargetEnvelope, TargetedActionResult, ToolResult,
};
use crate::dom::{
    AriaChild, AriaNode, Cursor, DocumentMetadata, DomTree, NodeRef, SnapshotNode,
    yaml_escape_key_if_needed, yaml_escape_value_if_needed,
};
#[cfg(test)]
use crate::error::BackendUnsupportedDetails;
use crate::error::{BrowserError, PageTargetLostDetails, Result};
use crate::tools::snapshot::{RenderMode, render_aria_tree};
use crate::tools::{
    click, close, close_tab, evaluate, extract, go_back, go_forward, hover, input, inspect_node,
    markdown, navigate, new_tab, press_key, read_links, screenshot, scroll, select, set_viewport,
    snapshot, switch_tab, tab_list, wait,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub(crate) const OPERATION_METRICS_METADATA_KEY: &str = "operation_metrics";

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct OperationMetrics {
    pub browser_evaluations: u64,
    pub poll_iterations: u64,
    pub dom_extractions: u64,
    pub dom_extraction_micros: u64,
    pub dom_nodes_last: usize,
    pub snapshot_render_micros: u64,
    pub handoff_rebuilds: u64,
    pub handoff_rebuild_micros: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<usize>,
}

impl OperationMetrics {
    fn is_empty(&self) -> bool {
        self.browser_evaluations == 0
            && self.poll_iterations == 0
            && self.dom_extractions == 0
            && self.dom_extraction_micros == 0
            && self.dom_nodes_last == 0
            && self.snapshot_render_micros == 0
            && self.handoff_rebuilds == 0
            && self.handoff_rebuild_micros == 0
            && self.output_bytes.is_none()
    }
}

pub(crate) fn duration_micros(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

/// Tool execution context
pub struct ToolContext<'a> {
    /// Browser session
    pub session: &'a BrowserSession,

    /// Optional DOM tree (extracted on demand)
    pub dom_tree: Option<DomTree>,

    metrics: OperationMetrics,
}

impl<'a> ToolContext<'a> {
    /// Create a new tool context
    pub fn new(session: &'a BrowserSession) -> Self {
        Self {
            session,
            dom_tree: None,
            metrics: OperationMetrics::default(),
        }
    }

    /// Create a context with a pre-extracted DOM tree
    pub fn with_dom(session: &'a BrowserSession, dom_tree: DomTree) -> Self {
        Self {
            session,
            dom_tree: Some(dom_tree),
            metrics: OperationMetrics::default(),
        }
    }

    /// Invalidate the cached DOM tree after a mutation.
    pub fn invalidate_dom(&mut self) {
        self.dom_tree = None;
    }

    /// Get or extract the DOM tree
    pub fn get_dom(&mut self) -> Result<&DomTree> {
        if self.dom_tree.is_none() {
            let started = Instant::now();
            let dom = self.session.extract_dom()?;
            self.metrics.browser_evaluations += 1;
            self.metrics.dom_extractions += 1;
            self.metrics.dom_extraction_micros += duration_micros(started.elapsed());
            self.metrics.dom_nodes_last = dom.count_nodes();
            self.dom_tree = Some(dom);
        }
        Ok(self.dom_tree.as_ref().unwrap())
    }

    /// Refresh and return the latest DOM tree after a document mutation.
    pub fn refresh_dom(&mut self) -> Result<&DomTree> {
        self.invalidate_dom();
        self.get_dom()
    }

    pub(crate) fn record_browser_evaluation(&mut self) {
        self.record_browser_evaluations(1);
    }

    pub(crate) fn record_browser_evaluations(&mut self, count: u64) {
        self.metrics.browser_evaluations += count;
    }

    pub(crate) fn record_poll_iteration(&mut self) {
        self.record_poll_iterations(1);
    }

    pub(crate) fn record_poll_iterations(&mut self, count: u64) {
        self.metrics.poll_iterations += count;
    }

    pub(crate) fn record_snapshot_render_micros(&mut self, micros: u64) {
        self.metrics.snapshot_render_micros += micros;
    }

    pub(crate) fn record_handoff_rebuild_micros(&mut self, micros: u64) {
        self.metrics.handoff_rebuilds += 1;
        self.metrics.handoff_rebuild_micros += micros;
    }

    pub(crate) fn finish(&self, mut result: ToolResult) -> ToolResult {
        if !self.metrics.is_empty() {
            result.metadata.insert(
                OPERATION_METRICS_METADATA_KEY.to_string(),
                serde_json::to_value(&self.metrics).unwrap_or_default(),
            );
        }

        result
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DocumentEnvelopeOptions {
    pub include_snapshot: bool,
    pub include_nodes: bool,
    pub snapshot_mode: SnapshotMode,
}

impl DocumentEnvelopeOptions {
    pub const fn minimal() -> Self {
        Self {
            include_snapshot: false,
            include_nodes: false,
            snapshot_mode: SnapshotMode::Viewport,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub const fn full() -> Self {
        Self::snapshot(SnapshotMode::Full)
    }

    pub const fn snapshot(snapshot_mode: SnapshotMode) -> Self {
        Self {
            include_snapshot: true,
            include_nodes: true,
            snapshot_mode,
        }
    }
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct TabSummary {
    pub tab_id: String,
    pub index: usize,
    pub active: bool,
    pub title: String,
    pub url: String,
}

impl TabSummary {
    pub fn from_browser_tab(index: usize, tab: &crate::browser::TabInfo) -> Self {
        Self {
            tab_id: tab.id.clone(),
            index,
            active: tab.active,
            title: tab.title.clone(),
            url: tab.url.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedTarget {
    pub method: String,
    pub selector: String,
    pub index: Option<usize>,
    pub node_ref: Option<NodeRef>,
    pub cursor: Option<Cursor>,
}

impl ResolvedTarget {
    pub fn to_target_envelope(&self) -> TargetEnvelope {
        let (method, resolution_status, recovered_from) = decode_target_method(&self.method);
        TargetEnvelope {
            method,
            resolution_status,
            recovered_from,
            cursor: self.cursor.clone(),
            node_ref: self.node_ref.clone(),
            selector: Some(self.selector.clone()),
            index: self.index,
        }
    }
}

#[derive(Debug)]
pub(crate) enum TargetResolution {
    Resolved(ResolvedTarget),
    Failure(ToolResult),
}

fn default_target_resolution_status() -> String {
    "exact".to_string()
}

#[derive(
    Debug, Clone, Copy, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TargetRecoveredFrom {
    Cursor,
    NodeRef,
}

impl TargetRecoveredFrom {
    fn encoded(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
            Self::NodeRef => "node_ref",
        }
    }

    fn decode(value: &str) -> Option<Self> {
        match value {
            "cursor" => Some(Self::Cursor),
            "node_ref" => Some(Self::NodeRef),
            _ => None,
        }
    }
}

const TARGET_METHOD_SELECTOR_REBOUND_MARKER: &str = "::selector_rebound::";

pub(crate) fn encode_selector_rebound_method(
    method: &str,
    recovered_from: TargetRecoveredFrom,
) -> String {
    format!(
        "{method}{TARGET_METHOD_SELECTOR_REBOUND_MARKER}{}",
        recovered_from.encoded()
    )
}

fn decode_target_method(encoded: &str) -> (String, String, Option<String>) {
    let Some((method, recovered_from)) = encoded.split_once(TARGET_METHOD_SELECTOR_REBOUND_MARKER)
    else {
        return (
            encoded.to_string(),
            default_target_resolution_status(),
            None,
        );
    };

    let Some(recovered_from) = TargetRecoveredFrom::decode(recovered_from) else {
        return (
            encoded.to_string(),
            default_target_resolution_status(),
            None,
        );
    };

    (
        method.to_string(),
        "selector_rebound".to_string(),
        Some(recovered_from.encoded().to_string()),
    )
}

fn invalid_target_failure(message: impl Into<String>) -> ToolResult {
    let message = message.into();
    structured_tool_failure("invalid_target", message, None, None, None, None)
}

fn stale_node_ref_failure(
    provided: &NodeRef,
    current: &DocumentMetadata,
    selector: Option<&str>,
    recovered_from: TargetRecoveredFrom,
) -> ToolResult {
    let selector = selector.filter(|selector| !selector.is_empty());
    structured_tool_failure(
        "stale_node_ref",
        "Stale node reference",
        Some(current.clone()),
        None,
        Some(serde_json::json!({
            "suggested_tool": "snapshot",
            "suggested_selector": selector,
        })),
        Some(serde_json::json!({
            "provided": provided,
            "resolution": {
                "status": "unrecoverable_stale",
                "recovered_from": recovered_from,
                "selector_rebound_attempted": selector.is_some(),
            },
        })),
    )
}

pub(crate) fn resolve_target_with_cursor(
    tool: &str,
    selector: Option<String>,
    index: Option<usize>,
    node_ref: Option<NodeRef>,
    cursor: Option<Cursor>,
    dom: Option<&DomTree>,
) -> Result<TargetResolution> {
    let target_count = usize::from(selector.is_some())
        + usize::from(index.is_some())
        + usize::from(node_ref.is_some())
        + usize::from(cursor.is_some());

    if target_count > 1 {
        return Ok(TargetResolution::Failure(invalid_target_failure(
            "Cannot specify more than one of 'selector', 'index', 'node_ref', or 'cursor'.",
        )));
    }

    if target_count == 0 {
        return Ok(TargetResolution::Failure(invalid_target_failure(
            "Must specify one of 'selector', 'index', 'node_ref', or 'cursor'.",
        )));
    }

    match (selector, index, node_ref, cursor) {
        (Some(selector), None, None, None) => {
            let cursor = dom.and_then(|dom| actionable_cursor_for_selector(dom, &selector));

            Ok(TargetResolution::Resolved(ResolvedTarget {
                method: "css".to_string(),
                selector,
                index: None,
                node_ref: None,
                cursor,
            }))
        }
        (None, Some(index), None, None) => {
            let dom = dom.ok_or_else(|| BrowserError::ToolExecutionFailed {
                tool: tool.to_string(),
                reason: "DOM tree is required to resolve an element index.".to_string(),
            })?;
            let cursor = dom.cursor_for_index(index).ok_or_else(|| {
                BrowserError::ElementNotFound(format!("No element with index {}", index))
            })?;

            Ok(TargetResolution::Resolved(ResolvedTarget {
                method: "index".to_string(),
                selector: cursor.selector.clone(),
                index: Some(cursor.index),
                node_ref: Some(cursor.node_ref.clone()),
                cursor: Some(cursor),
            }))
        }
        (None, None, Some(node_ref), None) => {
            let dom = dom.ok_or_else(|| BrowserError::ToolExecutionFailed {
                tool: tool.to_string(),
                reason: "DOM tree is required to resolve a node reference.".to_string(),
            })?;

            if node_ref.document_id != dom.document.document_id
                || node_ref.revision != dom.document.revision
            {
                return Ok(TargetResolution::Failure(stale_node_ref_failure(
                    &node_ref,
                    &dom.document,
                    None,
                    TargetRecoveredFrom::NodeRef,
                )));
            }

            let mut cursor = dom.cursor_for_index(node_ref.index).ok_or_else(|| {
                BrowserError::ElementNotFound(format!(
                    "No element with index {} for the provided node reference",
                    node_ref.index
                ))
            })?;
            cursor.node_ref = node_ref.clone();

            Ok(TargetResolution::Resolved(ResolvedTarget {
                method: "node_ref".to_string(),
                selector: cursor.selector.clone(),
                index: Some(node_ref.index),
                node_ref: Some(node_ref),
                cursor: Some(cursor),
            }))
        }
        (None, None, None, Some(cursor_input)) => {
            let dom = dom.ok_or_else(|| BrowserError::ToolExecutionFailed {
                tool: tool.to_string(),
                reason: "DOM tree is required to resolve a cursor.".to_string(),
            })?;

            if cursor_input.node_ref.document_id != dom.document.document_id
                || cursor_input.node_ref.revision != dom.document.revision
            {
                if !cursor_input.selector.is_empty()
                    && let Some(cursor) =
                        actionable_cursor_for_selector(dom, &cursor_input.selector)
                {
                    return Ok(TargetResolution::Resolved(ResolvedTarget {
                        method: encode_selector_rebound_method(
                            "cursor",
                            TargetRecoveredFrom::Cursor,
                        ),
                        selector: cursor.selector.clone(),
                        index: Some(cursor.index),
                        node_ref: Some(cursor.node_ref.clone()),
                        cursor: Some(cursor),
                    }));
                }

                return Ok(TargetResolution::Failure(stale_node_ref_failure(
                    &cursor_input.node_ref,
                    &dom.document,
                    Some(cursor_input.selector.as_str()),
                    TargetRecoveredFrom::Cursor,
                )));
            }

            let mut cursor = dom
                .cursor_for_index(cursor_input.node_ref.index)
                .ok_or_else(|| {
                    BrowserError::ElementNotFound(format!(
                        "No element with index {} for the provided cursor",
                        cursor_input.node_ref.index
                    ))
                })?;
            cursor.node_ref = cursor_input.node_ref.clone();

            Ok(TargetResolution::Resolved(ResolvedTarget {
                method: "cursor".to_string(),
                selector: cursor.selector.clone(),
                index: Some(cursor.index),
                node_ref: Some(cursor.node_ref.clone()),
                cursor: Some(cursor),
            }))
        }
        _ => Err(BrowserError::ToolExecutionFailed {
            tool: tool.to_string(),
            reason: "Failed to resolve target".to_string(),
        }),
    }
}

pub(crate) fn actionable_cursor_for_selector(dom: &DomTree, selector: &str) -> Option<Cursor> {
    dom.cursor_for_selector(selector)
}

fn normalized_error_value(value: Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::Object(map) if map.is_empty() => None,
        other => Some(other),
    }
}

pub(crate) fn structured_error_payload(
    code: impl Into<String>,
    error: impl Into<String>,
    document: Option<DocumentMetadata>,
    target: Option<TargetEnvelope>,
    recovery: Option<Value>,
    details: Option<Value>,
) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("code".to_string(), Value::String(code.into()));
    payload.insert("error".to_string(), Value::String(error.into()));

    if let Some(document) = document
        && let Some(value) =
            normalized_error_value(serde_json::to_value(document).unwrap_or(Value::Null))
    {
        payload.insert("document".to_string(), value);
    }

    if let Some(target) = target
        && let Some(value) =
            normalized_error_value(serde_json::to_value(target).unwrap_or(Value::Null))
    {
        payload.insert("target".to_string(), value);
    }

    if let Some(recovery) = recovery.and_then(normalized_error_value) {
        payload.insert("recovery".to_string(), recovery);
    }

    if let Some(details) = details.and_then(normalized_error_value) {
        payload.insert("details".to_string(), details);
    }

    Value::Object(payload)
}

pub(crate) fn structured_tool_failure(
    code: impl Into<String>,
    error: impl Into<String>,
    document: Option<DocumentMetadata>,
    target: Option<TargetEnvelope>,
    recovery: Option<Value>,
    details: Option<Value>,
) -> ToolResult {
    let error = error.into();
    ToolResult::failure_with(
        error.clone(),
        structured_error_payload(code, error, document, target, recovery, details),
    )
}

fn structured_failure(code: &str, error: String) -> ToolResult {
    structured_tool_failure(code, error, None, None, None, None)
}

fn attach_session_degraded_failure(details: PageTargetLostDetails) -> ToolResult {
    structured_tool_failure(
        ATTACH_PAGE_TARGET_LOST_CODE,
        details.detail.clone(),
        None,
        None,
        Some(serde_json::json!({
            "suggested_tool": "tab_list",
            "hint": details.recovery_hint.clone().unwrap_or_default(),
        })),
        Some(serde_json::json!({
            "kind": ATTACH_SESSION_PAGE_TARGET_LOSS_KIND,
            "operation": details.operation.clone(),
            "session_origin": "connected",
        })),
    )
}

pub(crate) fn tool_result_from_browser_error(
    error: BrowserError,
) -> std::result::Result<ToolResult, BrowserError> {
    match error {
        BrowserError::LaunchFailed(message) => Err(BrowserError::LaunchFailed(message)),
        BrowserError::ConnectionFailed(message) => Err(BrowserError::ConnectionFailed(message)),
        BrowserError::ChromeError(message) => Err(BrowserError::ChromeError(message)),
        BrowserError::InvalidArgument(reason) => Ok(structured_failure("invalid_argument", reason)),
        BrowserError::Timeout(reason) => Ok(structured_failure("timeout", reason)),
        BrowserError::SelectorInvalid(reason) => Ok(structured_failure(
            "invalid_selector",
            format!("Invalid selector: {}", reason),
        )),
        BrowserError::ElementNotFound(reason) => Ok(structured_failure(
            "element_not_found",
            format!("Element not found: {}", reason),
        )),
        BrowserError::DomParseFailed(reason) => Ok(structured_failure(
            "dom_parse_failed",
            format!("Failed to parse DOM: {}", reason),
        )),
        BrowserError::ToolExecutionFailed { tool, reason } => Ok(structured_tool_failure(
            "tool_execution_failed",
            reason,
            None,
            None,
            None,
            Some(serde_json::json!({
                "tool": tool,
            })),
        )),
        BrowserError::NavigationFailed(reason) => {
            Ok(structured_failure("navigation_failed", reason))
        }
        BrowserError::EvaluationFailed(reason) => {
            Ok(structured_failure("evaluation_failed", reason))
        }
        BrowserError::ScreenshotFailed(reason) => {
            Ok(structured_failure("screenshot_failed", reason))
        }
        BrowserError::DownloadFailed(reason) => Ok(structured_failure("download_failed", reason)),
        BrowserError::TabOperationFailed(reason) => {
            Ok(structured_failure("tab_operation_failed", reason))
        }
        BrowserError::PageTargetLost(details) => {
            if details.is_attach_session_degraded() {
                return Ok(attach_session_degraded_failure(details));
            }

            Ok(structured_tool_failure(
                ATTACH_PAGE_TARGET_LOST_CODE,
                details.detail.clone(),
                None,
                None,
                None,
                Some(serde_json::json!({
                    "kind": ATTACH_SESSION_PAGE_TARGET_LOSS_KIND,
                    "operation": details.operation,
                    "recoverable": details.recoverable,
                })),
            ))
        }
        BrowserError::BackendUnsupported(details) => Ok(structured_tool_failure(
            "backend_unsupported",
            details.to_string(),
            None,
            None,
            None,
            Some(serde_json::json!({
                "capability": details.capability,
                "operation": details.operation,
            })),
        )),
        BrowserError::JsonError(error) => Ok(structured_failure(
            "json_error",
            format!("JSON error: {}", error),
        )),
        BrowserError::IoError(error) => Ok(structured_failure(
            "io_error",
            format!("IO error: {}", error),
        )),
    }
}

pub(crate) fn normalize_tool_outcome(
    outcome: Result<ToolResult>,
    context: &ToolContext<'_>,
) -> Result<ToolResult> {
    match outcome {
        Ok(result) => Ok(context.finish(result)),
        Err(error) => match tool_result_from_browser_error(error) {
            Ok(result) => Ok(context.finish(result)),
            Err(error) => Err(error),
        },
    }
}

fn live_viewport_metrics(context: &mut ToolContext<'_>) -> Result<ViewportMetrics> {
    context.record_browser_evaluation();
    context.session.viewport_metrics(None)
}

pub(crate) fn build_document_envelope(
    context: &mut ToolContext,
    target: Option<&ResolvedTarget>,
    options: DocumentEnvelopeOptions,
) -> Result<DocumentEnvelope> {
    let target = target.map(|resolved| resolved.to_target_envelope());

    if options.include_snapshot || options.include_nodes {
        let (document, snapshot, nodes, global_interactive_count, mut scope, render_micros) = {
            let dom = context.get_dom()?;
            let document = dom.document.clone();
            let global_interactive_count = Some(dom.count_interactive());
            let current_projection = match options.snapshot_mode {
                SnapshotMode::Full => {
                    snapshot_projection(dom, SnapshotMode::Full, global_interactive_count)
                }
                SnapshotMode::Viewport | SnapshotMode::Delta => {
                    snapshot_projection(dom, SnapshotMode::Viewport, global_interactive_count)
                }
            };

            let (projection, cache_base) = match options.snapshot_mode {
                SnapshotMode::Delta => {
                    delta_snapshot_projection(context, &document, current_projection)?
                }
                SnapshotMode::Viewport => (current_projection.clone(), Some(current_projection)),
                SnapshotMode::Full => (current_projection, None),
            };

            if let Some(cache_base) = cache_base {
                context.session.store_snapshot_cache(Arc::new(
                    snapshot_cache_entry_from_projection(&document, &cache_base),
                ))?;
            }

            (
                document,
                options
                    .include_snapshot
                    .then(|| projection.snapshot.as_ref().to_string()),
                if options.include_nodes {
                    projection.nodes.iter().cloned().collect()
                } else {
                    Vec::new()
                },
                options
                    .include_nodes
                    .then_some(global_interactive_count)
                    .flatten(),
                Some(projection.scope),
                projection.render_micros,
            )
        };

        if let Some(scope) = scope.as_mut() {
            scope.viewport = Some(live_viewport_metrics(context)?);
        }

        if options.include_snapshot {
            context.record_snapshot_render_micros(render_micros);
        }

        return Ok(DocumentEnvelope {
            document,
            target,
            snapshot,
            nodes,
            scope,
            global_interactive_count,
        });
    }

    Ok(DocumentEnvelope {
        document: {
            context.record_browser_evaluation();
            context.session.document_metadata()?
        },
        target,
        snapshot: None,
        nodes: Vec::new(),
        scope: None,
        global_interactive_count: None,
    })
}

#[derive(Debug, Clone, Copy)]
struct SnapshotAnchorPolicy {
    strict_viewport: bool,
    allow_persistent_chrome: bool,
}

#[derive(Debug)]
enum ScopedSnapshotChild<'a> {
    Text(&'a str),
    Node(Box<ScopedSnapshotNode<'a>>),
}

#[derive(Debug)]
struct ScopedSnapshotNode<'a> {
    node: &'a AriaNode,
    public_handle: bool,
    children: Vec<ScopedSnapshotChild<'a>>,
}

fn build_scoped_snapshot_root<'a>(
    root: &'a AriaNode,
    policy: SnapshotAnchorPolicy,
) -> ScopedSnapshotNode<'a> {
    build_scoped_snapshot_node(root, policy, true)
        .expect("snapshot root should always remain renderable")
}

#[derive(Debug, Clone)]
struct SnapshotProjection {
    snapshot: Arc<str>,
    nodes: Arc<[SnapshotNode]>,
    scope: SnapshotScope,
    render_micros: u64,
}

fn snapshot_projection(
    dom: &DomTree,
    mode: SnapshotMode,
    global_interactive_count: Option<usize>,
) -> SnapshotProjection {
    let (snapshot, nodes, locality_fallback_reason, render_micros) = match mode {
        SnapshotMode::Full => {
            let started = Instant::now();
            (
                Arc::<str>::from(render_aria_tree(&dom.root, RenderMode::Ai, None)),
                Arc::<[SnapshotNode]>::from(snapshot_nodes_from_root(dom, &dom.root)),
                None,
                duration_micros(started.elapsed()),
            )
        }
        SnapshotMode::Viewport | SnapshotMode::Delta => {
            let viewport_local = build_scoped_snapshot_root(
                &dom.root,
                SnapshotAnchorPolicy {
                    strict_viewport: true,
                    allow_persistent_chrome: false,
                },
            );
            let (snapshot_root, locality_fallback_reason) = if !viewport_local.children.is_empty() {
                (viewport_local, None)
            } else {
                let viewport_with_persistent = build_scoped_snapshot_root(
                    &dom.root,
                    SnapshotAnchorPolicy {
                        strict_viewport: true,
                        allow_persistent_chrome: true,
                    },
                );
                if !viewport_with_persistent.children.is_empty() {
                    (
                        viewport_with_persistent,
                        Some("persistent_chrome_only".to_string()),
                    )
                } else {
                    let visible_document = build_scoped_snapshot_root(
                        &dom.root,
                        SnapshotAnchorPolicy {
                            strict_viewport: false,
                            allow_persistent_chrome: false,
                        },
                    );
                    if !visible_document.children.is_empty() {
                        (
                            visible_document,
                            Some("no_viewport_local_anchors".to_string()),
                        )
                    } else {
                        (
                            build_scoped_snapshot_root(
                                &dom.root,
                                SnapshotAnchorPolicy {
                                    strict_viewport: false,
                                    allow_persistent_chrome: true,
                                },
                            ),
                            Some("document_visible_fallback".to_string()),
                        )
                    }
                }
            };

            let started = Instant::now();
            (
                Arc::<str>::from(render_scoped_snapshot_root(&snapshot_root, RenderMode::Ai)),
                Arc::<[SnapshotNode]>::from(snapshot_nodes_from_scoped_root(dom, &snapshot_root)),
                locality_fallback_reason,
                duration_micros(started.elapsed()),
            )
        }
    };

    let returned_node_count = nodes.len();

    SnapshotProjection {
        snapshot,
        nodes,
        scope: SnapshotScope {
            mode,
            fallback_mode: None,
            viewport_biased: mode != SnapshotMode::Full,
            locality_fallback_reason,
            unavailable_frame_count: dom
                .document
                .frames
                .iter()
                .filter(|frame| frame.status != "expanded")
                .count(),
            returned_node_count,
            global_interactive_count,
            viewport: None,
        },
        render_micros,
    }
}

fn delta_snapshot_projection(
    context: &ToolContext<'_>,
    document: &DocumentMetadata,
    current_projection: SnapshotProjection,
) -> Result<(SnapshotProjection, Option<SnapshotProjection>)> {
    let Some(base) = context.session.snapshot_cache_entry(document)? else {
        let mut fallback = current_projection.clone();
        fallback.scope.mode = SnapshotMode::Delta;
        fallback.scope.fallback_mode = Some(SnapshotMode::Viewport);
        return Ok((fallback, Some(current_projection)));
    };

    if base.scope.mode == "full" {
        let mut fallback = current_projection.clone();
        fallback.scope.mode = SnapshotMode::Delta;
        fallback.scope.fallback_mode = Some(SnapshotMode::Viewport);
        return Ok((fallback, Some(current_projection)));
    }

    let mut projection = current_projection.clone();
    projection.scope.mode = SnapshotMode::Delta;
    projection.scope.fallback_mode = None;
    projection.snapshot = Arc::<str>::from(delta_snapshot_text(
        base.snapshot.as_ref(),
        current_projection.snapshot.as_ref(),
    ));

    let mut nodes = delta_snapshot_nodes(&base.nodes, &current_projection.nodes);
    if nodes.is_empty() && current_projection.snapshot.as_ref() != base.snapshot.as_ref() {
        projection.nodes = Arc::clone(&current_projection.nodes);
    } else {
        projection.nodes = Arc::<[SnapshotNode]>::from(std::mem::take(&mut nodes));
    }
    projection.scope.returned_node_count = projection.nodes.len();

    if projection.snapshot.is_empty() {
        projection.snapshot = Arc::clone(&current_projection.snapshot);
    }

    Ok((projection, Some(current_projection)))
}

fn snapshot_cache_entry_from_projection(
    document: &DocumentMetadata,
    projection: &SnapshotProjection,
) -> SnapshotCacheEntry {
    SnapshotCacheEntry {
        document: document.clone(),
        snapshot: Arc::clone(&projection.snapshot),
        nodes: Arc::clone(&projection.nodes),
        scope: SnapshotCacheScope {
            mode: snapshot_mode_label(projection.scope.mode).to_string(),
            fallback_mode: projection
                .scope
                .fallback_mode
                .map(|mode| snapshot_mode_label(mode).to_string()),
            viewport_biased: projection.scope.viewport_biased,
            returned_node_count: projection.scope.returned_node_count,
            unavailable_frame_count: projection.scope.unavailable_frame_count,
            global_interactive_count: projection.scope.global_interactive_count,
        },
    }
}

fn snapshot_mode_label(mode: SnapshotMode) -> &'static str {
    match mode {
        SnapshotMode::Viewport => "viewport",
        SnapshotMode::Delta => "delta",
        SnapshotMode::Full => "full",
    }
}

fn delta_snapshot_text(previous: &str, current: &str) -> String {
    let mut previous_counts = HashMap::new();
    for line in previous.lines() {
        *previous_counts.entry(line).or_insert(0usize) += 1;
    }

    let mut changed_lines = Vec::new();
    for line in current.lines() {
        match previous_counts.get_mut(line) {
            Some(count) if *count > 0 => {
                *count -= 1;
            }
            _ => changed_lines.push(line.to_string()),
        }
    }

    changed_lines.join("\n")
}

fn delta_snapshot_nodes(previous: &[SnapshotNode], current: &[SnapshotNode]) -> Vec<SnapshotNode> {
    let previous_by_selector = previous
        .iter()
        .map(|node| (node.cursor.selector.as_str(), node))
        .collect::<HashMap<_, _>>();

    current
        .iter()
        .filter(
            |node| match previous_by_selector.get(node.cursor.selector.as_str()) {
                Some(previous_node) => **previous_node != **node,
                None => true,
            },
        )
        .cloned()
        .collect()
}

fn build_scoped_snapshot_node<'a>(
    node: &'a AriaNode,
    policy: SnapshotAnchorPolicy,
    is_root: bool,
) -> Option<ScopedSnapshotNode<'a>> {
    let is_anchor = node_is_snapshot_anchor(node, policy);
    let mut scoped_children = Vec::new();
    let mut child_anchor_count = 0usize;

    for child in &node.children {
        match child {
            AriaChild::Text(text) => {
                if is_anchor || is_root {
                    scoped_children.push(ScopedSnapshotChild::Text(text));
                }
            }
            AriaChild::Node(child_node) => {
                if let Some(scoped_child) = build_scoped_snapshot_node(child_node, policy, false) {
                    child_anchor_count += 1;
                    scoped_children.push(ScopedSnapshotChild::Node(Box::new(scoped_child)));
                }
            }
        }
    }

    let public_handle = node.has_public_handle()
        && (node.carries_snapshot_state() || node_matches_policy(node, policy));

    let keep = is_root || is_anchor || child_anchor_count > 0;
    if !keep {
        return None;
    }

    let has_child_nodes = scoped_children
        .iter()
        .any(|child| matches!(child, ScopedSnapshotChild::Node(_)));
    let is_noise = node.role == "generic"
        && node.name.is_empty()
        && node.props.is_empty()
        && !public_handle
        && !node.carries_snapshot_state()
        && !has_child_nodes;

    if !is_root && is_noise {
        return None;
    }

    Some(ScopedSnapshotNode {
        node,
        public_handle,
        children: scoped_children,
    })
}

fn node_matches_policy(node: &AriaNode, policy: SnapshotAnchorPolicy) -> bool {
    let visible = if policy.strict_viewport {
        node.box_info.in_viewport
    } else {
        node.box_info.visible
    };

    visible && (policy.allow_persistent_chrome || !node.is_persistent_chrome())
}

fn node_is_snapshot_anchor(node: &AriaNode, policy: SnapshotAnchorPolicy) -> bool {
    node.carries_snapshot_state() || node_matches_policy(node, policy)
}

fn snapshot_nodes_from_root(dom: &DomTree, root: &AriaNode) -> Vec<SnapshotNode> {
    let mut nodes = Vec::new();
    collect_snapshot_nodes(dom, root, &mut nodes);
    nodes
}

fn snapshot_nodes_from_scoped_root(
    dom: &DomTree,
    root: &ScopedSnapshotNode<'_>,
) -> Vec<SnapshotNode> {
    let mut nodes = Vec::new();
    collect_scoped_snapshot_nodes(dom, root, &mut nodes);
    nodes
}

fn collect_snapshot_nodes(dom: &DomTree, node: &AriaNode, nodes: &mut Vec<SnapshotNode>) {
    if node.has_public_handle()
        && let Some(index) = node.index
        && let Some(cursor) = dom.cursor_for_index(index)
    {
        nodes.push(SnapshotNode {
            node_ref: cursor.node_ref.clone(),
            index: cursor.index,
            role: cursor.role.clone(),
            name: cursor.name.clone(),
            cursor,
        });
    }

    for child in &node.children {
        if let AriaChild::Node(child_node) = child {
            collect_snapshot_nodes(dom, child_node, nodes);
        }
    }
}

fn collect_scoped_snapshot_nodes(
    dom: &DomTree,
    node: &ScopedSnapshotNode<'_>,
    nodes: &mut Vec<SnapshotNode>,
) {
    if node.public_handle
        && let Some(index) = node.node.index
        && let Some(cursor) = dom.cursor_for_index(index)
    {
        nodes.push(SnapshotNode {
            node_ref: cursor.node_ref.clone(),
            index: cursor.index,
            role: cursor.role.clone(),
            name: cursor.name.clone(),
            cursor,
        });
    }

    for child in &node.children {
        if let ScopedSnapshotChild::Node(child_node) = child {
            collect_scoped_snapshot_nodes(dom, child_node, nodes);
        }
    }
}

fn render_scoped_snapshot_root(root: &ScopedSnapshotNode<'_>, mode: RenderMode) -> String {
    let mut lines = Vec::new();
    let render_cursor_pointer = matches!(mode, RenderMode::Ai);
    let render_active = matches!(mode, RenderMode::Ai);

    if root.node.role == "fragment" {
        for child in &root.children {
            match child {
                ScopedSnapshotChild::Text(text) => visit_scoped_text(text, "", &mut lines),
                ScopedSnapshotChild::Node(node) => {
                    visit_scoped_node(node, "", render_cursor_pointer, render_active, &mut lines)
                }
            }
        }
    } else {
        visit_scoped_node(root, "", render_cursor_pointer, render_active, &mut lines);
    }

    lines.join("\n")
}

fn visit_scoped_text(text: &str, indent: &str, lines: &mut Vec<String>) {
    let escaped = yaml_escape_value_if_needed(text);
    if !escaped.is_empty() {
        lines.push(format!("{}- text: {}", indent, escaped));
    }
}

fn visit_scoped_node(
    node: &ScopedSnapshotNode<'_>,
    indent: &str,
    render_cursor_pointer: bool,
    render_active: bool,
    lines: &mut Vec<String>,
) {
    let key = create_scoped_snapshot_key(node, render_cursor_pointer, render_active);
    let escaped_key = format!("{}- {}", indent, yaml_escape_key_if_needed(&key));

    if node.children.is_empty() && node.node.props.is_empty() {
        lines.push(escaped_key);
        return;
    }

    if let Some(text) = scoped_single_inlined_text_child(node) {
        lines.push(format!(
            "{}: {}",
            escaped_key,
            yaml_escape_value_if_needed(text)
        ));
        return;
    }

    lines.push(format!("{}:", escaped_key));

    for (name, value) in &node.node.props {
        lines.push(format!(
            "{}  - /{}: {}",
            indent,
            name,
            yaml_escape_value_if_needed(value)
        ));
    }

    let child_indent = format!("{}  ", indent);
    let in_cursor_pointer =
        node.public_handle && render_cursor_pointer && node.node.has_pointer_cursor();

    for child in &node.children {
        match child {
            ScopedSnapshotChild::Text(text) => visit_scoped_text(text, &child_indent, lines),
            ScopedSnapshotChild::Node(child_node) => visit_scoped_node(
                child_node,
                &child_indent,
                render_cursor_pointer && !in_cursor_pointer,
                render_active,
                lines,
            ),
        }
    }
}

fn create_scoped_snapshot_key(
    node: &ScopedSnapshotNode<'_>,
    render_cursor_pointer: bool,
    render_active: bool,
) -> String {
    let aria_node = node.node;
    let mut key = aria_node.role.clone();

    if !aria_node.name.is_empty() && aria_node.name.len() <= 900 {
        key.push(' ');
        key.push_str(&format!("{:?}", aria_node.name));
    }

    if let Some(checked) = &aria_node.checked {
        match checked {
            crate::dom::element::AriaChecked::Bool(true) => key.push_str(" [checked]"),
            crate::dom::element::AriaChecked::Bool(false) => {}
            crate::dom::element::AriaChecked::Mixed(_) => key.push_str(" [checked=mixed]"),
        }
    }

    if aria_node.disabled == Some(true) {
        key.push_str(" [disabled]");
    }

    if aria_node.expanded == Some(true) {
        key.push_str(" [expanded]");
    }

    if render_active && aria_node.active == Some(true) {
        key.push_str(" [active]");
    }

    if let Some(level) = aria_node.level {
        key.push_str(&format!(" [level={}]", level));
    }

    if let Some(pressed) = &aria_node.pressed {
        match pressed {
            crate::dom::element::AriaPressed::Bool(true) => key.push_str(" [pressed]"),
            crate::dom::element::AriaPressed::Bool(false) => {}
            crate::dom::element::AriaPressed::Mixed(_) => key.push_str(" [pressed=mixed]"),
        }
    }

    if aria_node.selected == Some(true) {
        key.push_str(" [selected]");
    }

    if let Some(index) = aria_node.index.filter(|_| node.public_handle) {
        key.push_str(&format!(" [index={}]", index));

        if render_cursor_pointer && aria_node.has_pointer_cursor() {
            key.push_str(" [cursor=pointer]");
        }
    }

    key
}

fn scoped_single_inlined_text_child<'a>(node: &'a ScopedSnapshotNode<'a>) -> Option<&'a str> {
    if node.children.len() == 1
        && node.node.props.is_empty()
        && let ScopedSnapshotChild::Text(text) = &node.children[0]
    {
        return Some(text);
    }
    None
}

/// Trait for browser automation tools with associated parameter types
pub trait Tool: Send + Sync + Default {
    /// Associated parameter type for this tool
    type Params: serde::Serialize + for<'de> serde::Deserialize<'de> + schemars::JsonSchema;

    /// Associated success output type for this tool
    type Output: serde::Serialize + schemars::JsonSchema + 'static;

    /// Get tool name
    fn name(&self) -> &str;

    /// Get tool description for registry and MCP surfaces.
    fn description(&self) -> &str;

    /// Get tool parameter schema (JSON Schema)
    fn parameters_schema(&self) -> Value {
        serde_json::to_value(schemars::schema_for!(Self::Params)).unwrap_or_default()
    }

    /// Get tool success output schema (JSON Schema)
    fn output_schema(&self) -> Value {
        serde_json::to_value(schemars::schema_for!(Self::Output)).unwrap_or_default()
    }

    /// Execute the tool with strongly-typed parameters
    fn execute_typed(&self, params: Self::Params, context: &mut ToolContext) -> Result<ToolResult>;

    /// Execute the tool with JSON parameters (default implementation)
    fn execute(&self, params: Value, context: &mut ToolContext) -> Result<ToolResult> {
        let typed_params: Self::Params = serde_json::from_value(params).map_err(|e| {
            crate::error::BrowserError::InvalidArgument(format!("Invalid parameters: {}", e))
        })?;
        self.execute_typed(typed_params, context)
    }
}

/// Type-erased tool trait for dynamic dispatch
pub trait DynTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    fn output_schema(&self) -> Value;
    fn execute(&self, params: Value, context: &mut ToolContext) -> Result<ToolResult>;
}

/// Blanket implementation to convert any Tool into DynTool
impl<T: Tool> DynTool for T {
    fn name(&self) -> &str {
        Tool::name(self)
    }

    fn description(&self) -> &str {
        Tool::description(self)
    }

    fn parameters_schema(&self) -> Value {
        Tool::parameters_schema(self)
    }

    fn output_schema(&self) -> Value {
        Tool::output_schema(self)
    }

    fn execute(&self, params: Value, context: &mut ToolContext) -> Result<ToolResult> {
        Tool::execute(self, params, context)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub parameters_schema: Value,
    pub output_schema: Value,
}

/// Tool registry for managing and accessing tools
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn DynTool>>,
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Create a registry with the default high-level agent tools.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register_default_tools();
        registry
    }

    /// Create a registry with the default high-level agent tools plus advanced operator tools.
    pub fn with_all_tools() -> Self {
        let mut registry = Self::with_defaults();
        registry.register_operator_tools();
        registry
    }

    /// Register the default high-level agent tool surface.
    pub fn register_default_tools(&mut self) {
        // Register navigation tools
        self.register(navigate::NavigateTool);
        self.register(go_back::GoBackTool);
        self.register(go_forward::GoForwardTool);
        self.register(wait::WaitTool);

        // Register interaction tools
        self.register(click::ClickTool);
        self.register(input::InputTool);
        self.register(select::SelectTool);
        self.register(hover::HoverTool);
        self.register(press_key::PressKeyTool);
        self.register(scroll::ScrollTool);
        self.register(set_viewport::SetViewportTool);

        // Register tab management tools
        self.register(new_tab::NewTabTool);
        self.register(tab_list::TabListTool);
        self.register(switch_tab::SwitchTabTool);
        self.register(close_tab::CloseTabTool);

        // Register reading and extraction tools
        self.register(extract::ExtractContentTool);
        self.register(markdown::GetMarkdownTool);
        self.register(read_links::ReadLinksTool);
        self.register(snapshot::SnapshotTool);
        self.register(inspect_node::InspectNodeTool);
        self.register(screenshot::ScreenshotTool);
        self.register(close::CloseTool);
    }

    /// Register advanced operator tools such as raw JavaScript evaluation.
    pub fn register_operator_tools(&mut self) {
        self.register(evaluate::EvaluateTool);
    }

    /// Register a tool
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::new(tool));
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<&Arc<dyn DynTool>> {
        self.tools.get(name)
    }

    /// Check if a tool exists
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// List all tool names
    pub fn list_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }

    /// Get all tools
    pub fn all_tools(&self) -> Vec<Arc<dyn DynTool>> {
        self.tools.values().cloned().collect()
    }

    /// List registry tool descriptors in a stable order.
    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        let mut descriptors: Vec<_> = self
            .tools
            .values()
            .map(|tool| ToolDescriptor {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters_schema: tool.parameters_schema(),
                output_schema: tool.output_schema(),
            })
            .collect();
        descriptors.sort_by(|left, right| left.name.cmp(&right.name));
        descriptors
    }

    /// Execute a tool by name
    pub fn execute(
        &self,
        name: &str,
        params: Value,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let outcome = match self.get(name) {
            Some(tool) => tool.execute(params, context),
            None => Ok(ToolResult::failure(format!("Tool '{}' not found", name))),
        };

        normalize_tool_outcome(outcome, context)
    }

    /// Get the number of registered tools
    pub fn count(&self) -> usize {
        self.tools.len()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::backend::FakeSessionBackend;
    use crate::browser::{SnapshotCacheEntry, SnapshotCacheScope};
    use crate::dom::{AriaChild, AriaNode, DomTree};
    use schemars::schema_for;
    use serde_json::json;
    use std::sync::Arc;

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
    fn public_target_deserializes_plain_selector_string() {
        let target: PublicTarget =
            serde_json::from_value(json!("#submit")).expect("plain selector should deserialize");

        assert_eq!(
            target,
            PublicTarget::Selector {
                selector: "#submit".to_string(),
            }
        );
    }

    #[test]
    fn public_target_schema_mentions_string_and_cursor_variants() {
        let schema = schema_for!(PublicTarget);
        let schema_json = serde_json::to_string(&schema).expect("schema should serialize");

        assert!(schema_json.contains("\"type\":\"string\""));
        assert!(schema_json.contains("\"kind\""));
        assert!(schema_json.contains("\"selector\""));
        assert!(schema_json.contains("\"cursor\""));
    }

    fn viewport_dom() -> DomTree {
        let mut offscreen_button = AriaNode::new("button", "Offscreen save")
            .with_index(0)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));
        offscreen_button.box_info.in_viewport = false;

        let visible_heading = AriaNode::new("heading", "Visible story").with_box(true, None);

        let visible_tab = AriaNode::new("button", "Visible tab")
            .with_index(1)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()))
            .with_selected(true);

        let root = AriaNode::fragment()
            .with_child(AriaChild::Node(Box::new(offscreen_button)))
            .with_child(AriaChild::Node(Box::new(visible_heading)))
            .with_child(AriaChild::Node(Box::new(visible_tab)));

        let mut dom = DomTree::new(root);
        dom.document.document_id = "doc-viewport".to_string();
        dom.document.revision = "main:4".to_string();
        dom.replace_selectors(vec![
            "#offscreen-save".to_string(),
            "#visible-tab".to_string(),
        ]);
        dom
    }

    fn persistent_chrome_dom() -> DomTree {
        let mut header_button = AriaNode::new("button", "Header action")
            .with_index(0)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));
        header_button.box_info.persistent_chrome = true;
        header_button.box_info.persistent_position = Some("sticky".to_string());
        header_button.box_info.persistent_edge = Some("top".to_string());

        let local_heading = AriaNode::new("heading", "Local section").with_box(true, None);

        let local_button = AriaNode::new("button", "Local action")
            .with_index(1)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));

        let root = AriaNode::fragment()
            .with_child(AriaChild::Node(Box::new(header_button)))
            .with_child(AriaChild::Node(Box::new(local_heading)))
            .with_child(AriaChild::Node(Box::new(local_button)));

        let mut dom = DomTree::new(root);
        dom.document.document_id = "doc-persistent".to_string();
        dom.document.revision = "main:9".to_string();
        dom.replace_selectors(vec![
            "#header-action".to_string(),
            "#local-action".to_string(),
        ]);
        dom
    }

    fn persistent_chrome_only_dom() -> DomTree {
        let mut header_button = AriaNode::new("button", "Header action")
            .with_index(0)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));
        header_button.box_info.persistent_chrome = true;
        header_button.box_info.persistent_position = Some("fixed".to_string());
        header_button.box_info.persistent_edge = Some("top".to_string());

        let mut hidden_content = AriaNode::new("button", "Hidden content")
            .with_index(1)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));
        hidden_content.box_info.in_viewport = false;

        let root = AriaNode::fragment()
            .with_child(AriaChild::Node(Box::new(header_button)))
            .with_child(AriaChild::Node(Box::new(hidden_content)));

        let mut dom = DomTree::new(root);
        dom.document.document_id = "doc-persistent-only".to_string();
        dom.document.revision = "main:2".to_string();
        dom.replace_selectors(vec![
            "#header-action".to_string(),
            "#hidden-content".to_string(),
        ]);
        dom
    }

    fn persistent_delta_dom(details_visible: bool) -> DomTree {
        let mut header_button = AriaNode::new("button", "Header action")
            .with_index(0)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));
        header_button.box_info.persistent_chrome = true;
        header_button.box_info.persistent_position = Some("sticky".to_string());
        header_button.box_info.persistent_edge = Some("top".to_string());

        let local_toggle = AriaNode::new("button", "Show details")
            .with_index(1)
            .with_public_handle(true)
            .with_box(true, Some("pointer".to_string()));

        let mut root = AriaNode::fragment()
            .with_child(AriaChild::Node(Box::new(header_button)))
            .with_child(AriaChild::Node(Box::new(
                AriaNode::new("heading", "Local section").with_box(true, None),
            )))
            .with_child(AriaChild::Node(Box::new(local_toggle)));

        if details_visible {
            root = root.with_child(AriaChild::Node(Box::new(
                AriaNode::new("button", "Details")
                    .with_index(2)
                    .with_public_handle(true)
                    .with_box(true, Some("pointer".to_string())),
            )));
        }

        let mut dom = DomTree::new(root);
        dom.document.document_id = "doc-persistent-delta".to_string();
        dom.document.revision = if details_visible {
            "main:7".to_string()
        } else {
            "main:6".to_string()
        };
        dom.replace_selectors(vec![
            "#header-action".to_string(),
            "#toggle".to_string(),
            "#details".to_string(),
        ]);
        dom
    }

    #[test]
    fn tool_result_maps_attach_page_target_loss_to_structured_degraded_failure() {
        let error = BrowserError::PageTargetLost(PageTargetLostDetails::attach_degraded(
            "snapshot",
            "Attached browser session lost its active page target during snapshot.".to_string(),
            "Run tab_list, then switch_tab to reacquire an active page target.",
        ));

        let result = tool_result_from_browser_error(error)
            .expect("degraded attach failures stay tool-local");

        assert!(!result.success);
        let data = result
            .data
            .expect("structured failure data should be present");
        assert_eq!(data["code"].as_str(), Some(ATTACH_PAGE_TARGET_LOST_CODE));
        assert_eq!(data["details"]["kind"].as_str(), Some("page_target_lost"));
        assert_eq!(data["details"]["operation"].as_str(), Some("snapshot"));
        assert_eq!(
            data["details"]["session_origin"].as_str(),
            Some("connected")
        );
        assert_eq!(
            data["recovery"]["suggested_tool"].as_str(),
            Some("tab_list")
        );
        assert!(
            data["recovery"]["hint"]
                .as_str()
                .unwrap_or_default()
                .contains("tab_list")
        );
    }

    #[test]
    fn tool_result_maps_backend_unsupported_to_structured_failure() {
        let error = BrowserError::BackendUnsupported(BackendUnsupportedDetails::new(
            "viewport_metrics",
            "snapshot",
        ));

        let result = tool_result_from_browser_error(error)
            .expect("unsupported backend capabilities stay tool-local");

        assert!(!result.success);
        let data = result
            .data
            .expect("structured failure data should be present");
        assert_eq!(data["code"].as_str(), Some("backend_unsupported"));
        assert_eq!(
            data["details"]["capability"].as_str(),
            Some("viewport_metrics")
        );
        assert_eq!(data["details"]["operation"].as_str(), Some("snapshot"));
    }

    #[test]
    fn resolve_target_with_cursor_rebinds_stale_cursor_via_selector() {
        let dom = sample_dom();
        let mut stale_cursor = dom.cursor_for_index(1).expect("cursor should exist");
        stale_cursor.node_ref.revision = "main:0".to_string();

        let result =
            resolve_target_with_cursor("click", None, None, None, Some(stale_cursor), Some(&dom))
                .expect("stale cursor should resolve via selector rebound");

        match result {
            TargetResolution::Resolved(target) => {
                let rebound_cursor = target
                    .cursor
                    .as_ref()
                    .expect("rebound cursor should be present");
                assert_eq!(
                    target.method,
                    encode_selector_rebound_method("cursor", TargetRecoveredFrom::Cursor)
                );
                assert_eq!(target.selector, "#submit");
                assert_eq!(target.index, Some(1));
                assert_eq!(rebound_cursor.selector, "#submit");
                assert_eq!(rebound_cursor.node_ref.document_id, "doc-1");
                assert_eq!(rebound_cursor.node_ref.revision, "main:1");
                assert_eq!(rebound_cursor.node_ref.index, 1);
                let envelope = target.to_target_envelope();
                assert_eq!(envelope.method, "cursor");
                assert_eq!(envelope.resolution_status, "selector_rebound");
                assert_eq!(envelope.recovered_from.as_deref(), Some("cursor"));
            }
            TargetResolution::Failure(failure) => panic!("unexpected failure: {:?}", failure),
        }
    }

    #[test]
    fn build_document_envelope_viewport_mode_scopes_snapshot_handles() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let dom = viewport_dom();
        let mut context = ToolContext::with_dom(&session, dom);

        let envelope = build_document_envelope(
            &mut context,
            None,
            DocumentEnvelopeOptions::snapshot(SnapshotMode::Viewport),
        )
        .expect("viewport envelope should build");

        assert_eq!(
            envelope.scope.as_ref().map(|scope| scope.mode),
            Some(SnapshotMode::Viewport)
        );
        assert_eq!(
            envelope.scope.as_ref().map(|scope| scope.viewport_biased),
            Some(true)
        );
        assert_eq!(
            envelope
                .scope
                .as_ref()
                .and_then(|scope| scope.viewport.clone()),
            Some(ViewportMetrics {
                width: 800.0,
                height: 600.0,
                device_pixel_ratio: 2.0,
            })
        );
        assert_eq!(
            envelope
                .scope
                .as_ref()
                .and_then(|scope| scope.locality_fallback_reason.as_deref()),
            None
        );
        assert_eq!(envelope.global_interactive_count, Some(2));
        assert_eq!(envelope.nodes.len(), 1);
        assert_eq!(envelope.nodes[0].name, "Visible tab");
        let snapshot = envelope.snapshot.expect("viewport snapshot should render");
        assert!(snapshot.contains("Visible story"));
        assert!(snapshot.contains("Visible tab"));
        assert!(!snapshot.contains("Offscreen save"));
    }

    #[test]
    fn build_document_envelope_full_mode_preserves_exhaustive_snapshot_handles() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let dom = viewport_dom();
        let mut context = ToolContext::with_dom(&session, dom);

        let envelope = build_document_envelope(
            &mut context,
            None,
            DocumentEnvelopeOptions::snapshot(SnapshotMode::Full),
        )
        .expect("full envelope should build");

        assert_eq!(
            envelope.scope.as_ref().map(|scope| scope.mode),
            Some(SnapshotMode::Full)
        );
        assert_eq!(
            envelope.scope.as_ref().map(|scope| scope.viewport_biased),
            Some(false)
        );
        assert_eq!(
            envelope
                .scope
                .as_ref()
                .and_then(|scope| scope.viewport.clone()),
            Some(ViewportMetrics {
                width: 800.0,
                height: 600.0,
                device_pixel_ratio: 2.0,
            })
        );
        assert_eq!(
            envelope
                .scope
                .as_ref()
                .and_then(|scope| scope.locality_fallback_reason.as_deref()),
            None
        );
        assert_eq!(envelope.global_interactive_count, Some(2));
        assert_eq!(envelope.nodes.len(), 2);
        let snapshot = envelope.snapshot.expect("full snapshot should render");
        assert!(snapshot.contains("Offscreen save"));
        assert!(snapshot.contains("Visible tab"));
    }

    #[test]
    fn build_document_envelope_delta_mode_falls_back_to_viewport_without_base() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let dom = viewport_dom();
        let mut context = ToolContext::with_dom(&session, dom);

        let envelope = build_document_envelope(
            &mut context,
            None,
            DocumentEnvelopeOptions::snapshot(SnapshotMode::Delta),
        )
        .expect("delta envelope should build");

        let scope = envelope.scope.expect("delta scope should be present");
        assert_eq!(scope.mode, SnapshotMode::Delta);
        assert_eq!(scope.fallback_mode, Some(SnapshotMode::Viewport));
        assert_eq!(scope.locality_fallback_reason.as_deref(), None);
        assert_eq!(scope.returned_node_count, envelope.nodes.len());
        assert_eq!(envelope.nodes.len(), 1);
        assert_eq!(envelope.nodes[0].name, "Visible tab");
    }

    #[test]
    fn build_document_envelope_delta_mode_uses_same_document_prior_revision_base() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let dom = viewport_dom();
        session
            .store_snapshot_cache(Arc::new(SnapshotCacheEntry {
                document: DocumentMetadata {
                    document_id: "doc-viewport".to_string(),
                    revision: "main:3".to_string(),
                    url: "https://viewport.example".to_string(),
                    title: "Viewport".to_string(),
                    ready_state: "complete".to_string(),
                    frames: Vec::new(),
                },
                snapshot: Arc::<str>::from("- heading \"Visible story\""),
                nodes: Arc::<[SnapshotNode]>::from(Vec::new()),
                scope: SnapshotCacheScope {
                    mode: "viewport".to_string(),
                    fallback_mode: None,
                    viewport_biased: true,
                    returned_node_count: 0,
                    unavailable_frame_count: 0,
                    global_interactive_count: Some(2),
                },
            }))
            .expect("snapshot cache should seed");
        let mut context = ToolContext::with_dom(&session, dom);

        let envelope = build_document_envelope(
            &mut context,
            None,
            DocumentEnvelopeOptions::snapshot(SnapshotMode::Delta),
        )
        .expect("delta envelope should build from prior base");

        let scope = envelope.scope.expect("delta scope should be present");
        assert_eq!(scope.mode, SnapshotMode::Delta);
        assert_eq!(scope.fallback_mode, None);
        assert_eq!(scope.locality_fallback_reason.as_deref(), None);
        assert_eq!(envelope.nodes.len(), 1);
        assert_eq!(envelope.nodes[0].name, "Visible tab");
        let snapshot = envelope.snapshot.expect("delta snapshot should render");
        assert!(snapshot.contains("Visible tab"));
    }

    #[test]
    fn build_document_envelope_viewport_mode_demotes_persistent_chrome_when_local_anchors_exist() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let dom = persistent_chrome_dom();
        let mut context = ToolContext::with_dom(&session, dom);

        let envelope = build_document_envelope(
            &mut context,
            None,
            DocumentEnvelopeOptions::snapshot(SnapshotMode::Viewport),
        )
        .expect("viewport envelope should build");

        let scope = envelope.scope.expect("viewport scope should be present");
        assert_eq!(scope.mode, SnapshotMode::Viewport);
        assert_eq!(scope.locality_fallback_reason.as_deref(), None);
        assert_eq!(envelope.nodes.len(), 1);
        assert_eq!(envelope.nodes[0].name, "Local action");
        let snapshot = envelope.snapshot.expect("viewport snapshot should render");
        assert!(snapshot.contains("Local section"));
        assert!(snapshot.contains("Local action"));
        assert!(!snapshot.contains("Header action"));
    }

    #[test]
    fn build_document_envelope_viewport_mode_reports_persistent_chrome_only_fallback() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let dom = persistent_chrome_only_dom();
        let mut context = ToolContext::with_dom(&session, dom);

        let envelope = build_document_envelope(
            &mut context,
            None,
            DocumentEnvelopeOptions::snapshot(SnapshotMode::Viewport),
        )
        .expect("viewport envelope should build");

        let scope = envelope.scope.expect("viewport scope should be present");
        assert_eq!(scope.mode, SnapshotMode::Viewport);
        assert_eq!(
            scope.locality_fallback_reason.as_deref(),
            Some("persistent_chrome_only")
        );
        assert_eq!(envelope.nodes.len(), 1);
        assert_eq!(envelope.nodes[0].name, "Header action");
        let snapshot = envelope.snapshot.expect("viewport snapshot should render");
        assert!(snapshot.contains("Header action"));
        assert!(!snapshot.contains("Hidden content"));
    }

    #[test]
    fn build_document_envelope_delta_mode_keeps_local_changes_ahead_of_persistent_chrome() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let base_dom = persistent_delta_dom(false);
        let base_projection = snapshot_projection(
            &base_dom,
            SnapshotMode::Viewport,
            Some(base_dom.count_interactive()),
        );
        session
            .store_snapshot_cache(Arc::new(snapshot_cache_entry_from_projection(
                &base_dom.document,
                &base_projection,
            )))
            .expect("snapshot cache should seed");

        let current_dom = persistent_delta_dom(true);
        let mut context = ToolContext::with_dom(&session, current_dom);

        let envelope = build_document_envelope(
            &mut context,
            None,
            DocumentEnvelopeOptions::snapshot(SnapshotMode::Delta),
        )
        .expect("delta envelope should build");

        let scope = envelope.scope.expect("delta scope should be present");
        assert_eq!(scope.mode, SnapshotMode::Delta);
        assert_eq!(scope.fallback_mode, None);
        assert_eq!(scope.locality_fallback_reason.as_deref(), None);
        let node_names = envelope
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>();
        assert!(node_names.contains(&"Details"));
        assert!(node_names.contains(&"Show details"));
        assert!(!node_names.contains(&"Header action"));
        let snapshot = envelope.snapshot.expect("delta snapshot should render");
        assert!(snapshot.contains("Details"));
        assert!(!snapshot.contains("Header action"));
    }
}
