//! Browser automation tools module
//!
//! This module provides a framework for browser automation tools and
//! includes implementations of common browser operations.

pub mod click;
pub mod close;
pub mod close_tab;
pub mod evaluate;
pub mod extract;
pub mod go_back;
pub mod go_forward;
pub mod hover;
pub mod html_to_markdown;
pub mod input;
pub mod markdown;
pub mod navigate;
pub mod new_tab;
pub mod press_key;
pub mod read_links;
pub mod readability_script;
pub mod screenshot;
pub mod scroll;
pub mod select;
pub mod snapshot;
pub mod switch_tab;
pub mod tab_list;
mod utils;
pub mod wait;

// Re-export Params types for use by MCP layer
pub use click::ClickParams;
pub use close::CloseParams;
pub use close_tab::CloseTabParams;
pub use evaluate::EvaluateParams;
pub use extract::ExtractParams;
pub use go_back::GoBackParams;
pub use go_forward::GoForwardParams;
pub use hover::HoverParams;
pub use input::InputParams;
pub use markdown::GetMarkdownParams;
pub use navigate::NavigateParams;
pub use new_tab::NewTabParams;
pub use press_key::PressKeyParams;
pub use read_links::ReadLinksParams;
pub use screenshot::ScreenshotParams;
pub use scroll::ScrollParams;
pub use select::SelectParams;
pub use snapshot::SnapshotParams;
pub use switch_tab::SwitchTabParams;
pub use tab_list::TabListParams;
pub use wait::WaitParams;
pub use wait::WaitCondition;

use crate::browser::BrowserSession;
use crate::dom::{DocumentMetadata, DomTree, NodeRef, SnapshotNode};
use crate::error::BrowserError;
use crate::error::Result;
use crate::tools::snapshot::{RenderMode, render_aria_tree};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Tool execution context
pub struct ToolContext<'a> {
    /// Browser session
    pub session: &'a BrowserSession,

    /// Optional DOM tree (extracted on demand)
    pub dom_tree: Option<DomTree>,
}

impl<'a> ToolContext<'a> {
    /// Create a new tool context
    pub fn new(session: &'a BrowserSession) -> Self {
        Self {
            session,
            dom_tree: None,
        }
    }

    /// Create a context with a pre-extracted DOM tree
    pub fn with_dom(session: &'a BrowserSession, dom_tree: DomTree) -> Self {
        Self {
            session,
            dom_tree: Some(dom_tree),
        }
    }

    /// Invalidate the cached DOM tree after a mutation.
    pub fn invalidate_dom(&mut self) {
        self.dom_tree = None;
    }

    /// Get or extract the DOM tree
    pub fn get_dom(&mut self) -> Result<&DomTree> {
        if self.dom_tree.is_none() {
            self.dom_tree = Some(self.session.extract_dom()?);
        }
        Ok(self.dom_tree.as_ref().unwrap())
    }

