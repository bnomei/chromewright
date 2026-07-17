//! Capture a semantic document from the shared browser session.

use crate::browser::BrowserSession;
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
}
