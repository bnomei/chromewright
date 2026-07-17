//! Semantic document capture, normalization, fail-closed identity, and shared renderers.
//!
//! This module is compiled only with the opt-in `tui` feature. It provides the
//! shared `SemanticDocument` model and projections (Markdown, JSON, outline,
//! debug, Ratatui) that later TUI and MCP surfaces consume. It does not expose
//! MCP tools, resources, CLI commands, or interaction actions.

mod component;
mod document;
mod extract;
mod identity;
mod limits;
mod normalize;
mod render;

#[cfg(test)]
mod tests;

pub use component::{LandmarkRole, SelectOption, SemanticAttrs, SemanticComponent, SemanticKind};
pub use document::{SemanticComponentIter, SemanticDocument};
pub use extract::extract_semantic_document;
pub use identity::{SemanticRef, SemanticRefError};
pub use limits::{
    MAX_SEMANTIC_COMPONENTS, MAX_SEMANTIC_DEPTH, MAX_SEMANTIC_SELECT_OPTIONS,
    MAX_SEMANTIC_STRING_CHARS, MAX_SEMANTIC_TOTAL_TEXT_CHARS,
};
pub use normalize::{RawSelectOption, RawSemanticNode, normalize_fixture};
pub use render::{
    ComponentProjection, DebugNode, DebugProjection, MAX_RENDER_OUTPUT_CHARS, OutlineEntry,
    OutlineProjection, RenderError, RenderedOutput, SemanticJsonProjection, SemanticRatatuiView,
    buffer_to_lines, render_component_json, render_component_markdown, render_debug,
    render_outline, render_ratatui_buffer, render_ratatui_lines, render_semantic_json,
    render_semantic_markdown, truncate_output,
};
