//! Semantic document capture, normalization, and fail-closed identity.
//!
//! This module is compiled only with the opt-in `tui` feature. It provides the
//! shared `SemanticDocument` model future renderers and TUI surfaces will consume.
//! It does not expose MCP tools, resources, CLI commands, or interaction actions.

mod component;
mod document;
mod extract;
mod identity;
mod limits;
mod normalize;

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
