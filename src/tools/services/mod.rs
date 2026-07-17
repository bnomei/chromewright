//! Shared orchestration helpers reused by multiple MCP tools.
//!
//! Submodules own multi-step flows (target recovery, actionability polling, markdown
//! pagination, wait conditions, node inspection) so individual tool crates stay thin.

pub(crate) mod inspection;
pub(crate) mod interaction;
pub(crate) mod markdown;
pub(crate) mod wait;
