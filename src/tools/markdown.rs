use crate::error::{BrowserError, Result};
use crate::tools::{Tool, ToolContext, ToolResult, services::markdown::execute_get_markdown};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(test)]
pub(crate) use crate::tools::services::markdown::paginate_markdown;

/// Parameters for getting markdown content with pagination support
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetMarkdownParams {
    /// Page number to extract (1-based index, default: 1)
    #[serde(default = "default_page")]
    pub page: usize,

    /// Maximum characters per page (default: 100000, must be greater than 0)
    #[serde(default = "default_page_size")]
    pub page_size: usize,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    100_000
}

impl Default for GetMarkdownParams {
    fn default() -> Self {
        Self {
            page: default_page(),
            page_size: default_page_size(),
        }
    }
}

impl GetMarkdownParams {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.page_size == 0 {
            return Err(BrowserError::InvalidArgument(
                "get_markdown.page_size must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Default)]
pub struct GetMarkdownTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetMarkdownOutput {
    pub markdown: String,
    pub title: String,
    pub url: String,
    pub current_page: usize,
    pub total_pages: usize,
    pub has_more_pages: bool,
    pub length: usize,
    pub byline: String,
    pub excerpt: String,
    pub site_name: String,
}

impl Tool for GetMarkdownTool {
    type Params = GetMarkdownParams;
    type Output = GetMarkdownOutput;

    fn name(&self) -> &str {
        "get_markdown"
    }

    fn description(&self) -> &str {
        "Read page content as markdown. Extraction only; use snapshot for actions."
    }

    fn execute_typed(
        &self,
        params: GetMarkdownParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        execute_get_markdown(params, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::MarkdownCacheEntry;
    use std::sync::Arc;

    fn sample_entry(full_markdown: &str) -> MarkdownCacheEntry {
        MarkdownCacheEntry {
            document_id: "doc-1".to_string(),
            revision: "rev-1".to_string(),
            title: "Example Title".to_string(),
            url: "https://example.com".to_string(),
            byline: "Example Author".to_string(),
            excerpt: "Example excerpt".to_string(),
            site_name: "Example Site".to_string(),
            full_markdown: Arc::<str>::from(full_markdown),
        }
    }

    #[test]
    fn test_get_markdown_params_default() {
        let params = GetMarkdownParams::default();

        assert_eq!(params.page, 1);
        assert_eq!(params.page_size, 100_000);
    }

    #[test]
    fn test_paginate_markdown_first_page_includes_title_and_more_pages_notice() {
        let entry = sample_entry("abcdefghij");
        let output = paginate_markdown(
            &entry,
            &GetMarkdownParams {
                page: 1,
                page_size: 4,
            },
        )
        .expect("pagination should succeed");

        assert_eq!(output.current_page, 1);
        assert_eq!(output.total_pages, 3);
        assert!(output.has_more_pages);
        assert!(output.markdown.starts_with("# Example Title"));
        assert!(output.markdown.contains("Page 1 of 3"));
        assert!(output.markdown.contains("2 more page(s)"));
    }

    #[test]
    fn test_paginate_markdown_clamps_to_last_page_without_title_prefix() {
        let entry = sample_entry("abcdefghij");
        let output = paginate_markdown(
            &entry,
            &GetMarkdownParams {
                page: 99,
                page_size: 4,
            },
        )
        .expect("pagination should succeed");

        assert_eq!(output.current_page, 3);
        assert_eq!(output.total_pages, 3);
        assert!(!output.has_more_pages);
        assert!(!output.markdown.starts_with("# Example Title"));
        assert!(output.markdown.contains("ij"));
        assert!(output.markdown.contains("This is the last page"));
    }

    #[test]
    fn test_paginate_markdown_empty_content_still_returns_single_page() {
        let entry = sample_entry("");
        let output = paginate_markdown(
            &entry,
            &GetMarkdownParams {
                page: 1,
                page_size: 10,
            },
        )
        .expect("pagination should succeed");

        assert_eq!(output.current_page, 1);
        assert_eq!(output.total_pages, 1);
        assert!(!output.has_more_pages);
        assert!(output.markdown.starts_with("# Example Title"));
    }

    #[test]
    fn test_paginate_markdown_rejects_zero_page_size() {
        let entry = sample_entry("abc");
        let err = paginate_markdown(
            &entry,
            &GetMarkdownParams {
                page: 1,
                page_size: 0,
            },
        )
        .expect_err("zero page_size should be rejected");

        assert!(matches!(err, BrowserError::InvalidArgument(_)));
    }

    #[test]
    fn test_paginate_markdown_uses_character_boundaries_for_utf8_content() {
        let entry = sample_entry("a😀bc");
        let output = paginate_markdown(
            &entry,
            &GetMarkdownParams {
                page: 2,
                page_size: 2,
            },
        )
        .expect("pagination should succeed");

        assert_eq!(output.current_page, 2);
        assert_eq!(output.total_pages, 2);
        assert!(output.markdown.starts_with("bc"));
        assert!(output.markdown.contains("This is the last page"));
    }
}
