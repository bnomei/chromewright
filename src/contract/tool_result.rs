use serde_json::Value;
use std::collections::HashMap;

/// Result of tool execution
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResult {
    /// Whether the tool execution was successful
    pub success: bool,

    /// Result data (JSON value)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,

    /// Error message if execution failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Additional metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
}

impl ToolResult {
    /// Create a successful result
    pub fn success(data: Option<Value>) -> Self {
        Self {
            success: true,
            data,
            error: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a successful result with data
    pub fn success_with<T: serde::Serialize>(data: T) -> Self {
        Self {
            success: true,
            data: serde_json::to_value(data).ok(),
            error: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a failure result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error.into()),
            metadata: HashMap::new(),
        }
    }

    /// Create a failure result with structured error data.
    pub fn failure_with<T: serde::Serialize>(error: impl Into<String>, data: T) -> Self {
        Self {
            success: false,
            data: serde_json::to_value(data).ok(),
            error: Some(error.into()),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the result
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}
