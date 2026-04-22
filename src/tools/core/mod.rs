use crate::browser::BrowserSession;
use crate::dom::{Cursor, DocumentMetadata, DomTree, NodeRef, SnapshotNode};
use crate::error::BrowserError;
use crate::error::Result;
use crate::tools::snapshot::{RenderMode, render_aria_tree};
use crate::tools::{
    click, close, close_tab, evaluate, extract, go_back, go_forward, hover, input, inspect_node,
    markdown, navigate, new_tab, press_key, read_links, screenshot, scroll, select, snapshot,
    switch_tab, tab_list, wait,
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
    pub output_bytes: usize,
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
            && self.output_bytes == 0
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
        let mut metrics = self.metrics.clone();
        metrics.output_bytes = result
            .data
            .as_ref()
            .and_then(|data| serde_json::to_vec(data).ok())
            .map_or(0, |bytes| bytes.len());

        if !metrics.is_empty() {
            result.metadata.insert(
                OPERATION_METRICS_METADATA_KEY.to_string(),
                serde_json::to_value(metrics).unwrap_or_default(),
            );
        }

        result
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DocumentEnvelope {
    pub document: DocumentMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<SnapshotNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interactive_count: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DocumentEnvelopeOptions {
    pub include_snapshot: bool,
    pub include_nodes: bool,
}

impl DocumentEnvelopeOptions {
    pub const fn minimal() -> Self {
        Self {
            include_snapshot: false,
            include_nodes: false,
        }
    }

    pub const fn full() -> Self {
        Self {
            include_snapshot: true,
            include_nodes: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetEnvelope {
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
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
        TargetEnvelope {
            method: self.method.clone(),
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

fn invalid_target_failure(message: impl Into<String>) -> ToolResult {
    let message = message.into();
    ToolResult::failure_with(
        message.clone(),
        serde_json::json!({
            "code": "invalid_target",
            "error": message,
        }),
    )
}

fn stale_node_ref_failure(provided: &NodeRef, current: &DocumentMetadata) -> ToolResult {
    ToolResult::failure_with(
        "Stale node reference",
        serde_json::json!({
            "code": "stale_node_ref",
            "error": "Stale node reference",
            "provided": provided,
            "current_document": {
                "document_id": current.document_id,
                "revision": current.revision,
            }
        }),
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
                return Ok(TargetResolution::Failure(stale_node_ref_failure(
                    &cursor_input.node_ref,
                    &dom.document,
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

/// Result of tool execution
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResult {
    /// Whether the tool execution was successful
    pub success: bool,

    /// Result data (JSON value)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,

    /// Error message if execution failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Additional metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
}

impl ToolResult {
    /// Create a successful result
    pub fn success(data: Option<Value>) -> Self {
        Self {
            success: true,
            data,
            error: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a successful result with data
    pub fn success_with<T: serde::Serialize>(data: T) -> Self {
        Self {
            success: true,
            data: serde_json::to_value(data).ok(),
            error: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a failure result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error.into()),
            metadata: HashMap::new(),
        }
    }

    /// Create a failure result with structured error data.
    pub fn failure_with<T: serde::Serialize>(error: impl Into<String>, data: T) -> Self {
        Self {
            success: false,
            data: serde_json::to_value(data).ok(),
            error: Some(error.into()),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the result
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

fn structured_failure(code: &str, error: String) -> ToolResult {
    ToolResult::failure_with(
        error.clone(),
        serde_json::json!({
            "code": code,
            "error": error,
        }),
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
        BrowserError::ToolExecutionFailed { tool, reason } => Ok(ToolResult::failure_with(
            reason.clone(),
            serde_json::json!({
                "code": "tool_execution_failed",
                "error": reason,
                "tool": tool,
            }),
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

pub(crate) fn build_document_envelope(
    context: &mut ToolContext,
    target: Option<&ResolvedTarget>,
    options: DocumentEnvelopeOptions,
) -> Result<DocumentEnvelope> {
    let target = target.map(|resolved| resolved.to_target_envelope());

    if options.include_snapshot || options.include_nodes {
        let (document, snapshot, nodes, interactive_count) = {
            let dom = context.get_dom()?;
            let snapshot = if options.include_snapshot {
                let started = Instant::now();
                let rendered = render_aria_tree(&dom.root, RenderMode::Ai, None);
                Some((rendered, duration_micros(started.elapsed())))
            } else {
                None
            };

            (
                dom.document.clone(),
                snapshot,
                options
                    .include_nodes
                    .then(|| dom.snapshot_nodes())
                    .unwrap_or_default(),
                options.include_nodes.then(|| dom.count_interactive()),
            )
        };

        if let Some((_, micros)) = snapshot.as_ref() {
            context.record_snapshot_render_micros(*micros);
        }

        return Ok(DocumentEnvelope {
            document,
            target,
            snapshot: snapshot.map(|(snapshot, _)| snapshot),
            nodes,
            interactive_count,
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
        interactive_count: None,
    })
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
        self.register(close::CloseTool);
    }

    /// Register advanced operator tools such as raw JavaScript evaluation
    /// and filesystem-bound screenshot capture.
    pub fn register_operator_tools(&mut self) {
        self.register(screenshot::ScreenshotTool);
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
