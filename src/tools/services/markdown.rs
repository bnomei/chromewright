//! Readability-based main-content extraction, markdown conversion, and session cache reuse.

use crate::browser::{MarkdownCacheEntry, MarkdownCacheMetadata};
use crate::error::{BrowserError, Result};
use crate::tools::html_to_markdown::convert_html_to_markdown;
use crate::tools::limits::{
    MAX_DOM_STRING_CHARS, MAX_MARKDOWN_HTML_CHARS, validate_markdown_html_chars,
};
use crate::tools::markdown::{GetMarkdownOutput, GetMarkdownParams};
use crate::tools::readability_script::READABILITY_SCRIPT;
use crate::tools::{DocumentResult, ToolContext, ToolResult};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

fn validate_markdown_metadata_string(field: &str, value: &str) -> Result<()> {
    let char_count = value.chars().count();
    if char_count > MAX_DOM_STRING_CHARS {
        return Err(BrowserError::resource_limit_exceeded(
            "markdown_metadata_chars",
            format!(
                "markdown metadata field {field} is {char_count} characters, exceeding the {MAX_DOM_STRING_CHARS} character limit"
            ),
            format!("{MAX_DOM_STRING_CHARS} characters"),
            format!("{char_count} characters"),
        ));
    }

    Ok(())
}

fn validate_markdown_extraction_metadata(result: &ExtractionResult) -> Result<()> {
    validate_markdown_metadata_string("title", &result.title)?;
    validate_markdown_metadata_string("url", &result.url)?;
    validate_markdown_metadata_string("excerpt", &result.excerpt)?;
    validate_markdown_metadata_string("byline", &result.byline)?;
    validate_markdown_metadata_string("site_name", &result.site_name)?;
    validate_markdown_metadata_string("lang", &result.lang)?;
    validate_markdown_metadata_string("dir", &result.dir)?;
    validate_markdown_metadata_string("published_time", &result.published_time)
}

fn validate_markdown_text_chars(char_count: usize) -> Result<()> {
    if char_count > MAX_MARKDOWN_HTML_CHARS {
        return Err(BrowserError::resource_limit_exceeded(
            "markdown_text_chars",
            format!(
                "markdown text input is {char_count} characters, exceeding the {MAX_MARKDOWN_HTML_CHARS} character limit"
            ),
            format!("{MAX_MARKDOWN_HTML_CHARS} characters"),
            format!("{char_count} characters"),
        ));
    }

    Ok(())
}

