//! Unified error taxonomy for browser, tool, and resource-limit failures.
//!
//! `BrowserError` is the single propagation type from CDP through tools to MCP;
//! structured detail structs carry recovery hints for attach-session degradation.

use thiserror::Error;

/// Structured context when the active CDP page target is lost or attach mode degrades.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageTargetLostDetails {
    /// Name of the backend or tool operation that observed the loss.
    pub operation: String,
    /// Human-readable failure detail from CDP or tab resolution.
    pub detail: String,
    /// When true, the session may retry after reacquiring a page target.
    pub recoverable: bool,
    /// Agent-facing steps for attach-mode degradation (tab_list / switch_tab / reconnect).
    pub recovery_hint: Option<String>,
}

impl PageTargetLostDetails {
    /// Build a recoverable page target loss suitable for retry after tab reacquisition.
    pub fn recoverable(operation: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            detail: detail.into(),
            recoverable: true,
            recovery_hint: None,
        }
    }

    /// Build a non-recoverable attach-session degradation with an explicit recovery hint.
    pub fn attach_degraded(
        operation: impl Into<String>,
        detail: impl Into<String>,
        recovery_hint: impl Into<String>,
    ) -> Self {
        Self {
            operation: operation.into(),
            detail: detail.into(),
            recoverable: false,
            recovery_hint: Some(recovery_hint.into()),
        }
    }

    /// True when this loss represents attach-mode degradation (not a transient retryable blip).
    pub fn is_attach_session_degraded(&self) -> bool {
        !self.recoverable && self.recovery_hint.is_some()
    }
}

impl std::fmt::Display for PageTargetLostDetails {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "operation={} recoverable={} detail={}",
            self.operation, self.recoverable, self.detail
        )
    }
}

/// Capability gap reported when the active `SessionBackend` cannot honor a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendUnsupportedDetails {
    /// Capability key (for example `screenshot_clip` or `viewport_emulation`).
    pub capability: String,
    /// Operation that requested the capability.
    pub operation: String,
}

impl BackendUnsupportedDetails {
    /// Pair a capability identifier with the operation that needed it.
    pub fn new(capability: impl Into<String>, operation: impl Into<String>) -> Self {
        Self {
            capability: capability.into(),
            operation: operation.into(),
        }
    }
}

impl std::fmt::Display for BackendUnsupportedDetails {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Browser backend does not support {} during {}",
            self.capability, self.operation
        )
    }
}

/// Budget violation for DOM, snapshot, screenshot, or extraction size limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLimitDetails {
    /// Resource budget name (for example `screenshot_png_bytes`).
    pub resource: String,
    /// Additional context about what was rejected.
    pub detail: String,
    /// Configured limit as a display string.
    pub limit: String,
    /// Observed size or count that exceeded the limit.
    pub actual: String,
}

impl ResourceLimitDetails {
    /// Capture a structured resource budget violation for MCP/tool error surfaces.
    pub fn new(
        resource: impl Into<String>,
        detail: impl Into<String>,
        limit: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            resource: resource.into(),
            detail: detail.into(),
            limit: limit.into(),
            actual: actual.into(),
        }
    }
}

impl std::fmt::Display for ResourceLimitDetails {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "resource={} limit={} actual={} detail={}",
            self.resource, self.limit, self.actual, self.detail
        )
    }
}

/// Single error taxonomy from CDP/`SessionBackend` through tools to MCP responses.
#[derive(Error, Debug)]
pub enum BrowserError {
    /// Launch mode failed to spawn or initialize Chrome.
    #[error("Failed to launch browser: {0}")]
    LaunchFailed(String),

    /// Attach mode failed to open a DevTools WebSocket or HTTP endpoint.
    #[error("Failed to connect to browser: {0}")]
    ConnectionFailed(String),

    /// A wait, navigation settle, or other timed operation exceeded its deadline.
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Caller-supplied CSS selector is syntactically invalid for resolution.
    #[error("Invalid selector: {0}")]
    SelectorInvalid(String),

    /// No element matched the selector or interactive index.
    #[error("Element not found: {0}")]
    ElementNotFound(String),

    /// In-page DOM extraction or accessibility tree parse failed.
    #[error("Failed to parse DOM: {0}")]
    DomParseFailed(String),

    /// Named MCP/tool handler failed after argument validation.
    #[error("Tool '{tool}' execution failed: {reason}")]
    ToolExecutionFailed { tool: String, reason: String },

    /// Caller argument violated a contract (viewport bounds, unsafe history, empty tab id, …).
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// Page navigation or history move did not complete successfully.
    #[error("Navigation failed: {0}")]
    NavigationFailed(String),

    /// CDP Runtime evaluation or browser-kernel script failed or returned undecodable data.
    #[error("JavaScript evaluation failed: {0}")]
    EvaluationFailed(String),

    /// Screenshot capture, PNG validation, or artifact filesystem storage failed.
    #[error("Screenshot failed: {0}")]
    ScreenshotFailed(String),

    /// Browser download path or transfer handling failed.
    #[error("Download failed: {0}")]
    DownloadFailed(String),

    /// Tab list, activate, open, or close failed at the `SessionBackend` boundary.
    #[error("Tab operation failed: {0}")]
    TabOperationFailed(String),

    /// Active page target lost or attach session degraded; see structured details for recovery.
    #[error("Page target lost: {0}")]
    PageTargetLost(PageTargetLostDetails),

    /// Active `SessionBackend` lacks the requested capability for this operation.
    #[error("Backend unsupported: {0}")]
    BackendUnsupported(BackendUnsupportedDetails),

    /// Operation would exceed a configured DOM, snapshot, or screenshot resource budget.
    #[error("Resource limit exceeded: {0}")]
    ResourceLimitExceeded(ResourceLimitDetails),

    /// Underlying headless_chrome / CDP transport error not mapped to a more specific variant.
    #[error("Chrome error: {0}")]
    ChromeError(String),

    /// JSON encode/decode failure for tool params, command configs, or evaluation payloads.
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Filesystem or other OS I/O failure (screenshot artifacts, temp dirs, …).
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl BrowserError {
    /// Construct a structured resource-limit failure for tool and capture boundaries.
    pub(crate) fn resource_limit_exceeded(
        resource: impl Into<String>,
        detail: impl Into<String>,
        limit: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self::ResourceLimitExceeded(ResourceLimitDetails::new(resource, detail, limit, actual))
    }
}

/// Convenience alias for fallible chromewright APIs that return [`BrowserError`].
pub type Result<T> = std::result::Result<T, BrowserError>;

/// Map headless_chrome `anyhow` errors into [`BrowserError::ChromeError`].
impl From<anyhow::Error> for BrowserError {
    fn from(err: anyhow::Error) -> Self {
        BrowserError::ChromeError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = BrowserError::LaunchFailed("Chrome not found".to_string());
        assert_eq!(
            err.to_string(),
            "Failed to launch browser: Chrome not found"
        );
    }

    #[test]
    fn test_tool_execution_error() {
        let err = BrowserError::ToolExecutionFailed {
            tool: "navigate".to_string(),
            reason: "Invalid URL".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Tool 'navigate' execution failed: Invalid URL"
        );
    }

    #[test]
    fn test_json_error_conversion() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json");
        assert!(json_err.is_err());

        let browser_err: BrowserError = json_err.unwrap_err().into();
        assert!(matches!(browser_err, BrowserError::JsonError(_)));
    }

    #[test]
    fn test_result_type_alias() {
        fn example_function() -> Result<String> {
            Err(BrowserError::InvalidArgument("test".to_string()))
        }

        let result = example_function();
        assert!(result.is_err());
    }
}
