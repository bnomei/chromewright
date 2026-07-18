//! Normalize browser-side semantic capture payloads into `SemanticDocument`.

use crate::dom::DocumentMetadata;
use crate::error::{BrowserError, Result};
use crate::semantic::component::{
    LandmarkRole, SelectOption, SemanticAttrs, SemanticComponent, SemanticKind,
};
use crate::semantic::document::SemanticDocument;
use crate::semantic::identity::{
    SemanticIdentity, SemanticRef, SemanticRefPayload, document_scope_from_url,
    fingerprint_identity,
};
use crate::semantic::limits::{
    MAX_SEMANTIC_DEPTH, MAX_SEMANTIC_SELECT_OPTIONS, validate_semantic_string,
};
use serde::Deserialize;
use std::collections::HashSet;

/// Browser-side extraction response decoded from `extract_semantic_dom.js`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SemanticCaptureResponse {
    pub document: DocumentMetadata,
    #[serde(default)]
    pub nodes: Vec<RawSemanticNode>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub error: Option<String>,
}

/// Intermediate semantic node emitted by the capture script or fixtures.
#[derive(Debug, Clone, Deserialize)]
pub struct RawSemanticNode {
    pub kind: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub unique_id: bool,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub landmark: Option<String>,
    #[serde(default)]
    pub heading_level: Option<u8>,
    #[serde(default)]
    pub ordered: Option<bool>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub href: Option<String>,
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub alt: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub input_type: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub checked: Option<bool>,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub readonly: Option<bool>,
    #[serde(default)]
    pub multiple: Option<bool>,
    #[serde(default)]
    pub button_type: Option<String>,
    #[serde(default)]
    pub options: Vec<RawSelectOption>,
    #[serde(default)]
    pub children: Vec<RawSemanticNode>,
}

/// Intermediate `<select>` option from capture or fixtures before budget clipping.
///
/// Normalized into [`SelectOption`] with per-field string budgets and
/// [`MAX_SEMANTIC_SELECT_OPTIONS`](crate::semantic::MAX_SEMANTIC_SELECT_OPTIONS).
#[derive(Debug, Clone, Deserialize)]
pub struct RawSelectOption {
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub disabled: bool,
}

/// Normalize a capture response into a bounded, identity-indexed document.
pub fn normalize_capture(response: SemanticCaptureResponse) -> Result<SemanticDocument> {
    if let Some(error) = response.error
        && !error.is_empty()
    {
        return Err(BrowserError::DomParseFailed(format!(
            "semantic capture failed: {error}"
        )));
    }

    if response.truncated {
        return Err(BrowserError::resource_limit_exceeded(
            "semantic_capture",
            "semantic capture exceeded internal size bounds and was truncated",
            "bounded complete capture",
            "truncated capture",
        ));
    }

    let document = response.document;
    let scope = document_scope_from_url(&document.url);
    let mut used_identities = HashSet::new();
    let mut ancestry = Vec::new();
    // One sibling-counter frame for root-level components.
    let mut kind_counters = vec![std::collections::HashMap::new()];

    let roots = normalize_nodes(
        &response.nodes,
        &document,
        &scope,
        &mut ancestry,
        &mut kind_counters,
        &mut used_identities,
        1,
    )?;

    SemanticDocument::from_components(document, roots)
}

/// Build a document from fixture-style raw nodes and explicit metadata.
pub fn normalize_fixture(
    document: DocumentMetadata,
    nodes: Vec<RawSemanticNode>,
) -> Result<SemanticDocument> {
    normalize_capture(SemanticCaptureResponse {
        document,
        nodes,
        truncated: false,
        error: None,
    })
}

fn normalize_nodes(
    nodes: &[RawSemanticNode],
    document: &DocumentMetadata,
    scope: &str,
    ancestry: &mut Vec<String>,
    kind_counters: &mut Vec<std::collections::HashMap<String, usize>>,
    used_identities: &mut HashSet<SemanticIdentity>,
    depth: usize,
) -> Result<Vec<SemanticComponent>> {
    if depth > MAX_SEMANTIC_DEPTH {
        return Err(BrowserError::resource_limit_exceeded(
            "semantic_depth",
            format!("semantic document depth exceeds {MAX_SEMANTIC_DEPTH}"),
            format!("{MAX_SEMANTIC_DEPTH}"),
            format!("{depth}"),
        ));
    }

    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        if let Some(component) = normalize_node(
            node,
            document,
            scope,
            ancestry,
            kind_counters,
            used_identities,
            depth,
        )? {
            out.push(component);
        }
    }
    Ok(out)
}

