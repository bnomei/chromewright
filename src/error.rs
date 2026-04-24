use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageTargetLostDetails {
    pub operation: String,
    pub detail: String,
    pub recoverable: bool,
    pub recovery_hint: Option<String>,
}

impl PageTargetLostDetails {
    pub fn recoverable(operation: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            detail: detail.into(),
            recoverable: true,
            recovery_hint: None,
        }
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendUnsupportedDetails {
    pub capability: String,
    pub operation: String,
}

impl BackendUnsupportedDetails {
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

/// Core error type for chromewright operations
#[derive(Error, Debug)]
pub enum BrowserError {
    /// Browser launch failed
    #[error("Failed to launch browser: {0}")]
    LaunchFailed(String),

    /// Browser connection failed
    #[error("Failed to connect to browser: {0}")]
    ConnectionFailed(String),

    /// Operation timed out
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Invalid CSS selector
    #[error("Invalid selector: {0}")]
    SelectorInvalid(String),

    /// Element not found in DOM
    #[error("Element not found: {0}")]
    ElementNotFound(String),

    /// DOM parsing failed
    #[error("Failed to parse DOM: {0}")]
    DomParseFailed(String),

    /// Tool execution failed
    #[error("Tool '{tool}' execution failed: {reason}")]
    ToolExecutionFailed { tool: String, reason: String },

    /// Invalid argument provided to a function
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// Navigation failed
    #[error("Navigation failed: {0}")]
    NavigationFailed(String),

    /// JavaScript evaluation failed
    #[error("JavaScript evaluation failed: {0}")]
    EvaluationFailed(String),

    /// Screenshot capture failed
    #[error("Screenshot failed: {0}")]
    ScreenshotFailed(String),

    /// Download operation failed
    #[error("Download failed: {0}")]
    DownloadFailed(String),

    /// Tab operation failed
    #[error("Tab operation failed: {0}")]
    TabOperationFailed(String),

    /// Browser page target was lost or degraded.
    #[error("Page target lost: {0}")]
    PageTargetLost(PageTargetLostDetails),

    /// Backend does not support the requested capability.
    #[error("Backend unsupported: {0}")]
    BackendUnsupported(BackendUnsupportedDetails),

    /// Chrome/CDP error from headless_chrome crate
    #[error("Chrome error: {0}")]
    ChromeError(String),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Result type alias for chromewright operations
pub type Result<T> = std::result::Result<T, BrowserError>;

/// Convert anyhow::Error from headless_chrome to BrowserError
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