    /// Refresh and return the latest DOM tree after a document mutation.
    pub fn refresh_dom(&mut self) -> Result<&DomTree> {
        self.invalidate_dom();
        self.get_dom()
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
    pub interactive_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetEnvelope {
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedTarget {
    pub selector: String,
    pub index: Option<usize>,
    pub node_ref: Option<NodeRef>,
}

impl ResolvedTarget {
    pub fn method(&self) -> &'static str {
        if self.node_ref.is_some() {
            "node_ref"
        } else if self.index.is_some() {
            "index"
        } else {
            "css"
        }
    }

    pub fn to_target_envelope(&self) -> TargetEnvelope {
        TargetEnvelope {
            method: self.method().to_string(),
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

pub(crate) fn resolve_target(
    tool: &str,
    selector: Option<String>,
    index: Option<usize>,
    node_ref: Option<NodeRef>,
    dom: Option<&DomTree>,
) -> Result<TargetResolution> {
    let target_count = usize::from(selector.is_some())
        + usize::from(index.is_some())
        + usize::from(node_ref.is_some());

    if target_count > 1 {
        return Ok(TargetResolution::Failure(invalid_target_failure(
            "Cannot specify more than one of 'selector', 'index', or 'node_ref'.",
        )));
    }

    if target_count == 0 {
        return Ok(TargetResolution::Failure(invalid_target_failure(
            "Must specify one of 'selector', 'index', or 'node_ref'.",
        )));
    }

    match (selector, index, node_ref) {
        (Some(selector), None, None) => Ok(TargetResolution::Resolved(ResolvedTarget {
            selector,
            index: None,
            node_ref: None,
        })),
        (None, Some(index), None) => {
            let dom = dom.ok_or_else(|| BrowserError::ToolExecutionFailed {
                tool: tool.to_string(),
                reason: "DOM tree is required to resolve an element index.".to_string(),
            })?;
            let selector = dom.get_selector(index).ok_or_else(|| {
                BrowserError::ElementNotFound(format!("No element with index {}", index))
            })?;

            Ok(TargetResolution::Resolved(ResolvedTarget {
                selector: selector.clone(),
                index: Some(index),
                node_ref: dom.node_ref_for_index(index),
            }))
        }
        (None, None, Some(node_ref)) => {
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

            let selector = dom.get_selector(node_ref.index).ok_or_else(|| {
                BrowserError::ElementNotFound(format!(
                    "No element with index {} for the provided node reference",
                    node_ref.index
                ))
            })?;

            Ok(TargetResolution::Resolved(ResolvedTarget {
                selector: selector.clone(),
                index: Some(node_ref.index),
                node_ref: Some(node_ref),
            }))
        }
        _ => Err(BrowserError::ToolExecutionFailed {
            tool: tool.to_string(),
            reason: "Failed to resolve target".to_string(),
        }),
    }
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

pub(crate) fn build_document_envelope(
    context: &mut ToolContext,
    target: Option<&ResolvedTarget>,
    include_snapshot: bool,
) -> Result<DocumentEnvelope> {
    let dom = context.get_dom()?;
    Ok(DocumentEnvelope {
        document: dom.document.clone(),
        target: target.map(|resolved| resolved.to_target_envelope()),
        snapshot: include_snapshot.then(|| render_aria_tree(&dom.root, RenderMode::Ai, None)),
        nodes: dom.snapshot_nodes(),
        interactive_count: dom.count_interactive(),
    })
}

/// Trait for browser automation tools with associated parameter types
pub trait Tool: Send + Sync + Default {
    /// Associated parameter type for this tool
    type Params: serde::Serialize + for<'de> serde::Deserialize<'de> + schemars::JsonSchema;

    /// Get tool name
    fn name(&self) -> &str;

    /// Get tool parameter schema (JSON Schema)
    fn parameters_schema(&self) -> Value {
        serde_json::to_value(schemars::schema_for!(Self::Params)).unwrap_or_default()
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
    fn parameters_schema(&self) -> Value;
    fn execute(&self, params: Value, context: &mut ToolContext) -> Result<ToolResult>;
}

/// Blanket implementation to convert any Tool into DynTool
impl<T: Tool> DynTool for T {
    fn name(&self) -> &str {
        Tool::name(self)
    }

    fn parameters_schema(&self) -> Value {
        Tool::parameters_schema(self)
    }

    fn execute(&self, params: Value, context: &mut ToolContext) -> Result<ToolResult> {
        Tool::execute(self, params, context)
    }
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

    /// Create a registry with default tools
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Register navigation tools
        registry.register(navigate::NavigateTool);
        registry.register(go_back::GoBackTool);
        registry.register(go_forward::GoForwardTool);
        registry.register(wait::WaitTool);

        // Register interaction tools
        registry.register(click::ClickTool);
        registry.register(input::InputTool);
        registry.register(select::SelectTool);
        registry.register(hover::HoverTool);
        registry.register(press_key::PressKeyTool);
        registry.register(scroll::ScrollTool);

        // Register tab management tools
        registry.register(new_tab::NewTabTool);
        registry.register(tab_list::TabListTool);
        registry.register(switch_tab::SwitchTabTool);
        registry.register(close_tab::CloseTabTool);

        // Register reading and extraction tools
        registry.register(extract::ExtractContentTool);
        registry.register(markdown::GetMarkdownTool);
        registry.register(read_links::ReadLinksTool);
        registry.register(snapshot::SnapshotTool);

        // Register utility tools
        registry.register(screenshot::ScreenshotTool);
        registry.register(evaluate::EvaluateTool);
        registry.register(close::CloseTool);

        registry
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
        self.tools.keys().cloned().collect()
    }

    /// Get all tools
    pub fn all_tools(&self) -> Vec<Arc<dyn DynTool>> {
        self.tools.values().cloned().collect()
    }

    /// Execute a tool by name
    pub fn execute(
        &self,
        name: &str,
        params: Value,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        match self.get(name) {
            Some(tool) => tool.execute(params, context),
            None => Ok(ToolResult::failure(format!("Tool '{}' not found", name))),
        }
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
    use crate::dom::{AriaChild, AriaNode};

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success(Some(serde_json::json!({"url": "https://example.com"})));
        assert!(result.success);
        assert!(result.data.is_some());
        assert!(result.error.is_none());
    }

    #[test]
    fn test_tool_result_failure() {
        let result = ToolResult::failure("Test error");
        assert!(!result.success);
        assert!(result.data.is_none());
        assert_eq!(result.error, Some("Test error".to_string()));
    }

    #[test]
    fn test_tool_result_with_metadata() {
        let result = ToolResult::success(None).with_metadata("duration_ms", serde_json::json!(100));

        assert!(result.metadata.contains_key("duration_ms"));
    }

    fn sample_dom() -> DomTree {
        let root = AriaNode::fragment().with_child(AriaChild::Node(Box::new(
            AriaNode::new("button", "Submit")
                .with_index(1)
                .with_box(true, Some("pointer".to_string())),
        )));
        let mut dom = DomTree::new(root);
        dom.document.document_id = "doc-1".to_string();
        dom.document.revision = "main:1".to_string();
        dom.selectors = vec![String::new(), "#submit".to_string()];
        dom
    }

    #[test]
    fn test_resolve_target_prefers_css_selector() {
        let target = resolve_target("click", Some("#submit".to_string()), None, None, None)
            .expect("selector target should resolve");

        match target {
            TargetResolution::Resolved(target) => {
                assert_eq!(
                    target,
                    ResolvedTarget {
                        selector: "#submit".to_string(),
                        index: None,
                        node_ref: None,
                    }
                );
                assert_eq!(target.method(), "css");
            }
            TargetResolution::Failure(failure) => panic!("unexpected failure: {:?}", failure),
        }
    }

    #[test]
    fn test_resolve_target_resolves_index_via_dom() {
        let dom = sample_dom();
        let target = resolve_target("click", None, Some(1), None, Some(&dom))
            .expect("index target should resolve against DOM");

        match target {
            TargetResolution::Resolved(target) => {
                assert_eq!(
                    target,
                    ResolvedTarget {
                        selector: "#submit".to_string(),
                        index: Some(1),
                        node_ref: Some(crate::dom::NodeRef {
                            document_id: "doc-1".to_string(),
                            revision: "main:1".to_string(),
                            index: 1,
                        }),
                    }
                );
                assert_eq!(target.method(), "node_ref");
            }
            TargetResolution::Failure(failure) => panic!("unexpected failure: {:?}", failure),
        }
    }

    #[test]
    fn test_resolve_target_rejects_invalid_combinations() {
        let both = resolve_target("click", Some("#submit".to_string()), Some(1), None, None)
            .expect("invalid combination should return tool failure");
        assert!(matches!(both, TargetResolution::Failure(_)));

        let neither = resolve_target("click", None, None, None, None)
            .expect("missing target should return tool failure");
        assert!(matches!(neither, TargetResolution::Failure(_)));
    }

    #[test]
    fn test_resolve_target_errors_for_missing_index() {
        let dom = sample_dom();
        let result = resolve_target("click", None, Some(9), None, Some(&dom));

        assert!(matches!(result, Err(BrowserError::ElementNotFound(_))));
    }

    #[test]
    fn test_resolve_target_rejects_stale_node_ref() {
        let dom = sample_dom();
        let result = resolve_target(
            "click",
            None,
            None,
            Some(crate::dom::NodeRef {
                document_id: "doc-1".to_string(),
                revision: "main:0".to_string(),
                index: 1,
            }),
            Some(&dom),
        )
        .expect("stale node ref should become tool failure");

        assert!(matches!(result, TargetResolution::Failure(_)));
    }
}
