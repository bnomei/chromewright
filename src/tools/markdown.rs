use crate::browser::session::MarkdownCacheEntry;
use crate::error::{BrowserError, Result};
use crate::tools::html_to_markdown::convert_html_to_markdown;
use crate::tools::readability_script::READABILITY_SCRIPT;
use crate::tools::{Tool, ToolContext, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Parameters for getting markdown content with pagination support
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetMarkdownParams {
    /// Page number to extract (1-based index, default: 1)
    #[serde(default = "default_page")]
    pub page: usize,

    /// Maximum characters per page (default: 100000)
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

    fn execute_typed(
        &self,
        params: GetMarkdownParams,
        context: &mut ToolContext,
    ) -> Result<ToolResult> {
        let document = context.session.document_metadata()?;
        if let Some(entry) = context.session.markdown_cache_entry(&document)? {
            return Ok(ToolResult::success_with(paginate_markdown(&entry, &params)));
        }

        if document.ready_state != "complete" {
            context
                .session
                .wait_for_document_ready_with_timeout(std::time::Duration::from_secs(5))
                .ok();
        }
        wait_for_markdown_settle(context, Duration::from_secs(2))?;

        let document = context.session.document_metadata()?;
        if let Some(entry) = context.session.markdown_cache_entry(&document)? {
            return Ok(ToolResult::success_with(paginate_markdown(&entry, &params)));
        }

        let extraction_result = extract_markdown(context)?;

        if extraction_result.readability_failed {
            return Err(BrowserError::ToolExecutionFailed {
                tool: "get_markdown".to_string(),
                reason: extraction_result
                    .error
                    .unwrap_or_else(|| "Readability extraction failed".to_string()),
            });
        }

        let entry = MarkdownCacheEntry {
            document_id: document.document_id,
            revision: document.revision,
            title: extraction_result.title,
            url: extraction_result.url,
            byline: extraction_result.byline,
            excerpt: extraction_result.excerpt,
            site_name: extraction_result.site_name,
            full_markdown: convert_html_to_markdown(&extraction_result.content),
        };
        context.session.store_markdown_cache(entry.clone())?;

        Ok(ToolResult::success_with(paginate_markdown(&entry, &params)))
    }
}

fn markdown_extraction_script() -> &'static str {
    static SCRIPT: OnceLock<String> = OnceLock::new();
    SCRIPT.get_or_init(|| {
        format!(
            "var READABILITY_SCRIPT = {};\n{}",
            serde_json::to_string(READABILITY_SCRIPT)
                .expect("Readability script serialization should never fail"),
            include_str!("convert_to_markdown.js")
        )
    })
}

fn extract_markdown(context: &ToolContext) -> Result<ExtractionResult> {
    let result = context
        .session
        .tab()?
        .evaluate(markdown_extraction_script(), false)
        .map_err(|e| BrowserError::EvaluationFailed(e.to_string()))?;

    let result_value = result.value.ok_or_else(|| {
        let description = result
            .description
            .map(|d| format!("Description: {}", d))
            .unwrap_or_else(|| format!("Type: {:?}", result.Type));

        BrowserError::ToolExecutionFailed {
            tool: "get_markdown".to_string(),
            reason: format!("No value returned from JavaScript. {}", description),
        }
    })?;

    if let Some(json_str) = result_value.as_str() {
        serde_json::from_str(json_str).map_err(|e| BrowserError::ToolExecutionFailed {
            tool: "get_markdown".to_string(),
            reason: format!("Failed to parse extraction result: {}", e),
        })
    } else {
        serde_json::from_value(result_value).map_err(|e| BrowserError::ToolExecutionFailed {
            tool: "get_markdown".to_string(),
            reason: format!("Failed to deserialize extraction result: {}", e),
        })
    }
}

fn paginate_markdown(entry: &MarkdownCacheEntry, params: &GetMarkdownParams) -> GetMarkdownOutput {
    let total_pages = if entry.full_markdown.is_empty() {
        1
    } else {
        (entry.full_markdown.len() + params.page_size - 1) / params.page_size
    };

    let current_page = params.page.clamp(1, total_pages.max(1));
    let start_idx = (current_page - 1) * params.page_size;
    let end_idx = (start_idx + params.page_size).min(entry.full_markdown.len());

    let mut page_content = if start_idx < entry.full_markdown.len() {
        entry.full_markdown[start_idx..end_idx].to_string()
    } else {
        String::new()
    };

    if current_page == 1 && !entry.title.is_empty() {
        page_content = format!("# {}\n\n{}", entry.title, page_content);
    }

    if total_pages > 1 {
        let pagination_info = if current_page < total_pages {
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
        page_content.push_str(&pagination_info);
    }

    let length = page_content.len();

    GetMarkdownOutput {
        markdown: page_content,
        title: entry.title.clone(),
        url: entry.url.clone(),
        current_page,
        total_pages,
        has_more_pages: current_page < total_pages,
        length,
        byline: entry.byline.clone(),
        excerpt: entry.excerpt.clone(),
        site_name: entry.site_name.clone(),
    }
}

fn wait_for_markdown_settle(context: &ToolContext, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    let mut previous_len: Option<u64> = None;
    let mut stable_polls = 0_u8;

    loop {
        let result = context
            .session
            .tab()?
            .evaluate(
                "(() => (document.body && document.body.innerText ? document.body.innerText.length : 0))()",
                false,
            )
            .map_err(|e| BrowserError::EvaluationFailed(e.to_string()))?;
        let current_len = result.value.and_then(|value| value.as_u64()).unwrap_or(0);

        if previous_len == Some(current_len) {
            stable_polls += 1;
        } else {
            previous_len = Some(current_len);
            stable_polls = 0;
        }

        if stable_polls >= 2 {
            return Ok(());
        }

        if start.elapsed() >= timeout {
            return Ok(());
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Structure for extraction result returned from JavaScript
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtractionResult {
    title: String,
    content: String,
    text_content: String,
    url: String,
    #[serde(default)]
    excerpt: String,
    #[serde(default)]
    byline: String,
    #[serde(default)]
    site_name: String,
    #[serde(default)]
    length: usize,
    #[serde(default)]
    lang: String,
    #[serde(default)]
    dir: String,
    #[serde(default)]
    published_time: String,
    #[serde(default)]
    readability_failed: bool,
    #[serde(default)]
    error: Option<String>,
}
