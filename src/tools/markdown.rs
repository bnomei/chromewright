use crate::error::{BrowserError, Result};
use crate::tools::{
    DocumentResult, Tool, ToolContext, ToolResult, services::markdown::execute_get_markdown,
};
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
pub struct GetMarkdownOutput {
    #[serde(flatten)]
    pub result: DocumentResult,
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
        "Read page content as markdown. For actions or precise nodes, use snapshot."
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
        MarkdownCacheEntry::new(
            "doc-1".to_string(),
            "rev-1".to_string(),
            "Example Title".to_string(),
            "https://example.com".to_string(),
            "Example Author".to_string(),
            "Example excerpt".to_string(),
            "Example Site".to_string(),
            Arc::<str>::from(full_markdown),
        )
    }

    fn sample_utf8_markdown(repetitions: usize) -> String {
        "😀 cafe résumé λ漢字🚀naive ".repeat(repetitions)
    }

    fn expected_paginated_markdown(
        title: &str,
        content: &str,
        page: usize,
        page_size: usize,
    ) -> String {
        let total_chars = content.chars().count();
        let total_pages = if content.is_empty() {
            1
        } else {
            total_chars.div_ceil(page_size)
        };
        let current_page = page.clamp(1, total_pages.max(1));
        let start_char = (current_page - 1) * page_size;
        let end_char = (start_char + page_size).min(total_chars);

        let page_body = content
            .chars()
            .skip(start_char)
            .take(end_char - start_char)
            .collect::<String>();

        let mut expected = if current_page == 1 && !title.is_empty() {
            format!("# {}\n\n{}", title, page_body)
        } else {
            page_body
        };

        if total_pages > 1 {
            let footer = if current_page < total_pages {
                format!(
                    "\n\n---\n\n*Page {} of {}. There are {} more page(s) with additional content.*\n",
                    current_page,
                    total_pages,
                    total_pages - current_page
                )
            } else {
                format!(
                    "\n\n---\n\n*Page {} of {}. This is the last page.*\n",
                    current_page, total_pages
                )
            };
            expected.push_str(&footer);
        }

        expected
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

    #[test]
    fn test_paginate_markdown_repeated_page_reads_return_same_output_for_cached_entry() {
        let content = sample_utf8_markdown(300);
        let entry = sample_entry(&content);
        let params = GetMarkdownParams {
            page: 2,
            page_size: 73,
        };

        let first = paginate_markdown(&entry, &params).expect("first pagination should succeed");
        let second = paginate_markdown(&entry, &params).expect("second pagination should succeed");
        let expected =
            expected_paginated_markdown(&entry.title, &content, params.page, params.page_size);
        let expected_total_pages = content.chars().count().div_ceil(params.page_size);

        assert_eq!(first.markdown, second.markdown);
        assert_eq!(first.markdown, expected);
        assert_eq!(first.current_page, 2);
        assert_eq!(first.total_pages, expected_total_pages);
        assert!(first.has_more_pages);
        assert!(!first.markdown.starts_with("# Example Title"));
        assert!(first.markdown.contains("Page 2 of"));
        assert!(
            first
                .markdown
                .contains("more page(s) with additional content")
        );
    }

    #[test]
    fn test_paginate_markdown_utf8_first_page_preserves_title_and_footer_contract() {
        let content = sample_utf8_markdown(160);
        let entry = sample_entry(&content);
        let params = GetMarkdownParams {
            page: 1,
            page_size: 65,
        };

        let output = paginate_markdown(&entry, &params).expect("pagination should succeed");
        let expected =
            expected_paginated_markdown(&entry.title, &content, params.page, params.page_size);

        assert_eq!(output.current_page, 1);
        assert_eq!(output.markdown, expected);
        assert!(output.markdown.starts_with("# Example Title\n\n"));
        assert!(output.markdown.contains("Page 1 of"));
        assert!(output.has_more_pages);
    }

    #[test]
    fn test_paginate_markdown_utf8_mid_page_matches_expected_slice_exactly() {
        let content = sample_utf8_markdown(220);
        let entry = sample_entry(&content);
        let params = GetMarkdownParams {
            page: 3,
            page_size: 64,
        };

        let output = paginate_markdown(&entry, &params).expect("pagination should succeed");
        let expected =
            expected_paginated_markdown(&entry.title, &content, params.page, params.page_size);
        let expected_body = content
            .chars()
            .skip((params.page - 1) * params.page_size)
            .take(params.page_size)
            .collect::<String>();

        assert_eq!(output.markdown, expected);
        assert!(output.markdown.starts_with(&expected_body));
        assert!(!output.markdown.starts_with("# Example Title"));
        assert!(output.markdown.contains("Page 3 of"));
    }
}
