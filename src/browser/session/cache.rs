use super::BrowserSession;
use crate::error::{BrowserError, Result};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct MarkdownCacheEntry {
    pub document_id: String,
    pub revision: String,
    pub title: String,
    pub url: String,
    pub byline: String,
    pub excerpt: String,
    pub site_name: String,
    pub full_markdown: Arc<str>,
}

impl BrowserSession {
    pub(crate) fn markdown_cache_entry(
        &self,
        document: &crate::dom::DocumentMetadata,
    ) -> Result<Option<Arc<MarkdownCacheEntry>>> {
        let guard = self
            .markdown_cache
            .lock()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "get_markdown".to_string(),
                reason: format!("Failed to read markdown cache: {}", e),
            })?;

        Ok(guard.as_ref().and_then(|entry| {
            (entry.document_id == document.document_id && entry.revision == document.revision)
                .then_some(Arc::clone(entry))
        }))
    }

    pub(crate) fn store_markdown_cache(&self, entry: Arc<MarkdownCacheEntry>) -> Result<()> {
        *self
            .markdown_cache
            .lock()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "get_markdown".to_string(),
                reason: format!("Failed to write markdown cache: {}", e),
            })? = Some(entry);
        Ok(())
    }
}