/// Extract main-content markdown (with session cache reuse), then return one page of text.
///
/// Waits for document readiness and a short settle window when the cache misses, runs
/// Readability in-page, converts HTML to markdown, and stores a revision-keyed cache entry.
pub(crate) fn execute_get_markdown(
    params: GetMarkdownParams,
    context: &mut ToolContext,
) -> Result<ToolResult> {
    params.validate()?;
    context.record_browser_evaluation();
    let document = context.session.document_metadata()?;
    if let Some(entry) = context.session.markdown_cache_entry(&document)? {
        let mut output = paginate_markdown(entry.as_ref(), &params)?;
        output.result = DocumentResult::new(document);
        return Ok(context.finish(ToolResult::success_with(output)));
    }

    if document.ready_state != "complete" {
        context
            .session
            .wait_for_document_ready_with_timeout(std::time::Duration::from_secs(5))?;
    }
    wait_for_markdown_settle(context, Duration::from_secs(2))?;

    context.record_browser_evaluation();
    let document = context.session.document_metadata()?;
    if let Some(entry) = context.session.markdown_cache_entry(&document)? {
        let mut output = paginate_markdown(entry.as_ref(), &params)?;
        output.result = DocumentResult::new(document);
        return Ok(context.finish(ToolResult::success_with(output)));
    }

    let extraction_result = extract_markdown(context)?;

    if extraction_result.resource_limit_exceeded {
        let char_count = extraction_result
            .char_count
            .unwrap_or(MAX_MARKDOWN_HTML_CHARS + 1);
        return Err(BrowserError::resource_limit_exceeded(
            "markdown_html_chars",
            extraction_result.error.unwrap_or_else(|| {
                format!(
                    "markdown HTML input is {char_count} characters, exceeding the {MAX_MARKDOWN_HTML_CHARS} character limit"
                )
            }),
            format!("{MAX_MARKDOWN_HTML_CHARS} characters"),
            format!("{char_count} characters"),
        ));
    }
    if extraction_result.readability_failed {
        return Err(BrowserError::ToolExecutionFailed {
            tool: "get_markdown".to_string(),
            reason: extraction_result
                .error
                .unwrap_or_else(|| "Readability extraction failed".to_string()),
        });
    }
    validate_markdown_html_chars(extraction_result.content.chars().count())?;
    validate_markdown_text_chars(extraction_result.text_content.chars().count())?;
    validate_markdown_extraction_metadata(&extraction_result)?;

    let entry = Arc::new(MarkdownCacheEntry::new(
        MarkdownCacheMetadata {
            document_id: document.document_id.clone(),
            revision: document.revision.clone(),
            title: extraction_result.title,
            url: extraction_result.url,
            byline: extraction_result.byline,
            excerpt: extraction_result.excerpt,
            site_name: extraction_result.site_name,
        },
        Arc::<str>::from(convert_html_to_markdown(&extraction_result.content)),
    ));
    context.session.store_markdown_cache(Arc::clone(&entry))?;

    let mut output = paginate_markdown(entry.as_ref(), &params)?;
    output.result = DocumentResult::new(document);
    Ok(context.finish(ToolResult::success_with(output)))
}

/// Slice a cached full-markdown entry into a page-sized window with checkpoint metadata.
pub(crate) fn paginate_markdown(
    entry: &MarkdownCacheEntry,
    params: &GetMarkdownParams,
) -> Result<GetMarkdownOutput> {
    params.validate()?;

    let total_chars = entry.pagination_total_chars();
    let total_pages = if entry.full_markdown.is_empty() {
        1
    } else {
        total_chars.div_ceil(params.page_size)
    };

    let current_page = params.page.clamp(1, total_pages.max(1));
    let start_char = (current_page - 1) * params.page_size;
    let end_char = (start_char + params.page_size).min(total_chars);
    let start_idx = byte_index_for_char_offset(entry, start_char);
    let end_idx = byte_index_for_char_offset(entry, end_char);

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

    Ok(GetMarkdownOutput {
        result: DocumentResult::new(crate::dom::DocumentMetadata {
            document_id: entry.document_id.clone(),
            revision: entry.revision.clone(),
            url: entry.url.clone(),
            title: entry.title.clone(),
            ready_state: "complete".to_string(),
            frames: Vec::new(),
        }),
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
    })
}

fn byte_index_for_char_offset(entry: &MarkdownCacheEntry, char_offset: usize) -> usize {
    let content = entry.full_markdown.as_ref();

    if char_offset == 0 {
        return 0;
    }

    if char_offset >= entry.pagination_total_chars() {
        return content.len();
    }

    let (checkpoint_char_offset, checkpoint_byte_offset) = entry.pagination_checkpoint(char_offset);
    let local_char_offset = char_offset - checkpoint_char_offset;

    if local_char_offset == 0 {
        return checkpoint_byte_offset;
    }

    checkpoint_byte_offset
        + content[checkpoint_byte_offset..]
            .char_indices()
            .nth(local_char_offset)
            .map(|(index, _)| index)
            .unwrap_or(content.len() - checkpoint_byte_offset)
}

fn markdown_extraction_script() -> &'static str {
    static SCRIPT: OnceLock<String> = OnceLock::new();
    SCRIPT.get_or_init(|| {
        let extraction_script = include_str!("../convert_to_markdown.js").replace(
            "__MARKDOWN_MAX_HTML_CHARS__",
            &MAX_MARKDOWN_HTML_CHARS.to_string(),
        );
        format!(
            "var READABILITY_SCRIPT = {};\n{}",
            serde_json::to_string(READABILITY_SCRIPT)
                .expect("Readability script serialization should never fail"),
            extraction_script
        )
    })
}

