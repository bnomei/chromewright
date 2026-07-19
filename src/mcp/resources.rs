//! Feature-gated, bounded MCP resource catalog for the co-hosted TUI companion.
//!
//! Active and collaboration URIs are dynamic aliases of shared state. Revision-
//! addressed URIs are immutable views of retained captures and never fall through
//! to the active document on miss.

use crate::semantic::{
    SemanticDocument, SemanticRef, render_component_markdown, render_debug, render_outline,
    render_semantic_json, render_semantic_markdown,
};
use crate::tui::{CoordinationError, CoordinationSnapshot, Lifecycle, SharedTuiState};
use rmcp::model::{
    ListResourceTemplatesResult, ListResourcesResult, Meta, ReadResourceResult, Resource,
    ResourceContents, ResourceTemplate,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Default character page size for semantic Markdown resources.
pub const DEFAULT_MARKDOWN_PAGE_CHARS: usize = 32_000;

/// Absolute upper bound for any single resource body.
pub const MAX_RESOURCE_CHARS: usize = 200_000;

const SCHEME: &str = "chromewright://";

/// Parsed `chromewright://` catalog URI for companion semantic resources.
///
/// Active and collaboration aliases resolve against live shared TUI state.
/// Revision-addressed page URIs are immutable retained captures and never fall
/// through to the active document when the requested revision is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceUri {
    /// Dynamic alias of the last complete semantic Markdown capture (paginated).
    ActiveSemanticMd { offset: usize, limit: usize },
    /// Immutable revision-addressed full-document semantic Markdown (paginated).
    PageSemanticMd {
        document_id: String,
        revision: String,
        offset: usize,
        limit: usize,
    },
    /// Immutable outline Markdown for a retained document revision.
    PageOutline {
        document_id: String,
        revision: String,
    },
    /// Immutable semantic JSON for a retained document revision (fail-closed on size).
    PageSemanticJson {
        document_id: String,
        revision: String,
    },
    /// Immutable debug JSON for a retained document revision (fail-closed on size).
    PageDebugJson {
        document_id: String,
        revision: String,
    },
    /// Immutable component Markdown fragment addressed by an opaque `semantic_ref`.
    PageComponent {
        document_id: String,
        revision: String,
        semantic_ref: String,
    },
    /// Read-only human selection collaboration state.
    Selection,
    /// Read-only agent attention collaboration state.
    Attention,
}

/// Fail-closed catalog errors for parse, retention, coordination, or render budget.
///
/// Mapped at the MCP handler boundary: malformed URIs become invalid params;
/// missing/evicted revisions, coordination faults, and render failures become
/// resource-not-found rather than active-document fallthrough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceError {
    /// URI is not a well-formed catalog path or carries disallowed query keys.
    MalformedUri,
    /// Catalog entry does not exist (reserved; retention misses use Coordination).
    NotFound,
    /// Shared TUI retention or lifecycle rejected the read (evicted, wrong doc, loading).
    Coordination(CoordinationError),
    /// Projection failed (stale `semantic_ref`, oversize body, or invalid JSON).
    Render(String),
}

