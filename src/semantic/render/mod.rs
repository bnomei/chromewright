//! Shared semantic renderers: Markdown, projections, and Ratatui frames.
//!
//! All renderers consume only [`SemanticDocument`] and its components/refs.
//! They never inspect raw HTML or drive the browser. Output is bounded and
//! preserves model-provided opaque `semantic_ref` values.

mod bounds;
mod markdown;
mod projections;
mod ratatui_view;

#[cfg(test)]
mod tests;

pub use bounds::{MAX_RENDER_OUTPUT_CHARS, RenderError, RenderedOutput, truncate_output};
pub use markdown::{render_component_markdown, render_semantic_markdown};
pub use projections::{
    ComponentProjection, DebugNode, DebugProjection, OutlineEntry, OutlineProjection,
    SemanticJsonProjection, render_component_json, render_debug, render_outline,
    render_semantic_json,
};
pub use ratatui_view::{
    SemanticRatatuiView, buffer_to_lines, render_ratatui_buffer, render_ratatui_lines,
};
