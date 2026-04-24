//! Internal implementation crate for the `chromewright` CLI.
//!
//! The supported user-facing surface is the `chromewright` binary and its MCP tool contract.
//! This crate remains public for repository-internal tests and refactors, but the Rust embedding
//! API is not treated as a stable product surface yet.

#[doc(hidden)]
pub mod browser;
#[doc(hidden)]
pub mod contract;
#[doc(hidden)]
pub mod dom;
#[doc(hidden)]
pub mod error;
#[doc(hidden)]
pub mod tools;

#[cfg(feature = "mcp-handler")]
#[doc(hidden)]
pub mod mcp;

#[doc(hidden)]
pub use browser::{BrowserSession, ClosedTabSummary, ConnectionOptions, LaunchOptions, TabInfo};
#[doc(hidden)]
pub use dom::{
    BoundingBox, DocumentMetadata, DomTree, ElementNode, FrameMetadata, NodeRef, SnapshotNode,
};
#[doc(hidden)]
pub use error::{BrowserError, Result};
#[doc(hidden)]
pub use tools::{Tool, ToolContext, ToolRegistry, ToolResult};

#[cfg(feature = "mcp-handler")]
#[doc(hidden)]
pub use mcp::BrowserServer;
#[cfg(feature = "mcp-handler")]
#[doc(hidden)]
pub use rmcp::ServiceExt;