impl std::fmt::Display for ResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedUri => f.write_str("malformed chromewright resource URI"),
            Self::NotFound => f.write_str("resource not found"),
            Self::Coordination(error) => write!(f, "{error}"),
            Self::Render(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ResourceError {}

impl From<CoordinationError> for ResourceError {
    fn from(value: CoordinationError) -> Self {
        Self::Coordination(value)
    }
}

/// Parse a catalog URI.
///
/// `offset`/`limit` are supported only on full-document semantic Markdown URIs.
/// Pagination on outline, JSON, debug, component, selection, or attention is
/// rejected explicitly rather than ignored.
pub fn parse_uri(uri: &str) -> Result<ResourceUri, ResourceError> {
    let remainder = uri
        .strip_prefix(SCHEME)
        .ok_or(ResourceError::MalformedUri)?;
    let (path, query) = match remainder.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (remainder, None),
    };

    if path == "active/semantic.md" {
        let (offset, limit) = parse_markdown_pagination(query)?;
        return Ok(ResourceUri::ActiveSemanticMd { offset, limit });
    }
    if path == "tui/selection.json" {
        reject_pagination(query)?;
        return Ok(ResourceUri::Selection);
    }
    if path == "tui/attention.json" {
        reject_pagination(query)?;
        return Ok(ResourceUri::Attention);
    }

    let segments: Vec<&str> = path.split('/').collect();
    match segments.as_slice() {
        ["page", document_id, revision, "semantic.md"] => {
            validate_id(document_id)?;
            validate_id(revision)?;
            let (offset, limit) = parse_markdown_pagination(query)?;
            Ok(ResourceUri::PageSemanticMd {
                document_id: (*document_id).into(),
                revision: (*revision).into(),
                offset,
                limit,
            })
        }
        ["page", document_id, revision, "outline.md"] => {
            validate_id(document_id)?;
            validate_id(revision)?;
            reject_pagination(query)?;
            Ok(ResourceUri::PageOutline {
                document_id: (*document_id).into(),
                revision: (*revision).into(),
            })
        }
        ["page", document_id, revision, "semantic.json"] => {
            validate_id(document_id)?;
            validate_id(revision)?;
            reject_pagination(query)?;
            Ok(ResourceUri::PageSemanticJson {
                document_id: (*document_id).into(),
                revision: (*revision).into(),
            })
        }
        ["page", document_id, revision, "debug.json"] => {
            validate_id(document_id)?;
            validate_id(revision)?;
            reject_pagination(query)?;
            Ok(ResourceUri::PageDebugJson {
                document_id: (*document_id).into(),
                revision: (*revision).into(),
            })
        }
        [
            "page",
            document_id,
            revision,
            "component",
            semantic_ref_and_suffix,
        ] if semantic_ref_and_suffix.ends_with(".md") => {
            validate_id(document_id)?;
            validate_id(revision)?;
            reject_pagination(query)?;
            let semantic_ref = semantic_ref_and_suffix
                .strip_suffix(".md")
                .filter(|value| !value.is_empty())
                .ok_or(ResourceError::MalformedUri)?;
            // Component refs may contain dots; only the final `.md` is the suffix.
            Ok(ResourceUri::PageComponent {
                document_id: (*document_id).into(),
                revision: (*revision).into(),
                semantic_ref: decode_path_segment(semantic_ref)?,
            })
        }
        _ => Err(ResourceError::MalformedUri),
    }
}

fn validate_id(value: &str) -> Result<(), ResourceError> {
    if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c == '/') {
        return Err(ResourceError::MalformedUri);
    }
    Ok(())
}

fn parse_markdown_pagination(query: Option<&str>) -> Result<(usize, usize), ResourceError> {
    let mut offset = 0usize;
    let mut limit = DEFAULT_MARKDOWN_PAGE_CHARS;
    if let Some(query) = query {
        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = pair.split_once('=').ok_or(ResourceError::MalformedUri)?;
            match key {
                "offset" => {
                    offset = value
                        .parse::<usize>()
                        .map_err(|_| ResourceError::MalformedUri)?;
                }
                "limit" => {
                    limit = value
                        .parse::<usize>()
                        .map_err(|_| ResourceError::MalformedUri)?;
                    if limit == 0 {
                        return Err(ResourceError::MalformedUri);
                    }
                    limit = limit.min(MAX_RESOURCE_CHARS);
                }
                _ => return Err(ResourceError::MalformedUri),
            }
        }
    }
    Ok((offset, limit.min(MAX_RESOURCE_CHARS)))
}

/// Reject `offset`/`limit` on non-paginated resource URIs.
fn reject_pagination(query: Option<&str>) -> Result<(), ResourceError> {
    let Some(query) = query else {
        return Ok(());
    };
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let key = pair.split_once('=').map(|(k, _)| k).unwrap_or(pair);
        if key == "offset" || key == "limit" {
            return Err(ResourceError::MalformedUri);
        }
        // Unknown query keys are also fail-closed for non-paginated URIs.
        return Err(ResourceError::MalformedUri);
    }
    Ok(())
}

