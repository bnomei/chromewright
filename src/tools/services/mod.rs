//! Shared orchestration helpers reused by multiple MCP tools.
//!
//! Submodules own multi-step flows so individual tool modules stay thin:
//! target resolution / stale-cursor policy, actionability polling, interaction
//! handoffs, markdown extraction with session cache reuse, wait conditions, and
//! `inspect_node` probe assembly.

pub(crate) mod inspection;
pub(crate) mod interaction;
pub(crate) mod markdown;
pub(crate) mod wait;
