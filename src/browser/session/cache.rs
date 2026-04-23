use super::BrowserSession;
use crate::browser::backend::{
    ScreenshotCapture, ScreenshotClip, ScreenshotFormat, ScreenshotMode,
};
use crate::dom::{DocumentMetadata, SnapshotNode};
use crate::error::{BrowserError, Result};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SCREENSHOT_ARTIFACT_LIMIT: usize = 8;
static SCREENSHOT_ARTIFACT_COUNTER: AtomicU64 = AtomicU64::new(1);
const MARKDOWN_PAGINATION_CHECKPOINT_INTERVAL: usize = 4_096;

#[derive(Debug, Clone)]
struct MarkdownPaginationMetadata {
    total_chars: usize,
    checkpoint_interval: usize,
    checkpoint_byte_offsets: Arc<[usize]>,
}

impl MarkdownPaginationMetadata {
    fn build(content: &str) -> Self {
        let mut checkpoint_byte_offsets = vec![0];
        let mut total_chars = 0;

        for (char_index, (byte_offset, _)) in content.char_indices().enumerate() {
            if char_index > 0 && char_index % MARKDOWN_PAGINATION_CHECKPOINT_INTERVAL == 0 {
                checkpoint_byte_offsets.push(byte_offset);
            }
            total_chars = char_index + 1;
        }

        Self {
            total_chars,
            checkpoint_interval: MARKDOWN_PAGINATION_CHECKPOINT_INTERVAL,
            checkpoint_byte_offsets: checkpoint_byte_offsets.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MarkdownCacheMetadata {
    pub document_id: String,
    pub revision: String,
    pub title: String,
    pub url: String,
    pub byline: String,
    pub excerpt: String,
    pub site_name: String,
}

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
    pagination: MarkdownPaginationMetadata,
}

impl MarkdownCacheEntry {
    pub(crate) fn new(metadata: MarkdownCacheMetadata, full_markdown: Arc<str>) -> Self {
        Self {
            document_id: metadata.document_id,
            revision: metadata.revision,
            title: metadata.title,
            url: metadata.url,
            byline: metadata.byline,
            excerpt: metadata.excerpt,
            site_name: metadata.site_name,
            pagination: MarkdownPaginationMetadata::build(&full_markdown),
            full_markdown,
        }
    }

    pub(crate) fn pagination_total_chars(&self) -> usize {
        self.pagination.total_chars
    }

    pub(crate) fn pagination_checkpoint(&self, char_offset: usize) -> (usize, usize) {
        let checkpoint_index = (char_offset / self.pagination.checkpoint_interval).min(
            self.pagination
                .checkpoint_byte_offsets
                .len()
                .saturating_sub(1),
        );
        let checkpoint_char_offset = checkpoint_index * self.pagination.checkpoint_interval;
        let checkpoint_byte_offset = self.pagination.checkpoint_byte_offsets[checkpoint_index];
        (checkpoint_char_offset, checkpoint_byte_offset)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotCacheScope {
    pub mode: String,
    pub fallback_mode: Option<String>,
    pub viewport_biased: bool,
    pub returned_node_count: usize,
    pub unavailable_frame_count: usize,
    pub global_interactive_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotCacheEntry {
    pub document: DocumentMetadata,
    pub snapshot: Arc<str>,
    pub nodes: Arc<[SnapshotNode]>,
    pub scope: SnapshotCacheScope,
}

#[derive(Debug, Clone)]
pub struct ScreenshotArtifact {
    pub id: String,
    pub uri: String,
    pub path: PathBuf,
    pub format: ScreenshotFormat,
    pub mime_type: &'static str,
    pub byte_count: usize,
    pub width: u32,
    pub height: u32,
    pub mode: ScreenshotMode,
    pub tab_id: String,
    pub clip: Option<ScreenshotClip>,
}

impl ScreenshotArtifact {
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> Arc<[u8]> {
        Arc::<[u8]>::from(
            std::fs::read(&self.path).expect("test screenshot artifact bytes should be readable"),
        )
    }
}

fn screenshot_artifact_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let ordinal = SCREENSHOT_ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{millis}-{ordinal}")
}

fn screenshot_artifact_root() -> PathBuf {
    std::env::temp_dir()
        .join("chromewright")
        .join("screenshots")
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn screenshot_artifact_filename(id: &str, format: ScreenshotFormat) -> String {
    format!("chromewright-shot-{id}.{}", format.extension())
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

    pub(crate) fn snapshot_cache_entry(
        &self,
        document: &DocumentMetadata,
    ) -> Result<Option<Arc<SnapshotCacheEntry>>> {
        let mut guard =
            self.snapshot_cache
                .lock()
                .map_err(|e| BrowserError::ToolExecutionFailed {
                    tool: "snapshot".to_string(),
                    reason: format!("Failed to read snapshot cache: {}", e),
                })?;

        let Some(entry) = guard.as_ref() else {
            return Ok(None);
        };

        if entry.document.document_id == document.document_id {
            return Ok(Some(Arc::clone(entry)));
        }

        *guard = None;
        Ok(None)
    }

    pub(crate) fn store_snapshot_cache(&self, entry: Arc<SnapshotCacheEntry>) -> Result<()> {
        *self
            .snapshot_cache
            .lock()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "snapshot".to_string(),
                reason: format!("Failed to write snapshot cache: {}", e),
            })? = Some(entry);
        Ok(())
    }

    pub(crate) fn invalidate_snapshot_cache(&self) -> Result<()> {
        *self
            .snapshot_cache
            .lock()
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "snapshot".to_string(),
                reason: format!("Failed to invalidate snapshot cache: {}", e),
            })? = None;
        Ok(())
    }

    pub(crate) fn store_screenshot_artifact(
        &self,
        capture: ScreenshotCapture,
    ) -> Result<Arc<ScreenshotArtifact>> {
        let root = screenshot_artifact_root();
        std::fs::create_dir_all(&root).map_err(|e| {
            BrowserError::ScreenshotFailed(format!(
                "Failed to prepare screenshot artifact directory: {}",
                e
            ))
        })?;

        let artifact_id = screenshot_artifact_id();
        let path = root.join(screenshot_artifact_filename(&artifact_id, capture.format));
        std::fs::write(&path, &capture.bytes).map_err(|e| {
            BrowserError::ScreenshotFailed(format!("Failed to store screenshot artifact: {}", e))
        })?;
        let path = path.canonicalize().unwrap_or(path);

        let artifact = Arc::new(ScreenshotArtifact {
            id: artifact_id,
            uri: file_uri(&path),
            path,
            format: capture.format,
            mime_type: capture.mime_type,
            byte_count: capture.byte_count,
            width: capture.width,
            height: capture.height,
            mode: capture.mode,
            tab_id: capture.tab.id,
            clip: capture.clip,
        });

        let mut evicted = VecDeque::new();
        {
            let mut guard = self.screenshot_artifacts.lock().map_err(|e| {
                BrowserError::ScreenshotFailed(format!(
                    "Failed to write screenshot artifact state: {}",
                    e
                ))
            })?;

            guard.push_back(Arc::clone(&artifact));
            while guard.len() > SCREENSHOT_ARTIFACT_LIMIT {
                if let Some(stale) = guard.pop_front() {
                    evicted.push_back(stale);
                }
            }
        }

        for stale in evicted {
            remove_screenshot_file(&stale.path)?;
        }

        Ok(artifact)
    }

    pub(crate) fn clear_screenshot_artifacts(&self) -> Result<()> {
        let drained = {
            let mut guard = self.screenshot_artifacts.lock().map_err(|e| {
                BrowserError::ScreenshotFailed(format!(
                    "Failed to clear screenshot artifact state: {}",
                    e
                ))
            })?;
            guard.drain(..).collect::<Vec<_>>()
        };

        let mut failures = Vec::new();
        for artifact in drained {
            if let Err(err) = remove_screenshot_file(&artifact.path) {
                failures.push(err.to_string());
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(BrowserError::ScreenshotFailed(format!(
                "Failed to clear screenshot artifacts: {}",
                failures.join("; ")
            )))
        }
    }

    #[cfg(test)]
    pub(crate) fn snapshot_cache_for_test(&self) -> Result<Option<Arc<SnapshotCacheEntry>>> {
        self.snapshot_cache
            .lock()
            .map(|guard| guard.clone())
            .map_err(|e| BrowserError::ToolExecutionFailed {
                tool: "snapshot".to_string(),
                reason: format!("Failed to inspect snapshot cache: {}", e),
            })
    }
}

fn remove_screenshot_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(BrowserError::ScreenshotFailed(format!(
            "Failed to remove screenshot artifact {}: {}",
            path.display(),
            err
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{FakeSessionBackend, ScreenshotRequest};
    use crate::dom::{Cursor, NodeRef};

    fn sample_document(document_id: &str, revision: &str) -> DocumentMetadata {
        DocumentMetadata {
            document_id: document_id.to_string(),
            revision: revision.to_string(),
            url: format!("https://{}.example", document_id),
            title: format!("Document {}", document_id),
            ready_state: "complete".to_string(),
            frames: Vec::new(),
        }
    }

    fn sample_snapshot_entry(document: DocumentMetadata) -> Arc<SnapshotCacheEntry> {
        Arc::new(SnapshotCacheEntry {
            document: document.clone(),
            snapshot: Arc::<str>::from("button \"Save\""),
            nodes: Arc::<[SnapshotNode]>::from(vec![SnapshotNode {
                cursor: Cursor {
                    node_ref: NodeRef {
                        document_id: document.document_id.clone(),
                        revision: document.revision.clone(),
                        index: 0,
                    },
                    selector: "#save".to_string(),
                    index: 0,
                    role: "button".to_string(),
                    name: "Save".to_string(),
                },
                node_ref: NodeRef {
                    document_id: document.document_id.clone(),
                    revision: document.revision.clone(),
                    index: 0,
                },
                index: 0,
                role: "button".to_string(),
                name: "Save".to_string(),
            }]),
            scope: SnapshotCacheScope {
                mode: "viewport".to_string(),
                fallback_mode: None,
                viewport_biased: true,
                returned_node_count: 1,
                unavailable_frame_count: 0,
                global_interactive_count: Some(1),
            },
        })
    }

    #[test]
    fn snapshot_cache_round_trips_for_matching_document_revision() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let document = sample_document("doc-1", "rev-1");
        let entry = sample_snapshot_entry(document.clone());

        session
            .store_snapshot_cache(Arc::clone(&entry))
            .expect("snapshot cache should store");

        let cached = session
            .snapshot_cache_entry(&document)
            .expect("snapshot cache should read")
            .expect("matching cache entry should exist");

        assert_eq!(cached.as_ref(), entry.as_ref());
    }

    #[test]
    fn snapshot_cache_reuses_prior_revision_for_matching_document_identity() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let stored_document = sample_document("doc-1", "rev-1");
        let current_document = sample_document("doc-1", "rev-2");

        session
            .store_snapshot_cache(sample_snapshot_entry(stored_document))
            .expect("snapshot cache should store");

        let cached = session
            .snapshot_cache_entry(&current_document)
            .expect("snapshot cache lookup should succeed")
            .expect("matching document identity should keep prior revision base");

        assert_eq!(cached.document.document_id, "doc-1");
        assert_eq!(cached.document.revision, "rev-1");
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("test helper should read cache")
                .is_some()
        );
    }

    #[test]
    fn snapshot_cache_evicts_mismatched_document_identity_on_read() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let stored_document = sample_document("doc-1", "rev-1");
        let current_document = sample_document("doc-2", "rev-9");

        session
            .store_snapshot_cache(sample_snapshot_entry(stored_document))
            .expect("snapshot cache should store");

        let cached = session
            .snapshot_cache_entry(&current_document)
            .expect("snapshot cache lookup should succeed");

        assert!(cached.is_none());
        assert!(
            session
                .snapshot_cache_for_test()
                .expect("test helper should read cache")
                .is_none()
        );
    }

