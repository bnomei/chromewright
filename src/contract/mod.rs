//! Shared structured envelopes and JSON schemas for MCP tool inputs and outputs.
//!
//! Types here define the cross-tool contract for document metadata, element targeting,
//! snapshot scope, and viewport emulation so tools and the MCP adapter serialize consistently.

mod document;
mod target;
mod tool_result;
mod viewport;

pub use document::{
    DocumentActionResult, DocumentEnvelope, DocumentResult, SnapshotMode, SnapshotScope,
    TargetedActionResult,
};
pub use target::{PublicTarget, TargetEnvelope, TargetStatus};
pub use tool_result::ToolResult;
pub use viewport::{
    ViewportEmulation, ViewportEmulationRequest, ViewportMetrics, ViewportOperationResult,
    ViewportOrientation, ViewportResetRequest,
};