fn normalize_node(
    node: &RawSemanticNode,
    document: &DocumentMetadata,
    scope: &str,
    ancestry: &mut Vec<String>,
    kind_counters: &mut Vec<std::collections::HashMap<String, usize>>,
    used_identities: &mut HashSet<SemanticIdentity>,
    depth: usize,
) -> Result<Option<SemanticComponent>> {
    let kind = parse_kind(&node.kind)?;
    let signature = local_signature(node, kind);
    let sibling_key = sibling_counter_key(kind, node);
    let sibling_index = next_sibling_index(kind_counters, &sibling_key);
    let step = format!("{sibling_key}[{sibling_index}]");

    let identity = choose_identity(node, scope, ancestry, &signature, used_identities);
    used_identities.insert(identity.clone());

    let semantic_ref = SemanticRef::encode(&SemanticRefPayload {
        document_id: document.document_id.clone(),
        revision: document.revision.clone(),
        identity,
    });

    ancestry.push(step);
    kind_counters.push(std::collections::HashMap::new());
    let children = normalize_nodes(
        &node.children,
        document,
        scope,
        ancestry,
        kind_counters,
        used_identities,
        depth + 1,
    )?;
    kind_counters.pop();
    ancestry.pop();

    let attrs = build_attrs(node, kind)?;
    let mut label = optional_clipped(&node.label, "label")?;
    let mut text = optional_clipped(&node.text, "text")?;

    // Textual containers with nested semantic children must not also keep aggregate
    // innerText; ordered children are authoritative for rendering.
    if !children.is_empty() && is_textual_container(kind) {
        text = None;
        // Headings/list items may still expose a compact label only when leaf-shaped.
        if matches!(kind, SemanticKind::Heading | SemanticKind::ListItem) {
            label = None;
        }
    }

    // Links, buttons, and form controls are terminal leaves.
    let children = if is_terminal_leaf(kind) {
        Vec::new()
    } else {
        children
    };

    Ok(Some(SemanticComponent {
        semantic_ref,
        kind,
        label,
        text,
        attrs,
        interaction_selector: optional_clipped(&node.selector, "selector")?,
        children,
    }))
}

fn is_textual_container(kind: SemanticKind) -> bool {
    matches!(
        kind,
        SemanticKind::Text | SemanticKind::Heading | SemanticKind::ListItem
    )
}

fn is_terminal_leaf(kind: SemanticKind) -> bool {
    matches!(
        kind,
        SemanticKind::Link
            | SemanticKind::Button
            | SemanticKind::Input
            | SemanticKind::Textarea
            | SemanticKind::Select
            | SemanticKind::Image
    )
}

fn choose_identity(
    node: &RawSemanticNode,
    scope: &str,
    ancestry: &[String],
    signature: &str,
    used_identities: &HashSet<SemanticIdentity>,
) -> SemanticIdentity {
    if node.unique_id
        && let Some(id) = node.id.as_deref().filter(|id| !id.is_empty())
    {
        let candidate = SemanticIdentity::author_id(id);
        if !used_identities.contains(&candidate) {
            return candidate;
        }
    }

    // Prefer fingerprint; if collision (rare), salt with ancestry length and retry.
    let mut identity = fingerprint_identity(scope, ancestry, signature);
    if !used_identities.contains(&identity) {
        return identity;
    }

    let mut salt = 0u32;
    loop {
        salt += 1;
        identity = fingerprint_identity(scope, ancestry, &format!("{signature}|salt:{salt}"));
        if !used_identities.contains(&identity) {
            return identity;
        }
        if salt > 10_000 {
            // Deterministic last resort still scoped to document ancestry.
            return fingerprint_identity(
                scope,
                ancestry,
                &format!("{signature}|fallback:{}", used_identities.len()),
            );
        }
    }
}

fn parse_kind(kind: &str) -> Result<SemanticKind> {
    match kind {
        "landmark" => Ok(SemanticKind::Landmark),
        "heading" => Ok(SemanticKind::Heading),
        "text" => Ok(SemanticKind::Text),
        "list" => Ok(SemanticKind::List),
        "list_item" => Ok(SemanticKind::ListItem),
        "link" => Ok(SemanticKind::Link),
        "image" => Ok(SemanticKind::Image),
        "input" => Ok(SemanticKind::Input),
        "textarea" => Ok(SemanticKind::Textarea),
        "select" => Ok(SemanticKind::Select),
        "button" => Ok(SemanticKind::Button),
        "group" => Ok(SemanticKind::Group),
        other => Err(BrowserError::DomParseFailed(format!(
            "unknown semantic kind: {other}"
        ))),
    }
}

