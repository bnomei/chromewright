use crate::browser::session::MarkdownCacheEntry;
use crate::error::{BrowserError, Result};
use crate::tools::html_to_markdown::convert_html_to_markdown;
use crate::tools::markdown::{GetMarkdownOutput, GetMarkdownParams};
use crate::tools::readability_script::READABILITY_SCRIPT;
use crate::tools::{ToolContext, ToolResult};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub(crate) fn execute_get_markdown(
    params: GetMarkdownParams,
    context: &mut ToolContext,
) -> Result<ToolResult> {
    params.validate()?;
    context.record_browser_evaluation();
    let document = context.session.document_metadata()?;
    if let Some(entry) = context.session.markdown_cache_entry(&document)? {
        return Ok(context.finish(ToolResult::success_with(paginate_markdown(
            entry.as_ref(),
            &params,
        )?)));
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
        return Ok(context.finish(ToolResult::success_with(paginate_markdown(
            entry.as_ref(),
            &params,
        )?)));
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

    let entry = Arc::new(MarkdownCacheEntry {
        document_id: document.document_id,
        revision: document.revision,
        title: extraction_result.title,
        url: extraction_result.url,
        byline: extraction_result.byline,
        excerpt: extraction_result.excerpt,
        site_name: extraction_result.site_name,
        full_markdown: Arc::<str>::from(convert_html_to_markdown(&extraction_result.content)),
    });
    context.session.store_markdown_cache(Arc::clone(&entry))?;

    Ok(context.finish(ToolResult::success_with(paginate_markdown(
        entry.as_ref(),
        &params,
    )?)))
}

pub(crate) fn paginate_markdown(
    entry: &MarkdownCacheEntry,
    params: &GetMarkdownParams,
) -> Result<GetMarkdownOutput> {
    params.validate()?;

    let total_chars = entry.full_markdown.chars().count();
    let total_pages = if entry.full_markdown.is_empty() {
        1
    } else {
        total_chars.div_ceil(params.page_size)
    };

    let current_page = params.page.clamp(1, total_pages.max(1));
    let start_char = (current_page - 1) * params.page_size;
    let end_char = (start_char + params.page_size).min(total_chars);
    let start_idx = byte_index_for_char_offset(&entry.full_markdown, start_char);
    let end_idx = byte_index_for_char_offset(&entry.full_markdown, end_char);

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

fn byte_index_for_char_offset(content: &str, char_offset: usize) -> usize {
    if char_offset == 0 {
        return 0;
    }

    content
        .char_indices()
        .nth(char_offset)
        .map(|(index, _)| index)
        .unwrap_or(content.len())
}

fn markdown_extraction_script() -> &'static str {
    static SCRIPT: OnceLock<String> = OnceLock::new();
    SCRIPT.get_or_init(|| {
        format!(
            "var READABILITY_SCRIPT = {};\n{}",
            serde_json::to_string(READABILITY_SCRIPT)
                .expect("Readability script serialization should never fail"),
            include_str!("../convert_to_markdown.js")
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{ScriptEvaluation, SessionBackend, TabDescriptor};
    use crate::dom::{DocumentMetadata, DomTree};
    use std::any::Any;

    struct ReadyWaitFailureBackend;

    impl SessionBackend for ReadyWaitFailureBackend {
        fn as_any(&self) -> &dyn Any {
            self
        }

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
            unreachable!("list_tabs is not used in this test")
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

    impl SessionBackend for InvalidMarkdownSettleBackend {
        fn as_any(&self) -> &dyn Any {
            self
        }

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
            unreachable!("list_tabs is not used in this test")
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
}
