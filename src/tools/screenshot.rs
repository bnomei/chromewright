use crate::error::{BrowserError, Result};
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScreenshotParams {
    /// Path to save the screenshot
    pub path: String,

    /// Capture full page (default: false)
    #[serde(default)]
    pub full_page: bool,

    /// Explicit acknowledgement that this operator tool writes to the local filesystem.
    #[serde(default)]
    pub confirm_unsafe: bool,
}

#[derive(Default)]
pub struct ScreenshotTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScreenshotOutput {
    pub path: String,
    pub resolved_path: String,
    pub size_bytes: usize,
    pub full_page: bool,
}

impl Tool for ScreenshotTool {
    type Params = ScreenshotParams;
    type Output = ScreenshotOutput;

    fn name(&self) -> &str {
        "screenshot"
    }

    fn execute_typed(
        &self,
        params: ScreenshotParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        if !params.confirm_unsafe {
            return Err(BrowserError::InvalidArgument(
                "screenshot requires confirm_unsafe=true".to_string(),
            ));
        }

        let screenshot_data = context
            .session
            .tab()?
            .capture_screenshot(
                headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                None,
                None,
                params.full_page,
            )
            .map_err(|e| BrowserError::ScreenshotFailed(e.to_string()))?;

        let output_path = resolve_output_path(&params.path)?;
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                BrowserError::ScreenshotFailed(format!(
                    "Failed to prepare screenshot directory: {}",
                    e
                ))
            })?;
        }

        std::fs::write(&output_path, &screenshot_data).map_err(|e| {
            BrowserError::ScreenshotFailed(format!("Failed to save screenshot: {}", e))
        })?;

        Ok(ToolResult::success_with(ScreenshotOutput {
            path: params.path,
            resolved_path: output_path.display().to_string(),
            size_bytes: screenshot_data.len(),
            full_page: params.full_page,
        }))
    }
}

fn resolve_output_path(path: &str) -> Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(BrowserError::InvalidArgument(
            "screenshot path cannot be empty".to_string(),
        ));
    }

    let relative = Path::new(trimmed);
    if relative.is_absolute() {
        return Err(BrowserError::InvalidArgument(
            "screenshot path must be relative to the current working directory".to_string(),
        ));
    }

    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(BrowserError::InvalidArgument(
            "screenshot path must not escape the current working directory".to_string(),
        ));
    }

    let cwd = std::env::current_dir()?;
    Ok(cwd.join(relative))
}

#[cfg(test)]
mod tests {
    use super::resolve_output_path;

    #[test]
    fn test_resolve_output_path_rejects_absolute_paths() {
        let result = resolve_output_path("/tmp/test.png");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_output_path_rejects_parent_traversal() {
        let result = resolve_output_path("../test.png");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_output_path_accepts_safe_relative_paths() {
        let result = resolve_output_path("artifacts/test.png").expect("path should resolve");
        assert!(result.ends_with("artifacts/test.png"));
    }
}
