//! OSC 52 clipboard copy with a visible non-destructive fallback.

use base64::Engine;
use std::io::{self, Write};

/// Result of attempting to copy text to the terminal clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardResult {
    /// OSC 52 sequence was written successfully.
    Copied,
    /// Terminal refused or is unavailable; payload retained for status display.
    Fallback { text: String },
}

/// Encode text as an OSC 52 clipboard sequence (clipboard `c`).
pub fn osc52_sequence(text: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    format!("\x1b]52;c;{encoded}\x07")
}

/// Attempt OSC 52 copy to the provided writer.
///
/// On write failure, returns [`ClipboardResult::Fallback`] without panicking
/// and without mutating browser/page state.
pub fn copy_text_to_writer<W: Write>(writer: &mut W, text: &str) -> ClipboardResult {
    let seq = osc52_sequence(text);
    match writer
        .write_all(seq.as_bytes())
        .and_then(|_| writer.flush())
    {
        Ok(()) => ClipboardResult::Copied,
        Err(_) => ClipboardResult::Fallback {
            text: text.to_string(),
        },
    }
}

/// Copy using stdout when a real terminal is present; otherwise fallback.
pub fn copy_text(text: &str) -> ClipboardResult {
    if !std::io::IsTerminal::is_terminal(&io::stdout()) {
        return ClipboardResult::Fallback {
            text: text.to_string(),
        };
    }
    let mut out = io::stdout().lock();
    copy_text_to_writer(&mut out, text)
}

/// Status line for a copy attempt (never includes key legends).
pub fn copy_status(result: &ClipboardResult, kind: &str) -> String {
    match result {
        ClipboardResult::Copied => format!("copied {kind}"),
        ClipboardResult::Fallback { text } => {
            let preview: String = text.chars().take(48).collect();
            let ellipsis = if text.chars().count() > 48 { "…" } else { "" };
            format!("clipboard unavailable; {kind}: {preview}{ellipsis}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_is_well_formed() {
        let seq = osc52_sequence("hello");
        assert!(seq.starts_with("\x1b]52;c;"));
        assert!(seq.ends_with('\x07'));
        assert!(seq.contains(&base64::engine::general_purpose::STANDARD.encode("hello")));
    }

    #[test]
    fn write_failure_is_non_destructive_fallback() {
        struct FailWriter;
        impl Write for FailWriter {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("nope"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let result = copy_text_to_writer(&mut FailWriter, "secret-ref");
        assert_eq!(
            result,
            ClipboardResult::Fallback {
                text: "secret-ref".into()
            }
        );
        let status = copy_status(&result, "semantic_ref");
        assert!(status.contains("clipboard unavailable"));
        assert!(status.contains("secret-ref"));
        assert!(!status.to_ascii_lowercase().contains("press "));
    }

    #[test]
    fn successful_write_reports_copied() {
        let mut buf = Vec::new();
        let result = copy_text_to_writer(&mut buf, "block");
        assert_eq!(result, ClipboardResult::Copied);
        assert!(!buf.is_empty());
    }
}