fn decode_path_segment(value: &str) -> Result<String, ResourceError> {
    // Accept raw opaque refs and percent-encoded forms without inventing identity.
    if value.contains('%') {
        percent_decode(value)
    } else {
        Ok(value.to_owned())
    }
}

fn percent_decode(input: &str) -> Result<String, ResourceError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hi = from_hex(bytes[index + 1]).ok_or(ResourceError::MalformedUri)?;
                let lo = from_hex(bytes[index + 2]).ok_or(ResourceError::MalformedUri)?;
                out.push((hi << 4) | lo);
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ResourceError::MalformedUri)
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
fn encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Static templates advertised by the companion resource capability.
pub fn resource_templates() -> ListResourceTemplatesResult {
    ListResourceTemplatesResult::with_all_items(vec![
        ResourceTemplate::new(
            "chromewright://page/{document_id}/{revision}/semantic.md",
            "page_semantic_md",
        )
        .with_description("Immutable revision-addressed semantic Markdown")
        .with_mime_type("text/markdown"),
        ResourceTemplate::new(
            "chromewright://page/{document_id}/{revision}/outline.md",
            "page_outline_md",
        )
        .with_description("Immutable revision-addressed semantic outline")
        .with_mime_type("text/markdown"),
        ResourceTemplate::new(
            "chromewright://page/{document_id}/{revision}/semantic.json",
            "page_semantic_json",
        )
        .with_description("Immutable revision-addressed semantic JSON")
        .with_mime_type("application/json"),
        ResourceTemplate::new(
            "chromewright://page/{document_id}/{revision}/debug.json",
            "page_debug_json",
        )
        .with_description("Immutable revision-addressed debug projection")
        .with_mime_type("application/json"),
        ResourceTemplate::new(
            "chromewright://page/{document_id}/{revision}/component/{semantic_ref}.md",
            "page_component_md",
        )
        .with_description("Immutable revision-addressed component Markdown fragment")
        .with_mime_type("text/markdown"),
    ])
}

/// Dynamic aliases plus concrete retained revision URIs currently addressable.
pub fn list_resources(shared: &SharedTuiState) -> ListResourcesResult {
    let snapshot = shared.read_snapshot();
    let mut resources = vec![
        Resource::new("chromewright://active/semantic.md", "active_semantic_md")
            .with_description("Dynamic alias of the last complete semantic Markdown capture")
            .with_mime_type("text/markdown")
            .with_meta(lifecycle_meta(&snapshot)),
        Resource::new("chromewright://tui/selection.json", "tui_selection")
            .with_description("Human selection collaboration state (read-only)")
            .with_mime_type("application/json"),
        Resource::new("chromewright://tui/attention.json", "tui_attention")
            .with_description("Agent attention collaboration state (read-only)")
            .with_mime_type("application/json"),
    ];

    for document in snapshot.retained.iter().chain(snapshot.active.iter()) {
        let document_id = &document.document.document_id;
        let revision = &document.document.revision;
        for (suffix, name, mime, description) in [
            (
                "semantic.md",
                "page_semantic_md",
                "text/markdown",
                "Immutable semantic Markdown for a retained revision",
            ),
            (
                "outline.md",
                "page_outline_md",
                "text/markdown",
                "Immutable outline for a retained revision",
            ),
            (
                "semantic.json",
                "page_semantic_json",
                "application/json",
                "Immutable semantic JSON for a retained revision",
            ),
            (
                "debug.json",
                "page_debug_json",
                "application/json",
                "Immutable debug JSON for a retained revision",
            ),
        ] {
            let uri = format!("chromewright://page/{document_id}/{revision}/{suffix}");
            resources.push(
                Resource::new(uri, name)
                    .with_description(description)
                    .with_mime_type(mime),
            );
        }
    }

    ListResourcesResult::with_all_items(resources)
}