    #[test]
    fn screenshot_artifact_retention_prunes_old_entries() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let mut first_path = None;

        for _ in 0..=SCREENSHOT_ARTIFACT_LIMIT {
            let artifact = session
                .capture_screenshot_artifact(ScreenshotRequest::default())
                .expect("screenshot artifact should store");
            if first_path.is_none() {
                first_path = Some(artifact.path.clone());
            }
        }

        let artifacts = session.screenshot_artifacts_for_test();
        assert_eq!(artifacts.len(), SCREENSHOT_ARTIFACT_LIMIT);
        assert!(
            !first_path.expect("first artifact should exist").exists(),
            "oldest artifact should be pruned from disk"
        );

        session
            .close()
            .expect("session close should clean artifacts");
    }

    #[test]
    fn screenshot_artifact_tracks_png_metadata() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let artifact = session
            .capture_screenshot_artifact(ScreenshotRequest::default())
            .expect("default screenshot artifact should store");

        assert_eq!(artifact.format, ScreenshotFormat::Png);
        assert_eq!(artifact.mime_type, "image/png");
        assert_eq!((artifact.width, artifact.height), (1600, 1200));
        assert_eq!(artifact.byte_count, artifact.bytes().len());
        assert!(artifact.uri.starts_with("file://"));
        assert!(artifact.path.exists());

        session
            .close()
            .expect("session close should clean artifacts");
    }
}