fn parse_landmark(value: Option<&str>) -> Result<Option<LandmarkRole>> {
    match value {
        None | Some("") => Ok(None),
        Some("main") => Ok(Some(LandmarkRole::Main)),
        Some("aside") => Ok(Some(LandmarkRole::Aside)),
        Some("header") => Ok(Some(LandmarkRole::Header)),
        Some("nav") => Ok(Some(LandmarkRole::Nav)),
        Some("section") => Ok(Some(LandmarkRole::Section)),
        Some("footer") => Ok(Some(LandmarkRole::Footer)),
        Some(other) => Err(BrowserError::DomParseFailed(format!(
            "unknown landmark role: {other}"
        ))),
    }
}

fn build_attrs(node: &RawSemanticNode, kind: SemanticKind) -> Result<SemanticAttrs> {
    let mut attrs = SemanticAttrs {
        landmark: parse_landmark(node.landmark.as_deref())?,
        heading_level: node.heading_level,
        ordered: node.ordered,
        href: optional_clipped(&node.href, "href")?,
        src: optional_clipped(&node.src, "src")?,
        alt: optional_clipped(&node.alt, "alt")?,
        name: optional_clipped(&node.name, "name")?,
        value: optional_clipped(&node.value, "value")?,
        input_type: optional_clipped(&node.input_type, "input_type")?,
        placeholder: optional_clipped(&node.placeholder, "placeholder")?,
        checked: node.checked,
        disabled: node.disabled,
        required: node.required,
        readonly: node.readonly,
        multiple: node.multiple,
        button_type: optional_clipped(&node.button_type, "button_type")?,
        options: Vec::new(),
        tag: optional_clipped(&node.tag, "tag")?,
    };

    if kind == SemanticKind::Select {
        let mut options = Vec::new();
        for (index, option) in node.options.iter().enumerate() {
            if index >= MAX_SEMANTIC_SELECT_OPTIONS {
                return Err(BrowserError::resource_limit_exceeded(
                    "semantic_select_options",
                    format!("select has more than {MAX_SEMANTIC_SELECT_OPTIONS} options"),
                    format!("{MAX_SEMANTIC_SELECT_OPTIONS}"),
                    format!("{}", node.options.len()),
                ));
            }
            validate_semantic_string("option.value", &option.value)?;
            if let Some(label) = &option.label {
                validate_semantic_string("option.label", label)?;
            }
            options.push(SelectOption {
                value: option.value.clone(),
                label: option.label.clone(),
                selected: option.selected,
                disabled: option.disabled,
            });
        }
        attrs.options = options;
    }

    Ok(attrs)
}

fn optional_clipped(value: &Option<String>, field: &str) -> Result<Option<String>> {
    match value {
        None => Ok(None),
        Some(text) if text.is_empty() => Ok(None),
        Some(text) => {
            validate_semantic_string(field, text)?;
            Ok(Some(text.clone()))
        }
    }
}

fn local_signature(node: &RawSemanticNode, kind: SemanticKind) -> String {
    let mut parts = vec![kind_token(kind).to_string()];
    if let Some(tag) = &node.tag {
        parts.push(format!("tag:{tag}"));
    }
    if let Some(landmark) = &node.landmark {
        parts.push(format!("lm:{landmark}"));
    }
    if let Some(level) = node.heading_level {
        parts.push(format!("h:{level}"));
    }
    if let Some(ordered) = node.ordered {
        parts.push(format!("ol:{ordered}"));
    }
    if let Some(href) = &node.href {
        parts.push(format!("href:{href}"));
    }
    if let Some(name) = &node.name {
        parts.push(format!("name:{name}"));
    }
    if let Some(input_type) = &node.input_type {
        parts.push(format!("type:{input_type}"));
    }
    if let Some(src) = &node.src {
        parts.push(format!("src:{src}"));
    }
    // Text is intentionally excluded from identity so wording edits do not retarget
    // fingerprints when ancestry and structural signature still match. Author ids
    // remain preferred when unique.
    parts.join("|")
}

fn kind_token(kind: SemanticKind) -> &'static str {
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

fn sibling_counter_key(kind: SemanticKind, node: &RawSemanticNode) -> String {
    match kind {
        SemanticKind::Landmark => {
            format!("landmark:{}", node.landmark.as_deref().unwrap_or("unknown"))
        }
        SemanticKind::Heading => format!("heading:{}", node.heading_level.unwrap_or(0)),
        SemanticKind::Link => format!("link:{}", node.href.as_deref().unwrap_or("")),
        SemanticKind::Input => format!(
            "input:{}:{}",
            node.input_type.as_deref().unwrap_or("text"),
            node.name.as_deref().unwrap_or("")
        ),
        other => kind_token(other).to_string(),
    }
}

fn next_sibling_index(
    kind_counters: &mut [std::collections::HashMap<String, usize>],
    key: &str,
) -> usize {
    let counters = kind_counters
        .last_mut()
        .expect("kind counter frame must exist for current ancestry depth");
    let entry = counters.entry(key.to_string()).or_insert(0);
    let index = *entry;
    *entry += 1;
    index
}