/// Non-mutating read of a catalog URI against shared coordination state.
pub fn read_resource(
    shared: &SharedTuiState,
    uri: &str,
) -> Result<ReadResourceResult, ResourceError> {
    let parsed = parse_uri(uri)?;
    match parsed {
        ResourceUri::ActiveSemanticMd { offset, limit } => {
            let snapshot = shared.read_snapshot();
            let document = snapshot
                .active
                .as_ref()
                .ok_or(ResourceError::Coordination(CoordinationError::NoDocument))?;
            let rendered = render_semantic_markdown(document)
                .map_err(|error| ResourceError::Render(error.to_string()))?;
            Ok(markdown_page_result(
                uri,
                &rendered.content,
                &document.document.document_id,
                &document.document.revision,
                &snapshot.lifecycle,
                offset,
                limit,
            ))
        }
        ResourceUri::PageSemanticMd {
            document_id,
            revision,
            offset,
            limit,
        } => {
            let document = shared.revision(&document_id, &revision)?;
            let rendered = render_semantic_markdown(&document)
                .map_err(|error| ResourceError::Render(error.to_string()))?;
            Ok(markdown_page_result(
                uri,
                &rendered.content,
                &document.document.document_id,
                &document.document.revision,
                &Lifecycle::Ready,
                offset,
                limit,
            ))
        }
        ResourceUri::PageOutline {
            document_id,
            revision,
        } => {
            let document = shared.revision(&document_id, &revision)?;
            let rendered = render_outline(&document)
                .map_err(|error| ResourceError::Render(error.to_string()))?;
            // Outline already preserves a complete JSON fence when truncated.
            Ok(text_result(
                uri,
                "text/markdown",
                ensure_resource_body(rendered.content)?,
                revision_meta(&document, rendered.truncated),
            ))
        }
        ResourceUri::PageSemanticJson {
            document_id,
            revision,
        } => {
            let document = shared.revision(&document_id, &revision)?;
            // JSON projections fail closed when over budget — never mid-object truncate.
            let rendered = render_semantic_json(&document)
                .map_err(|error| ResourceError::Render(error.to_string()))?;
            let body = ensure_resource_body(rendered.content)?;
            validate_json_body(&body)?;
            Ok(text_result(
                uri,
                "application/json",
                body,
                revision_meta(&document, false),
            ))
        }
        ResourceUri::PageDebugJson {
            document_id,
            revision,
        } => {
            let document = shared.revision(&document_id, &revision)?;
            let rendered = render_debug(&document)
                .map_err(|error| ResourceError::Render(error.to_string()))?;
            let body = ensure_resource_body(rendered.content)?;
            validate_json_body(&body)?;
            Ok(text_result(
                uri,
                "application/json",
                body,
                revision_meta(&document, false),
            ))
        }
        ResourceUri::PageComponent {
            document_id,
            revision,
            semantic_ref,
        } => {
            let document = shared.revision(&document_id, &revision)?;
            let reference = SemanticRef::from_opaque(semantic_ref);
            let rendered = render_component_markdown(&document, &reference).map_err(|error| {
                // Fail closed for stale/wrong-document/unknown refs; never active fallthrough.
                ResourceError::Render(error.to_string())
            })?;
            Ok(text_result(
                uri,
                "text/markdown",
                ensure_resource_body(rendered.content)?,
                revision_meta(&document, rendered.truncated),
            ))
        }
        ResourceUri::Selection => {
            let snapshot = shared.read_snapshot();
            let body = json!({
                "semantic_ref": snapshot.selection.as_ref().map(|r| r.as_str()),
                "lifecycle": lifecycle_name(&snapshot.lifecycle),
            });
            Ok(text_result(
                uri,
                "application/json",
                body.to_string(),
                lifecycle_meta(&snapshot),
            ))
        }
        ResourceUri::Attention => {
            let snapshot = shared.read_snapshot();
            // Enforce the same message cap used at the tool boundary.
            let message = snapshot.attention.message.as_ref().map(|message| {
                message
                    .chars()
                    .take(crate::tui::MAX_ATTENTION_MESSAGE_CHARS)
                    .collect::<String>()
            });
            let body = json!({
                "semantic_ref": snapshot.attention.semantic_ref.as_ref().map(|r| r.as_str()),
                "document_id": snapshot.attention.document_id,
                "revision": snapshot.attention.revision,
                "message": message,
                "lifecycle": lifecycle_name(&snapshot.lifecycle),
            });
            Ok(text_result(
                uri,
                "application/json",
                body.to_string(),
                lifecycle_meta(&snapshot),
            ))
        }
    }
}

