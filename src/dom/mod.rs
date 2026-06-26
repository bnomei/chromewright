//! Accessibility-tree extraction and snapshot indexing for actionable page elements.
//!
//! `DomTree` captures an ARIA snapshot with revision-scoped `NodeRef` and `Cursor`
//! handoffs that downstream tools use for target resolution and stale detection.

pub mod element;
pub mod tree;
pub mod yaml;

pub use element::{AriaChild, AriaNode, BoundingBox, ElementNode};
pub use tree::{Cursor, DocumentMetadata, DomTree, FrameMetadata, NodeRef, SnapshotNode};
pub use yaml::{yaml_escape_key_if_needed, yaml_escape_value_if_needed};
