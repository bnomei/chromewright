//! # browser-use
//!
//! A Rust library for browser automation via Chrome DevTools Protocol (CDP), designed for AI agent integration.
//!
//! ## Features
//!
//! - **MCP Server**: Model Context Protocol server for AI-driven browser automation
//! - **Browser Session Management**: Launch or connect to Chrome/Chromium instances
//! - **Tool System**: High-level browser operations (snapshot, navigate, click, input, wait, tabs, extraction)
//! - **DOM Extraction**: Extract page structure with revision-scoped node references for AI-friendly targeting
//!
//! ## MCP Server
//!
//! The recommended way to use this library is via the Model Context Protocol (MCP) server,
//! which exposes browser automation tools to AI agents like Claude:
//!
//! ### Running the MCP Server
//!
//! ```bash
//! # Run a headless local browser over stdio
//! cargo run --features mcp-server --bin mcp-server
//!
//! # Run with a visible local browser
//! cargo run --features mcp-server --bin mcp-server -- --headed
//!
//! # Connect to an existing Chrome DevTools WebSocket instead of launching Chrome
//! cargo run --features mcp-server --bin mcp-server -- \
//!   --ws-endpoint ws://127.0.0.1:9222/devtools/browser/<id>
//!
//! # Serve MCP over streamable HTTP on localhost:3000/mcp
//! cargo run --features mcp-server --bin mcp-server -- --transport http
//! ```
//!
//! ## Library Usage (Advanced)
//!
//! For direct integration in Rust applications:
//!
//! ### Basic Browser Automation
//!
//! ```rust,no_run
//! use browser_use::{BrowserSession, LaunchOptions};
//!
//! # fn main() -> browser_use::Result<()> {
//! // Launch a browser
//! let session = BrowserSession::launch(LaunchOptions::default())?;
//!
//! // Navigate to a page
//! session.navigate("https://example.com")?;
//!
//! // Extract DOM and inspect the current revision
//! let dom = session.extract_dom()?;
//! println!("Document revision: {}", dom.document.revision);
//! # Ok(())
//! # }
//! ```
//!
//! ### Using the Tool System
//!
//! ```rust,no_run
//! use browser_use::{BrowserSession, LaunchOptions};
//! use browser_use::tools::{ToolRegistry, ToolContext};
//! use serde_json::json;
//!
//! # fn main() -> browser_use::Result<()> {
//! let session = BrowserSession::launch(LaunchOptions::default())?;
//! let registry = ToolRegistry::with_defaults();
//! let mut context = ToolContext::new(&session);
//!
//! // Navigate using the tool system
//! registry.execute("navigate", json!({"url": "https://example.com"}), &mut context)?;
//!
//! // Inspect the page and act on a revision-scoped node ref
//! let snapshot = registry.execute("snapshot", json!({}), &mut context)?;
//! let node_ref = snapshot.data
//!     .as_ref()
//!     .and_then(|data| data["nodes"].as_array())
//!     .and_then(|nodes| nodes.first())
//!     .and_then(|node| node.get("node_ref"))
//!     .cloned()
//!     .expect("snapshot should expose at least one actionable node");
//! registry.execute("click", json!({ "node_ref": node_ref }), &mut context)?;
//! # Ok(())
//! # }
//! ```
//!
//! The default registry intentionally excludes advanced operator tools such as raw JavaScript
//! evaluation and filesystem-bound screenshots. Opt into those explicitly only when needed:
//!
//! ```rust,no_run
//! use browser_use::tools::ToolRegistry;
//!
//! let mut registry = ToolRegistry::with_defaults();
//! registry.register_operator_tools();
//! ```
//!
//! High-level action tools now return metadata-first document envelopes by default. Use the
//! `snapshot` tool when you need the full YAML snapshot plus actionable-node list.
//!
//! Once operator tools are registered, `evaluate` and `screenshot` still require
//! `confirm_unsafe = true` per call. High-level `navigate` and `new_tab` also reject unsafe
//! schemes such as `data:` and `file:` unless the caller passes `allow_unsafe = true`.
//!
//! ### Document Snapshots For AI Agents
//!
//! The library exposes actionable elements as revision-scoped node references rather than relying
//! on fragile CSS selectors:
//!
//! ```rust,no_run
//! # use browser_use::{BrowserSession, LaunchOptions};
//! # fn main() -> browser_use::Result<()> {
//! # let session = BrowserSession::launch(LaunchOptions::default())?;
//! # session.navigate("https://example.com")?;
//! let dom = session.extract_dom()?;
//!
//! // Snapshot metadata is revision-scoped, and actionable nodes can be resolved to node refs.
//! println!("Document ID: {}", dom.document.document_id);
//! println!("Document revision: {}", dom.document.revision);
//! # Ok(())
//! # }
//! ```
//!
//! ## Module Overview
//!
//! - [`browser`]: Browser session management and configuration
//! - [`dom`]: DOM extraction, revision-scoped node references, and tree representation
//! - [`tools`]: Browser automation tools and registry composition helpers
//! - [`error`]: Error types and result aliases
//! - [`mcp`]: **Model Context Protocol server** (requires `mcp-handler` feature) - **Start here for AI integration**

pub mod browser;
pub mod dom;
pub mod error;
pub mod tools;

#[cfg(feature = "mcp-handler")]
pub mod mcp;

pub use browser::{BrowserSession, ConnectionOptions, LaunchOptions};
pub use dom::{
    BoundingBox, DocumentMetadata, DomTree, ElementNode, FrameMetadata, NodeRef, SnapshotNode,
};
pub use error::{BrowserError, Result};
pub use tools::{Tool, ToolContext, ToolRegistry, ToolResult};

#[cfg(feature = "mcp-handler")]
pub use mcp::BrowserServer;
#[cfg(feature = "mcp-handler")]
pub use rmcp::ServiceExt;