/// Fail closed when a complete renderer body would exceed the resource budget.
/// Never character-truncates JSON or structured Markdown into invalid content.
fn ensure_resource_body(content: String) -> Result<String, ResourceError> {
    let chars = content.chars().count();
    if chars > MAX_RESOURCE_CHARS {
        return Err(ResourceError::Render(format!(
            "resource body of {chars} characters exceeds the {MAX_RESOURCE_CHARS} character limit"
        )));
    }
    Ok(content)
}

fn validate_json_body(content: &str) -> Result<(), ResourceError> {
    serde_json::from_str::<Value>(content).map_err(|error| {
        ResourceError::Render(format!("resource JSON is not structurally valid: {error}"))
    })?;
    Ok(())
}

fn markdown_page_result(
    uri: &str,
    content: &str,
    document_id: &str,
    revision: &str,
    lifecycle: &Lifecycle,
    offset: usize,
    limit: usize,
) -> ReadResourceResult {
    let chars: Vec<char> = content.chars().collect();
    let total = chars.len();
    let start = offset.min(total);
    let end = (start + limit).min(total);
    let page: String = chars[start..end].iter().collect();
    let truncated = end < total;
    let next_offset = truncated.then_some(end);

    let mut meta_map = BTreeMap::new();
    meta_map.insert("document_id".into(), Value::String(document_id.into()));
    meta_map.insert("revision".into(), Value::String(revision.into()));
    meta_map.insert(
        "lifecycle".into(),
        Value::String(lifecycle_name(lifecycle).into()),
    );
    meta_map.insert(
        "claims_ready".into(),
        json!(matches!(lifecycle, Lifecycle::Ready)),
    );
    meta_map.insert("offset".into(), json!(start));
    meta_map.insert("limit".into(), json!(limit));
    meta_map.insert("total_chars".into(), json!(total));
    meta_map.insert("truncated".into(), json!(truncated));
    if let Some(next_offset) = next_offset {
        meta_map.insert("next_offset".into(), json!(next_offset));
    }

    let contents = ResourceContents::text(page, uri).with_mime_type("text/markdown");
    let mut result = ReadResourceResult::new(vec![contents]);
    result.meta = Some(Meta(meta_map.into_iter().collect()));
    result
}

fn text_result(uri: &str, mime: &str, text: String, meta: Meta) -> ReadResourceResult {
    let contents = ResourceContents::text(text, uri).with_mime_type(mime);
    let mut result = ReadResourceResult::new(vec![contents]);
    result.meta = Some(meta);
    result
}

fn revision_meta(document: &SemanticDocument, truncated: bool) -> Meta {
    let mut map = BTreeMap::new();
    map.insert(
        "document_id".into(),
        Value::String(document.document.document_id.clone()),
    );
    map.insert(
        "revision".into(),
        Value::String(document.document.revision.clone()),
    );
    map.insert("truncated".into(), json!(truncated));
    Meta(map.into_iter().collect())
}

fn lifecycle_meta(snapshot: &CoordinationSnapshot) -> Meta {
    let mut map = BTreeMap::new();
    map.insert(
        "lifecycle".into(),
        Value::String(lifecycle_name(&snapshot.lifecycle).into()),
    );
    if let Some(document) = &snapshot.active {
        map.insert(
            "document_id".into(),
            Value::String(document.document.document_id.clone()),
        );
        map.insert(
            "revision".into(),
            Value::String(document.document.revision.clone()),
        );
    }
    // Explicit: Loading never claims Ready even when last document is present.
    map.insert("claims_ready".into(), json!(snapshot.claims_ready()));
    Meta(map.into_iter().collect())
}

