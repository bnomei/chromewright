//! Capture a semantic document from the shared browser session.

use crate::browser::BrowserSession;
use crate::dom::DocumentMetadata;
use crate::error::{BrowserError, Result};
use crate::semantic::document::SemanticDocument;
use crate::semantic::normalize::{SemanticCaptureResponse, normalize_capture};

/// Capture the active tab's hydrated DOM as a bounded `SemanticDocument`.
///
/// This path is independent of `BrowserSession::extract_dom` / `DomTree`.
pub fn extract_semantic_document(session: &BrowserSession) -> Result<SemanticDocument> {
    let evaluation = session.evaluate(include_str!("extract_semantic_dom.js"), false)?;
    let value = evaluation.value.ok_or_else(|| {
        BrowserError::DomParseFailed("No value returned from semantic capture".to_string())
    })?;

    let response = decode_capture_value(value)?;
    normalize_capture(response)
}

/// Whether a main-frame semantic capture is still safe to publish against a
/// subsequent browser metadata probe.
///
/// The semantic extractor intentionally does not walk iframe documents, so its
/// revision is the metadata probe's `main:<revision>` segment rather than the
/// full token with iframe suffixes. A mutation to the represented top-level DOM
/// changes that segment and fails closed. Frame-only changes do not invalidate
/// a capture that contains no frame content.
pub(crate) fn capture_matches_document_metadata(
    captured: &DocumentMetadata,
    metadata: &DocumentMetadata,
) -> bool {
    if captured.document_id != metadata.document_id
        || captured.url != metadata.url
        || captured.title != metadata.title
        || captured.ready_state != metadata.ready_state
    {
        return false;
    }

    if captured.revision == metadata.revision {
        return true;
    }

    metadata
        .revision
        .split('|')
        .next()
        .is_some_and(|main_revision| main_revision == captured.revision)
}

fn decode_capture_value(value: serde_json::Value) -> Result<SemanticCaptureResponse> {
    let normalized = match value {
        serde_json::Value::String(json_str) => serde_json::from_str(&json_str).map_err(|e| {
            BrowserError::DomParseFailed(format!("Failed to parse semantic capture JSON: {e}"))
        })?,
        structured => structured,
    };

    serde_json::from_value(normalized).map_err(|e| {
        BrowserError::DomParseFailed(format!("Failed to decode semantic capture payload: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_accepts_stringified_and_structured_payloads() {
        let structured = json!({
            "document": {
                "document_id": "doc-1",
                "revision": "1",
                "url": "https://example.com/",
                "title": "T",
                "ready_state": "complete",
                "frames": []
            },
            "nodes": [],
            "truncated": false,
            "error": null
        });

        let as_string = serde_json::Value::String(structured.to_string());
        let decoded = decode_capture_value(as_string).expect("string payload");
        assert_eq!(decoded.document.document_id, "doc-1");

        let decoded = decode_capture_value(structured).expect("object payload");
        assert_eq!(decoded.document.revision, "1");
    }

    fn metadata(revision: &str) -> DocumentMetadata {
        DocumentMetadata {
            document_id: "doc-1".into(),
            revision: revision.into(),
            url: "https://example.com/".into(),
            title: "T".into(),
            ready_state: "complete".into(),
            frames: Vec::new(),
        }
    }

    #[test]
    fn capture_matches_metadata_with_ignored_frame_suffixes() {
        assert!(capture_matches_document_metadata(
            &metadata("main:7"),
            &metadata("main:7|frame0:frame-doc:4|frame1:cross_origin"),
        ));
    }

    #[test]
    fn capture_rejects_main_frame_mutation_even_when_frame_suffixes_match() {
        assert!(!capture_matches_document_metadata(
            &metadata("main:7"),
            &metadata("main:8|frame0:frame-doc:4"),
        ));
    }

    #[test]
    fn capture_rejects_non_revision_metadata_changes() {
        let captured = metadata("main:7");
        let mut changed = metadata("main:7|frame0:frame-doc:4");
        changed.title = "changed".into();
        assert!(!capture_matches_document_metadata(&captured, &changed));
    }
}
