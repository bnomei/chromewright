//! Internal tool execution result before MCP envelope conversion.
//!
//! Tool-local failures keep `success = false` with structured `data` so the MCP
//! adapter can emit a normal `CallToolResult` error body. Infrastructure faults
//! bypass this type and become MCP internal errors.

use serde_json::Value;
use std::collections::HashMap;

/// Success or failure payload produced by a tool before MCP `CallToolResult` conversion.
///
/// Tool-local failures set `success = false` and still travel as structured
/// `CallToolResult` content (preserving `document` / `target` / `recovery` in `data`).
/// Infrastructure faults bypass this type and become MCP internal errors.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResult {
    /// Tool-local success; false yields a structured error envelope, not an MCP internal error.
    pub success: bool,

    /// Structured success payload or tool-error fields (`code`, `error`, envelopes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,

    /// Human-readable tool-local error when `success` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Operation metrics and adapter hints (duration, browser evaluations, …).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
}

impl ToolResult {
    /// Successful tool-local result with optional structured JSON data.
    pub fn success(data: Option<Value>) -> Self {
        Self {
            success: true,
            data,
            error: None,
            metadata: HashMap::new(),
        }
    }

    /// Successful tool-local result serializing `data` to JSON.
    pub fn success_with<T: serde::Serialize>(data: T) -> Self {
        Self {
            success: true,
            data: serde_json::to_value(data).ok(),
            error: None,
            metadata: HashMap::new(),
        }
    }

    /// Tool-local failure with a message and no structured error body.
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error.into()),
            metadata: HashMap::new(),
        }
    }

    /// Tool-local failure with a message plus structured error fields for MCP clients.
    pub fn failure_with<T: serde::Serialize>(error: impl Into<String>, data: T) -> Self {
        Self {
            success: false,
            data: serde_json::to_value(data).ok(),
            error: Some(error.into()),
            metadata: HashMap::new(),
        }
    }

    /// Attach a metadata entry (metrics, tool name) without changing success state.
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}