fn lifecycle_name(lifecycle: &Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Ready => "ready",
        Lifecycle::Loading { .. } => "loading",
        Lifecycle::Error { .. } => "error",
    }
}

/// Build a concrete component resource URI for a retained document/ref.
#[cfg(test)]
pub fn component_uri(document_id: &str, revision: &str, semantic_ref: &str) -> String {
    format!(
        "chromewright://page/{document_id}/{revision}/component/{}.md",
        encode_path_segment(semantic_ref)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DocumentMetadata;
    use crate::semantic::SemanticDocument;
    use crate::tui::SharedTuiState;
    use serde_json::Value;

    fn empty_doc(document_id: &str, revision: &str) -> SemanticDocument {
        SemanticDocument::empty(DocumentMetadata {
            document_id: document_id.into(),
            revision: revision.into(),
            url: format!("https://example.test/{revision}"),
            title: format!("Title {revision}"),
            ready_state: "complete".into(),
            frames: vec![],
        })
        .expect("document")
    }

    fn shared() -> SharedTuiState {
        SharedTuiState::with_retention(2)
    }

    fn text_body(result: &ReadResourceResult) -> &str {
        match result.contents.first() {
            Some(ResourceContents::TextResourceContents { text, .. }) => text.as_str(),
            _ => panic!("expected text resource contents"),
        }
    }

    #[test]
    fn tui_resource_parses_catalog_and_rejects_malformed() {
        assert!(matches!(
            parse_uri("chromewright://active/semantic.md"),
            Ok(ResourceUri::ActiveSemanticMd { .. })
        ));
        assert!(matches!(
            parse_uri("chromewright://page/tab/r1/semantic.md?offset=10&limit=5"),
            Ok(ResourceUri::PageSemanticMd {
                offset: 10,
                limit: 5,
                ..
            })
        ));
        assert!(matches!(
            parse_uri("chromewright://page/tab/r1/component/sref1.x.md"),
            Ok(ResourceUri::PageComponent { .. })
        ));
        assert_eq!(parse_uri("file:///tmp/x"), Err(ResourceError::MalformedUri));
        assert_eq!(
            parse_uri("chromewright://page//r1/semantic.md"),
            Err(ResourceError::MalformedUri)
        );
        // Pagination is Markdown semantic only — reject on other catalog URIs.
        assert_eq!(
            parse_uri("chromewright://page/tab/r1/outline.md?offset=0"),
            Err(ResourceError::MalformedUri)
        );
        assert_eq!(
            parse_uri("chromewright://page/tab/r1/semantic.json?limit=10"),
            Err(ResourceError::MalformedUri)
        );
        assert_eq!(
            parse_uri("chromewright://page/tab/r1/debug.json?offset=1&limit=2"),
            Err(ResourceError::MalformedUri)
        );
        assert_eq!(
            parse_uri("chromewright://tui/selection.json?offset=0"),
            Err(ResourceError::MalformedUri)
        );
        assert_eq!(
            parse_uri("chromewright://tui/attention.json?limit=1"),
            Err(ResourceError::MalformedUri)
        );
        assert_eq!(
            parse_uri("chromewright://page/tab/r1/component/sref1.x.md?offset=0"),
            Err(ResourceError::MalformedUri)
        );
    }

    #[test]
    fn tui_resource_active_alias_and_revision_uris_share_state_without_fallthrough() {
        let shared = shared();
        shared.publish(empty_doc("tab", "r1"));
        shared.publish(empty_doc("tab", "r2"));
        shared.publish(empty_doc("tab", "r3"));
        // retention limit 2 keeps r2/r3 historical when r4 becomes active; r1 is evicted.
        shared.publish(empty_doc("tab", "r4"));

        let active = read_resource(&shared, "chromewright://active/semantic.md").expect("active");
        assert!(
            text_body(&active).contains("revision=\"r4\"") || text_body(&active).contains("r4")
        );
        assert_eq!(
            active
                .meta
                .as_ref()
                .and_then(|meta| meta.0.get("claims_ready"))
                .and_then(Value::as_bool),
            Some(true)
        );

        let r2 = read_resource(&shared, "chromewright://page/tab/r2/semantic.md").expect("r2");
        assert!(text_body(&r2).contains("r2"));

        let missing = read_resource(&shared, "chromewright://page/tab/r1/semantic.md");
        assert!(matches!(
            missing,
            Err(ResourceError::Coordination(
                CoordinationError::EvictedRevision
            ))
        ));

        let wrong = read_resource(&shared, "chromewright://page/other/r4/outline.md");
        assert!(matches!(
            wrong,
            Err(ResourceError::Coordination(
                CoordinationError::WrongDocument
            ))
        ));

        let unavailable = read_resource(&shared, "chromewright://page/tab/nope/semantic.json");
        assert!(matches!(
            unavailable,
            Err(ResourceError::Coordination(
                CoordinationError::RevisionUnavailable
            ))
        ));
    }

    #[test]
    fn tui_resource_loading_reads_do_not_claim_ready() {
        let shared = shared();
        shared.publish(empty_doc("tab", "r1"));
        shared.begin_page_action("navigate").unwrap();
        let active = read_resource(&shared, "chromewright://active/semantic.md").expect("active");
        let meta = active.meta.as_ref().expect("meta");
        assert_eq!(
            meta.0.get("lifecycle").and_then(Value::as_str),
            Some("loading")
        );
        assert_eq!(
            meta.0.get("claims_ready").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(meta.0.get("revision").and_then(Value::as_str), Some("r1"));
    }

    #[test]
    fn tui_resource_markdown_pagination_is_bounded() {
        let shared = shared();
        shared.publish(empty_doc("tab", "r1"));
        // Force a long body by reading with tiny limit repeatedly.
        let first = read_resource(
            &shared,
            "chromewright://active/semantic.md?offset=0&limit=20",
        )
        .unwrap();
        let first_text = text_body(&first);
        assert!(first_text.chars().count() <= 20);
        let meta = first.meta.as_ref().unwrap();
        let total = meta.0.get("total_chars").and_then(Value::as_u64).unwrap() as usize;
        if total > 20 {
            assert_eq!(meta.0.get("truncated").and_then(Value::as_bool), Some(true));
            let next = meta.0.get("next_offset").and_then(Value::as_u64).unwrap();
            assert_eq!(next, 20);
        }
    }

    #[test]
    fn tui_resource_selection_and_attention_are_read_only_aliases() {
        use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};

        let shared = shared();
        let document = normalize_fixture(
            DocumentMetadata {
                document_id: "tab".into(),
                revision: "r1".into(),
                url: "https://example.test/r1".into(),
                title: "Title r1".into(),
                ready_state: "complete".into(),
                frames: vec![],
            },
            vec![RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                id: Some("spotlight".into()),
                unique_id: true,
                selector: None,
                text: Some("hello".into()),
                href: None,
                landmark: None,
                heading_level: None,
                ordered: None,
                label: None,
                src: None,
                alt: None,
                name: None,
                value: None,
                input_type: None,
                placeholder: None,
                checked: None,
                disabled: None,
                required: None,
                readonly: None,
                multiple: None,
                button_type: None,
                options: vec![],
                children: vec![],
            }],
        )
        .expect("doc");
        let reference = document.semantic_refs().into_iter().next().unwrap();
        shared.publish(document);
        shared
            .set_attention(reference.clone(), Some("focus here".into()))
            .expect("attention");
        let selection =
            read_resource(&shared, "chromewright://tui/selection.json").expect("selection");
        assert!(text_body(&selection).contains("semantic_ref"));
        let attention =
            read_resource(&shared, "chromewright://tui/attention.json").expect("attention");
        let body = text_body(&attention);
        assert!(body.contains("focus here"));
        assert!(body.contains(reference.as_str()));
        assert!(body.contains("\"document_id\":\"tab\""));
        assert!(body.contains("\"revision\":\"r1\""));
    }

    #[test]
    fn tui_resource_json_bodies_are_valid_or_fail_closed() {
        let shared = shared();
        shared.publish(empty_doc("tab", "r1"));
        let semantic = read_resource(&shared, "chromewright://page/tab/r1/semantic.json")
            .expect("semantic.json");
        let body = text_body(&semantic);
        serde_json::from_str::<Value>(body).expect("valid semantic json");
        let debug =
            read_resource(&shared, "chromewright://page/tab/r1/debug.json").expect("debug.json");
        serde_json::from_str::<Value>(text_body(&debug)).expect("valid debug json");
    }

    #[test]
    fn tui_resource_list_includes_aliases_and_retained_revision_views() {
        let shared = shared();
        shared.publish(empty_doc("tab", "r1"));
        shared.publish(empty_doc("tab", "r2"));
        let listed = list_resources(&shared);
        let uris: Vec<&str> = listed
            .resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect();
        assert!(uris.contains(&"chromewright://active/semantic.md"));
        assert!(uris.contains(&"chromewright://tui/selection.json"));
        assert!(uris.contains(&"chromewright://page/tab/r1/semantic.md"));
        assert!(uris.contains(&"chromewright://page/tab/r2/debug.json"));
        assert!(!resource_templates().resource_templates.is_empty());
    }

    #[test]
    fn tui_resource_reads_do_not_mutate_shared_state() {
        use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};

        let shared = shared();
        let document = normalize_fixture(
            DocumentMetadata {
                document_id: "tab".into(),
                revision: "r1".into(),
                url: "https://example.test/r1".into(),
                title: "Title r1".into(),
                ready_state: "complete".into(),
                frames: vec![],
            },
            vec![RawSemanticNode {
                kind: "text".into(),
                tag: Some("p".into()),
                id: Some("spotlight".into()),
                unique_id: true,
                selector: None,
                text: Some("hello".into()),
                href: None,
                landmark: None,
                heading_level: None,
                ordered: None,
                label: None,
                src: None,
                alt: None,
                name: None,
                value: None,
                input_type: None,
                placeholder: None,
                checked: None,
                disabled: None,
                required: None,
                readonly: None,
                multiple: None,
                button_type: None,
                options: vec![],
                children: vec![],
            }],
        )
        .expect("doc");
        let reference = document.semantic_refs().into_iter().next().unwrap();
        shared.publish(document);
        shared
            .set_attention(reference.clone(), Some("before".into()))
            .expect("attention");
        let _ = read_resource(&shared, "chromewright://active/semantic.md").unwrap();
        let _ = read_resource(&shared, "chromewright://tui/attention.json").unwrap();
        assert_eq!(shared.attention().message.as_deref(), Some("before"));
        assert_eq!(shared.attention().semantic_ref.as_ref(), Some(&reference));
        assert_eq!(shared.active().unwrap().document.revision, "r1");
    }

    #[test]
    fn tui_resource_component_uri_encoding_roundtrip() {
        let ref_token = "sref1.tab..r1..author_id..main";
        let uri = component_uri("tab", "r1", ref_token);
        match parse_uri(&uri).unwrap() {
            ResourceUri::PageComponent { semantic_ref, .. } => {
                assert_eq!(semantic_ref, ref_token);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn tui_resource_component_read_fails_closed_for_unknown_ref() {
        let shared = shared();
        shared.publish(empty_doc("tab", "r1"));
        let err = read_resource(
            &shared,
            "chromewright://page/tab/r1/component/sref1.not-a-real-ref.md",
        )
        .expect_err("unknown component");
        assert!(matches!(err, ResourceError::Render(_)));
    }
}