fn extract_markdown(context: &mut ToolContext) -> Result<ExtractionResult> {
    context.record_browser_evaluation();
    let result = context
        .session
        .evaluate(markdown_extraction_script(), false)?;

    let result_value = result.value.ok_or_else(|| {
        let description = result
            .description
            .map(|d| format!("Description: {}", d))
            .unwrap_or_else(|| {
                format!(
                    "Type: {}",
                    result.type_name.unwrap_or_else(|| "unknown".to_string())
                )
            });

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

fn wait_for_markdown_settle(context: &mut ToolContext, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    let mut previous_len: Option<u64> = None;
    let mut stable_polls = 0_u8;

    loop {
        context.record_poll_iteration();
        context.record_browser_evaluation();
        let result = context.session.evaluate(
            "(() => (document.body && document.body.textContent ? document.body.textContent.length : 0))()",
            false,
        )?;
        let current_len = parse_markdown_text_length(result)?;

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

fn parse_markdown_text_length(result: crate::browser::backend::ScriptEvaluation) -> Result<u64> {
    let Some(value) = result.value else {
        return Err(BrowserError::ToolExecutionFailed {
            tool: "get_markdown".to_string(),
            reason: "Markdown settle probe returned no value".to_string(),
        });
    };

    value
        .as_u64()
        .ok_or_else(|| BrowserError::ToolExecutionFailed {
            tool: "get_markdown".to_string(),
            reason: format!(
                "Markdown settle probe returned a non-numeric body length ({})",
                json_type_name(&value)
            ),
        })
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
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
    #[serde(default)]
    resource_limit_exceeded: bool,
    #[serde(default)]
    char_count: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{ScriptEvaluation, SessionBackend, TabDescriptor};
    use crate::dom::{DocumentMetadata, DomTree};
    use crate::tools::limits::{MAX_DOM_STRING_CHARS, MAX_MARKDOWN_HTML_CHARS};

    struct ReadyWaitFailureBackend;

    impl SessionBackend for ReadyWaitFailureBackend {
        fn navigate(&self, _url: &str) -> Result<()> {
            unreachable!("navigate is not used in this test")
        }

        fn wait_for_navigation(&self) -> Result<()> {
            unreachable!("wait_for_navigation is not used in this test")
        }

        fn wait_for_document_ready_with_timeout(&self, _timeout: Duration) -> Result<()> {
            Err(BrowserError::Timeout(
                "document never reached ready state".to_string(),
            ))
        }

        fn document_metadata(&self) -> Result<DocumentMetadata> {
            Ok(DocumentMetadata {
                ready_state: "loading".to_string(),
                ..DocumentMetadata::default()
            })
        }

        fn extract_dom(&self) -> Result<DomTree> {
            unreachable!("extract_dom is not used in this test")
        }

        fn extract_dom_with_prefix(&self, _prefix: &str) -> Result<DomTree> {
            unreachable!("extract_dom_with_prefix is not used in this test")
        }

        fn evaluate(&self, _script: &str, _await_promise: bool) -> Result<ScriptEvaluation> {
            unreachable!("evaluate is not used in this test")
        }

        fn capture_screenshot(&self, _full_page: bool) -> Result<Vec<u8>> {
            unreachable!("capture_screenshot is not used in this test")
        }

        fn press_key(&self, _key: &str) -> Result<()> {
            unreachable!("press_key is not used in this test")
        }

        fn list_tabs(&self) -> Result<Vec<TabDescriptor>> {
            Ok(vec![TabDescriptor {
                id: "tab-1".to_string(),
                title: "Test Tab".to_string(),
                url: "about:blank".to_string(),
            }])
        }

        fn active_tab(&self) -> Result<TabDescriptor> {
            unreachable!("active_tab is not used in this test")
        }

        fn open_tab(&self, _url: &str) -> Result<TabDescriptor> {
            unreachable!("open_tab is not used in this test")
        }

        fn activate_tab(&self, _tab_id: &str) -> Result<()> {
            unreachable!("activate_tab is not used in this test")
        }

        fn close_tab(&self, _tab_id: &str, _with_unload: bool) -> Result<()> {
            unreachable!("close_tab is not used in this test")
        }

        fn close(&self) -> Result<()> {
            unreachable!("close is not used in this test")
        }
    }

    struct InvalidMarkdownSettleBackend;

    enum MarkdownExtractionPayload {
        ResourceLimit,
        OversizedContent,
        OversizedMetadata,
        OversizedTextContent,
    }

    struct MarkdownExtractionBackend {
        payload: MarkdownExtractionPayload,
    }

    impl SessionBackend for InvalidMarkdownSettleBackend {
        fn navigate(&self, _url: &str) -> Result<()> {
            unreachable!("navigate is not used in this test")
        }

        fn wait_for_navigation(&self) -> Result<()> {
            unreachable!("wait_for_navigation is not used in this test")
        }

        fn wait_for_document_ready_with_timeout(&self, _timeout: Duration) -> Result<()> {
            unreachable!("wait_for_document_ready_with_timeout is not used in this test")
        }

        fn document_metadata(&self) -> Result<DocumentMetadata> {
            unreachable!("document_metadata is not used in this test")
        }

        fn extract_dom(&self) -> Result<DomTree> {
            unreachable!("extract_dom is not used in this test")
        }

        fn extract_dom_with_prefix(&self, _prefix: &str) -> Result<DomTree> {
            unreachable!("extract_dom_with_prefix is not used in this test")
        }

        fn evaluate(&self, _script: &str, _await_promise: bool) -> Result<ScriptEvaluation> {
            Ok(ScriptEvaluation {
                value: Some(serde_json::Value::String("eleven".to_string())),
                description: None,
                type_name: Some("String".to_string()),
            })
        }

        fn capture_screenshot(&self, _full_page: bool) -> Result<Vec<u8>> {
            unreachable!("capture_screenshot is not used in this test")
        }

        fn press_key(&self, _key: &str) -> Result<()> {
            unreachable!("press_key is not used in this test")
        }

        fn list_tabs(&self) -> Result<Vec<TabDescriptor>> {
            Ok(vec![TabDescriptor {
                id: "tab-1".to_string(),
                title: "Test Tab".to_string(),
                url: "about:blank".to_string(),
            }])
        }

        fn active_tab(&self) -> Result<TabDescriptor> {
            unreachable!("active_tab is not used in this test")
        }

        fn open_tab(&self, _url: &str) -> Result<TabDescriptor> {
            unreachable!("open_tab is not used in this test")
        }

        fn activate_tab(&self, _tab_id: &str) -> Result<()> {
            unreachable!("activate_tab is not used in this test")
        }

        fn close_tab(&self, _tab_id: &str, _with_unload: bool) -> Result<()> {
            unreachable!("close_tab is not used in this test")
        }

        fn close(&self) -> Result<()> {
            unreachable!("close is not used in this test")
        }
    }

    impl SessionBackend for MarkdownExtractionBackend {
        fn navigate(&self, _url: &str) -> Result<()> {
            unreachable!("navigate is not used in this test")
        }

        fn wait_for_navigation(&self) -> Result<()> {
            unreachable!("wait_for_navigation is not used in this test")
        }

        fn wait_for_document_ready_with_timeout(&self, _timeout: Duration) -> Result<()> {
            unreachable!("wait_for_document_ready_with_timeout is not used in this test")
        }

        fn document_metadata(&self) -> Result<DocumentMetadata> {
            Ok(DocumentMetadata {
                document_id: "doc-markdown".to_string(),
                revision: "rev-1".to_string(),
                url: "https://example.test".to_string(),
                title: "Example".to_string(),
                ready_state: "complete".to_string(),
                frames: Vec::new(),
            })
        }

        fn extract_dom(&self) -> Result<DomTree> {
            unreachable!("extract_dom is not used in this test")
        }

        fn extract_dom_with_prefix(&self, _prefix: &str) -> Result<DomTree> {
            unreachable!("extract_dom_with_prefix is not used in this test")
        }

        fn evaluate(&self, script: &str, _await_promise: bool) -> Result<ScriptEvaluation> {
            if script.contains("document.body && document.body.textContent") {
                return Ok(ScriptEvaluation {
                    value: Some(serde_json::json!(0)),
                    description: None,
                    type_name: Some("Number".to_string()),
                });
            }

            let value = match self.payload {
                MarkdownExtractionPayload::ResourceLimit => serde_json::json!({
                    "title": "Example",
                    "content": "",
                    "textContent": "",
                    "url": "https://example.test",
                    "resourceLimitExceeded": true,
                    "charCount": MAX_MARKDOWN_HTML_CHARS + 1,
                    "readabilityFailed": false,
                    "error": "markdown HTML input exceeds the character limit"
                }),
                MarkdownExtractionPayload::OversizedContent => serde_json::json!({
                    "title": "Example",
                    "content": "x".repeat(MAX_MARKDOWN_HTML_CHARS + 1),
                    "textContent": "",
                    "url": "https://example.test",
                    "readabilityFailed": false
                }),
                MarkdownExtractionPayload::OversizedMetadata => serde_json::json!({
                    "title": "x".repeat(MAX_DOM_STRING_CHARS + 1),
                    "content": "<main><p>bounded</p></main>",
                    "textContent": "bounded",
                    "url": "https://example.test",
                    "readabilityFailed": false
                }),
                MarkdownExtractionPayload::OversizedTextContent => serde_json::json!({
                    "title": "Example",
                    "content": "<main><p>bounded</p></main>",
                    "textContent": "x".repeat(MAX_MARKDOWN_HTML_CHARS + 1),
                    "url": "https://example.test",
                    "readabilityFailed": false
                }),
            };

            Ok(ScriptEvaluation {
                value: Some(serde_json::Value::String(value.to_string())),
                description: None,
                type_name: Some("String".to_string()),
            })
        }

        fn capture_screenshot(&self, _full_page: bool) -> Result<Vec<u8>> {
            unreachable!("capture_screenshot is not used in this test")
        }

        fn press_key(&self, _key: &str) -> Result<()> {
            unreachable!("press_key is not used in this test")
        }

        fn list_tabs(&self) -> Result<Vec<TabDescriptor>> {
            Ok(vec![TabDescriptor {
                id: "tab-1".to_string(),
                title: "Example".to_string(),
                url: "https://example.test".to_string(),
            }])
        }

        fn active_tab(&self) -> Result<TabDescriptor> {
            Ok(TabDescriptor {
                id: "tab-1".to_string(),
                title: "Example".to_string(),
                url: "https://example.test".to_string(),
            })
        }

        fn open_tab(&self, _url: &str) -> Result<TabDescriptor> {
            unreachable!("open_tab is not used in this test")
        }

        fn activate_tab(&self, _tab_id: &str) -> Result<()> {
            unreachable!("activate_tab is not used in this test")
        }

        fn close_tab(&self, _tab_id: &str, _with_unload: bool) -> Result<()> {
            unreachable!("close_tab is not used in this test")
        }

        fn close(&self) -> Result<()> {
            unreachable!("close is not used in this test")
        }
    }

    #[test]
    fn test_execute_get_markdown_propagates_document_ready_wait_errors() {
        let session = BrowserSession::with_test_backend(ReadyWaitFailureBackend);
        let mut context = ToolContext::new(&session);
        let err = execute_get_markdown(GetMarkdownParams::default(), &mut context)
            .expect_err("document ready failures should propagate");

        match err {
            BrowserError::Timeout(reason) => {
                assert!(reason.contains("never reached ready state"));
            }
            other => panic!("unexpected markdown readiness error: {other:?}"),
        }
    }

    #[test]
    fn test_wait_for_markdown_settle_rejects_non_numeric_payloads() {
        let session = BrowserSession::with_test_backend(InvalidMarkdownSettleBackend);
        let mut context = ToolContext::new(&session);
        let err = wait_for_markdown_settle(&mut context, Duration::from_millis(10))
            .expect_err("invalid settle payloads should fail");

        match err {
            BrowserError::ToolExecutionFailed { tool, reason } => {
                assert_eq!(tool, "get_markdown");
                assert!(reason.contains("non-numeric body length"));
                assert!(reason.contains("string"));
            }
            other => panic!("unexpected settle probe error: {other:?}"),
        }
    }

    #[test]
    fn test_execute_get_markdown_rejects_resource_limit_payload() {
        let session = BrowserSession::with_test_backend(MarkdownExtractionBackend {
            payload: MarkdownExtractionPayload::ResourceLimit,
        });
        let mut context = ToolContext::new(&session);

        let err = execute_get_markdown(GetMarkdownParams::default(), &mut context)
            .expect_err("resource limit payload should fail closed");

        match err {
            BrowserError::ResourceLimitExceeded(details) => {
                assert_eq!(details.resource, "markdown_html_chars");
                assert_eq!(
                    details.limit,
                    format!("{MAX_MARKDOWN_HTML_CHARS} characters")
                );
                assert_eq!(
                    details.actual,
                    format!("{} characters", MAX_MARKDOWN_HTML_CHARS + 1)
                );
            }
            other => panic!("unexpected markdown resource limit error: {other:?}"),
        }
    }

    #[test]
    fn test_execute_get_markdown_defensively_rejects_oversized_html_content() {
        let session = BrowserSession::with_test_backend(MarkdownExtractionBackend {
            payload: MarkdownExtractionPayload::OversizedContent,
        });
        let mut context = ToolContext::new(&session);

        let err = execute_get_markdown(GetMarkdownParams::default(), &mut context)
            .expect_err("oversized extracted HTML should fail closed");

        match err {
            BrowserError::ResourceLimitExceeded(details) => {
                assert_eq!(details.resource, "markdown_html_chars");
                assert_eq!(
                    details.actual,
                    format!("{} characters", MAX_MARKDOWN_HTML_CHARS + 1)
                );
            }
            other => panic!("unexpected markdown oversized content error: {other:?}"),
        }
    }

    #[test]
    fn test_execute_get_markdown_rejects_oversized_metadata() {
        let session = BrowserSession::with_test_backend(MarkdownExtractionBackend {
            payload: MarkdownExtractionPayload::OversizedMetadata,
        });
        let mut context = ToolContext::new(&session);

        let err = execute_get_markdown(GetMarkdownParams::default(), &mut context)
            .expect_err("oversized markdown metadata should fail closed");

        match err {
            BrowserError::ResourceLimitExceeded(details) => {
                assert_eq!(details.resource, "markdown_metadata_chars");
                assert_eq!(details.limit, format!("{MAX_DOM_STRING_CHARS} characters"));
                assert_eq!(
                    details.actual,
                    format!("{} characters", MAX_DOM_STRING_CHARS + 1)
                );
            }
            other => panic!("unexpected markdown metadata error: {other:?}"),
        }
    }

    #[test]
    fn test_execute_get_markdown_rejects_oversized_text_content() {
        let session = BrowserSession::with_test_backend(MarkdownExtractionBackend {
            payload: MarkdownExtractionPayload::OversizedTextContent,
        });
        let mut context = ToolContext::new(&session);

        let err = execute_get_markdown(GetMarkdownParams::default(), &mut context)
            .expect_err("oversized markdown text content should fail closed");

        match err {
            BrowserError::ResourceLimitExceeded(details) => {
                assert_eq!(details.resource, "markdown_text_chars");
                assert_eq!(
                    details.limit,
                    format!("{MAX_MARKDOWN_HTML_CHARS} characters")
                );
                assert_eq!(
                    details.actual,
                    format!("{} characters", MAX_MARKDOWN_HTML_CHARS + 1)
                );
            }
            other => panic!("unexpected markdown text content error: {other:?}"),
        }
    }
}
