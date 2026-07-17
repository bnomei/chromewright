//! Semantic JSON, outline, component, and debug projections.

use crate::semantic::component::{SemanticAttrs, SemanticComponent, SemanticKind};
use crate::semantic::document::SemanticDocument;
use crate::semantic::identity::SemanticRef;
use crate::semantic::render::bounds::{
    MAX_RENDER_OUTPUT_CHARS, RenderError, RenderedOutput, bound_json_output, truncate_output,
};
use serde::Serialize;

fn kind_name(kind: SemanticKind) -> &'static str {
    match kind {
        SemanticKind::Landmark => "landmark",
        SemanticKind::Heading => "heading",
        SemanticKind::Text => "text",
        SemanticKind::List => "list",
        SemanticKind::ListItem => "list_item",
        SemanticKind::Link => "link",
        SemanticKind::Image => "image",
        SemanticKind::Input => "input",
        SemanticKind::Textarea => "textarea",
        SemanticKind::Select => "select",
        SemanticKind::Button => "button",
        SemanticKind::Group => "group",
    }
}

/// Full-document semantic JSON: metadata, revision, and component tree with refs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SemanticJsonProjection {
    pub document_id: String,
    pub revision: String,
    pub url: String,
    pub title: String,
    pub ready_state: String,
    pub component_count: usize,
    pub roots: Vec<SemanticComponent>,
}

/// Compact outline entry for landmarks, headings, and lists.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OutlineEntry {
    pub depth: usize,
    pub kind: String,
    pub semantic_ref: SemanticRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landmark: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading_level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    pub focusable: bool,
}

/// Outline projection over a document.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OutlineProjection {
    pub document_id: String,
    pub revision: String,
    pub url: String,
    pub title: String,
    pub entries: Vec<OutlineEntry>,
}

/// Single-component projection (exact ref) including nested children.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ComponentProjection {
    pub document_id: String,
    pub revision: String,
    pub semantic_ref: SemanticRef,
    pub component: SemanticComponent,
}

/// One debug node with safe model metadata (kind, tag, attrs, depth, ref).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DebugNode {
    pub depth: usize,
    pub kind: String,
    pub semantic_ref: SemanticRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "SemanticAttrs::is_empty")]
    pub attrs: SemanticAttrs,
    pub focusable: bool,
    pub child_count: usize,
}

/// Bounded debug projection of the full tree in document order.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DebugProjection {
    pub document_id: String,
    pub revision: String,
    pub url: String,
    pub title: String,
    pub ready_state: String,
    pub component_count: usize,
    pub nodes: Vec<DebugNode>,
}

/// Serialize the semantic tree as JSON with document metadata and opaque refs.
pub fn render_semantic_json(document: &SemanticDocument) -> Result<RenderedOutput, RenderError> {
    render_semantic_json_with_limit(document, MAX_RENDER_OUTPUT_CHARS)
}

/// Compact outline of landmarks, headings, lists, and focusable controls.
pub fn render_outline(document: &SemanticDocument) -> Result<RenderedOutput, RenderError> {
    render_outline_with_limit(document, MAX_RENDER_OUTPUT_CHARS)
}

/// JSON projection for one exact component reference.
pub fn render_component_json(
    document: &SemanticDocument,
    semantic_ref: &SemanticRef,
) -> Result<RenderedOutput, RenderError> {
    render_component_json_with_limit(document, semantic_ref, MAX_RENDER_OUTPUT_CHARS)
}

/// Debug projection with kind, tag, attrs, depth, and opaque refs (no HTML).
pub fn render_debug(document: &SemanticDocument) -> Result<RenderedOutput, RenderError> {
    render_debug_with_limit(document, MAX_RENDER_OUTPUT_CHARS)
}

/// Test/helper entry: semantic JSON with an explicit character budget.
///
/// On success the content is always valid JSON and never truncated mid-document.
pub(crate) fn render_semantic_json_with_limit(
    document: &SemanticDocument,
    limit: usize,
) -> Result<RenderedOutput, RenderError> {
    let projection = SemanticJsonProjection {
        document_id: document.document.document_id.clone(),
        revision: document.document.revision.clone(),
        url: document.document.url.clone(),
        title: document.document.title.clone(),
        ready_state: document.document.ready_state.clone(),
        component_count: document.component_count(),
        roots: document.roots.clone(),
    };
    serialize_json_projection(&projection, document, limit)
}

/// Test/helper entry: component JSON with an explicit character budget.
pub(crate) fn render_component_json_with_limit(
    document: &SemanticDocument,
    semantic_ref: &SemanticRef,
    limit: usize,
) -> Result<RenderedOutput, RenderError> {
    let component = document.resolve(semantic_ref)?;
    let projection = ComponentProjection {
        document_id: document.document.document_id.clone(),
        revision: document.document.revision.clone(),
        semantic_ref: semantic_ref.clone(),
        component: component.clone(),
    };
    serialize_json_projection(&projection, document, limit)
}

/// Test/helper entry: debug JSON with an explicit character budget.
pub(crate) fn render_debug_with_limit(
    document: &SemanticDocument,
    limit: usize,
) -> Result<RenderedOutput, RenderError> {
    let mut nodes = Vec::new();
    for root in &document.roots {
        collect_debug(root, 0, &mut nodes);
    }
    let projection = DebugProjection {
        document_id: document.document.document_id.clone(),
        revision: document.document.revision.clone(),
        url: document.document.url.clone(),
        title: document.document.title.clone(),
        ready_state: document.document.ready_state.clone(),
        component_count: document.component_count(),
        nodes,
    };
    serialize_json_projection(&projection, document, limit)
}

/// Test/helper entry: outline with an explicit character budget.
///
/// The embedded JSON fence is never mid-truncated: the JSON is validated as a
/// complete document first, then human text is truncated if needed so the
/// trailing ```json block remains intact and parseable.
pub(crate) fn render_outline_with_limit(
    document: &SemanticDocument,
    limit: usize,
) -> Result<RenderedOutput, RenderError> {
    let mut entries = Vec::new();
    for root in &document.roots {
        collect_outline(root, 0, &mut entries);
    }
    let projection = OutlineProjection {
        document_id: document.document.document_id.clone(),
        revision: document.document.revision.clone(),
        url: document.document.url.clone(),
        title: document.document.title.clone(),
        entries,
    };

    let json = serde_json::to_string_pretty(&projection).map_err(|_| RenderError::OutputLimit {
        limit,
        produced_chars: 0,
    })?;
    let json_chars = json.chars().count();
    // Fence overhead: "\n```json\n" + "\n```\n"
    let fence_overhead = "\n```json\n".chars().count() + "\n```\n".chars().count();
    let fence_total = json_chars.saturating_add(fence_overhead);
    if fence_total > limit {
        return Err(RenderError::OutputLimit {
            limit,
            produced_chars: fence_total,
        });
    }

    let mut human = String::new();
    human.push_str(&format!(
        "# Outline: {}\n\n",
        projection.title.replace('\n', " ")
    ));
    human.push_str(&format!(
        "document_id: {}  \nrevision: {}  \nurl: {}\n\n",
        projection.document_id, projection.revision, projection.url
    ));
    for entry in &projection.entries {
        let indent = "  ".repeat(entry.depth);
        let label = entry
            .label
            .as_deref()
            .or(entry.text.as_deref())
            .unwrap_or("");
        let role = entry
            .landmark
            .as_deref()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default();
        let href = entry
            .href
            .as_deref()
            .map(|h| format!(" → {h}"))
            .unwrap_or_default();
        human.push_str(&format!(
            "{indent}- [{}]{} {}{} `{}`\n",
            entry.kind,
            role,
            label,
            href,
            entry.semantic_ref.as_str()
        ));
    }

    let human_budget = limit.saturating_sub(fence_total);
    let (human, truncated) = truncate_output(human, human_budget);
    let mut content = human;
    content.push_str("\n```json\n");
    content.push_str(&json);
    content.push_str("\n```\n");

    // Final guard: total must fit; JSON body remains complete.
    let total_chars = content.chars().count();
    if total_chars > limit {
        return Err(RenderError::OutputLimit {
            limit,
            produced_chars: total_chars,
        });
    }

    Ok(RenderedOutput::new(
        content,
        truncated,
        document.document.document_id.clone(),
        document.document.revision.clone(),
    ))
}

fn serialize_json_projection<T: Serialize>(
    projection: &T,
    document: &SemanticDocument,
    limit: usize,
) -> Result<RenderedOutput, RenderError> {
    let content =
        serde_json::to_string_pretty(projection).map_err(|_| RenderError::OutputLimit {
            limit,
            produced_chars: 0,
        })?;
    bound_json_output(
        content,
        &document.document.document_id,
        &document.document.revision,
        limit,
    )
}

fn collect_outline(component: &SemanticComponent, depth: usize, entries: &mut Vec<OutlineEntry>) {
    let include = matches!(
        component.kind,
        SemanticKind::Landmark
            | SemanticKind::Heading
            | SemanticKind::List
            | SemanticKind::Link
            | SemanticKind::Input
            | SemanticKind::Textarea
            | SemanticKind::Select
            | SemanticKind::Button
            | SemanticKind::Group
            | SemanticKind::Image
    );
    if include {
        entries.push(OutlineEntry {
            depth,
            kind: kind_name(component.kind).to_string(),
            semantic_ref: component.semantic_ref.clone(),
            landmark: component.attrs.landmark.map(|role| match role {
                crate::semantic::component::LandmarkRole::Main => "main".into(),
                crate::semantic::component::LandmarkRole::Aside => "aside".into(),
                crate::semantic::component::LandmarkRole::Header => "header".into(),
                crate::semantic::component::LandmarkRole::Nav => "nav".into(),
                crate::semantic::component::LandmarkRole::Section => "section".into(),
                crate::semantic::component::LandmarkRole::Footer => "footer".into(),
            }),
            heading_level: component.attrs.heading_level,
            label: component.label.clone(),
            text: component.text.clone(),
            href: component.attrs.href.clone(),
            focusable: component.is_focusable(),
        });
    }
    for child in &component.children {
        collect_outline(child, depth + 1, entries);
    }
}

fn collect_debug(component: &SemanticComponent, depth: usize, nodes: &mut Vec<DebugNode>) {
    let attrs = component.attrs.clone();
    nodes.push(DebugNode {
        depth,
        kind: kind_name(component.kind).to_string(),
        semantic_ref: component.semantic_ref.clone(),
        label: component.label.clone(),
        text: component.text.clone(),
        tag: attrs.tag.clone(),
        attrs,
        focusable: component.is_focusable(),
        child_count: component.children.len(),
    });
    for child in &component.children {
        collect_debug(child, depth + 1, nodes);
    }
}
