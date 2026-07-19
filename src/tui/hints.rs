//! Deterministic two-key hints (Vimari-style Hint mode).
//!
//! Assigns unique labels to viewport-visible links **and** form controls, each
//! bound to an exact `semantic_ref`. Multi-line/wrapped targets still get one
//! label (painted only on the block-start row). Matching fails closed on
//! ambiguity; partial prefixes keep collecting keys until Exact or None.

use crate::semantic::{SemanticComponent, SemanticKind, SemanticRef};
use crate::tui::content::ContentLine;
use std::collections::HashMap;

/// Hint alphabet for two-key labels (home-row friendly, deterministic).
const HINT_CHARS: &[u8] = b"asdfgqwertzxcvb";

/// One assigned two-key hint over a viewport-visible target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkHint {
    /// Deterministic two-character label (e.g. `aa`, `as`).
    pub label: String,
    /// Exact `semantic_ref` of the link or form control.
    pub semantic_ref: SemanticRef,
}

/// Whether a component can receive a hint label (`f` / `F`).
pub fn is_hintable_component(component: &SemanticComponent) -> bool {
    if component.attrs.disabled.unwrap_or(false) {
        return false;
    }
    match component.kind {
        SemanticKind::Link | SemanticKind::Textarea | SemanticKind::Select | SemanticKind::Button => {
            true
        }
        SemanticKind::Input => {
            let t = component
                .attrs
                .input_type
                .as_deref()
                .unwrap_or("text")
                .to_ascii_lowercase();
            // Skip non-interactive / non-visible control types.
            !matches!(t.as_str(), "hidden" | "file")
        }
        _ => false,
    }
}

/// Assign two-key labels to hintable targets currently visible in the viewport.
///
/// Only the first visible row of each target participates (wrapped multi-line
/// links/controls still get a single label). Labels are deterministic for a
/// given ordered set of targets. Maximum `HINT_CHARS.len()^2` simultaneous hints.
pub fn assign_hints(
    lines: &[ContentLine],
    scroll_y: usize,
    viewport_height: usize,
    targets: &[SemanticComponent],
) -> Vec<LinkHint> {
    let visible_end = scroll_y.saturating_add(viewport_height.max(1));
    // First encounter of each ref in the viewport = block-start / first visible
    // row. Continuation wrap rows share the same ref but must not add duplicates
    // (and render only paints on block_start).
    let mut visible_refs = Vec::new();
    for line in lines
        .iter()
        .skip(scroll_y)
        .take(visible_end.saturating_sub(scroll_y))
    {
        if let Some(r) = &line.semantic_ref
            && !visible_refs.contains(r)
        {
            visible_refs.push(r.clone());
        }
    }

    let mut target_by_ref: HashMap<&SemanticRef, &SemanticComponent> = HashMap::new();
    for target in targets {
        if is_hintable_component(target) {
            target_by_ref.insert(&target.semantic_ref, target);
        }
    }

    let mut ordered: Vec<SemanticRef> = visible_refs
        .into_iter()
        .filter(|r| target_by_ref.contains_key(r))
        .collect();

    // Preserve document order of targets, not raw line encounter order.
    ordered.sort_by_key(|r| {
        targets
            .iter()
            .position(|t| &t.semantic_ref == r)
            .unwrap_or(usize::MAX)
    });

    let alphabet: Vec<char> = HINT_CHARS.iter().map(|&b| b as char).collect();
    let max = alphabet.len() * alphabet.len();
    ordered
        .into_iter()
        .take(max)
        .enumerate()
        .map(|(i, semantic_ref)| {
            let a = alphabet[i / alphabet.len()];
            let b = alphabet[i % alphabet.len()];
            LinkHint {
                label: format!("{a}{b}"),
                semantic_ref,
            }
        })
        .collect()
}

/// Resolve a typed hint buffer against assigned hints (fail closed on ambiguity).
pub fn match_hint<'a>(hints: &'a [LinkHint], buffer: &str) -> HintMatch<'a> {
    if buffer.is_empty() {
        return HintMatch::Partial;
    }
    let exact: Vec<_> = hints.iter().filter(|h| h.label == buffer).collect();
    if exact.len() == 1 {
        return HintMatch::Exact(&exact[0].semantic_ref);
    }
    if exact.len() > 1 {
        // Should not happen with unique labels; fail closed.
        return HintMatch::None;
    }
    if hints.iter().any(|h| h.label.starts_with(buffer)) {
        return HintMatch::Partial;
    }
    HintMatch::None
}

/// Outcome of matching the in-progress hint buffer to assigned link labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HintMatch<'a> {
    /// Exactly one label matches; carry its `semantic_ref` for follow/open.
    Exact(&'a SemanticRef),
    /// Buffer is a proper prefix of at least one label (keep collecting keys).
    Partial,
    /// No label matches or is a prefix of the buffer (reject).
    None,
}

/// Generate the full deterministic two-key sequence table (for tests).
#[allow(dead_code)]
pub fn label_for_index(index: usize) -> Option<String> {
    let n = HINT_CHARS.len();
    if index >= n * n {
        return None;
    }
    let a = HINT_CHARS[index / n] as char;
    let b = HINT_CHARS[index % n] as char;
    Some(format!("{a}{b}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DocumentMetadata;
    use crate::semantic::normalize::{RawSemanticNode, normalize_fixture};
    use crate::tui::content::build_content_lines;
    use std::collections::HashSet;

    fn link_node(id: &str, href: &str, text: &str) -> RawSemanticNode {
        RawSemanticNode {
            kind: "link".into(),
            tag: Some("a".into()),
            id: Some(id.into()),
            unique_id: true,
            selector: None,
            text: Some(text.into()),
            href: Some(href.into()),
            landmark: None,
            heading_level: None,
            ordered: None,
            label: None,
            src: None,
            alt: None,
            name: None,
            value: None,
            input_type: None,
            placeholder: None,
            checked: None,
            disabled: None,
            required: None,
            readonly: None,
            multiple: None,
            button_type: None,
            options: vec![],
            children: vec![],
        }
    }

    #[test]
    fn two_key_labels_are_deterministic() {
        assert_eq!(label_for_index(0).as_deref(), Some("aa"));
        assert_eq!(label_for_index(1).as_deref(), Some("as"));
        assert_eq!(
            label_for_index(0),
            label_for_index(0),
            "stable across calls"
        );
    }

    fn input_node(id: &str, name: &str) -> RawSemanticNode {
        RawSemanticNode {
            kind: "input".into(),
            tag: Some("input".into()),
            id: Some(id.into()),
            unique_id: true,
            selector: None,
            text: None,
            href: None,
            landmark: None,
            heading_level: None,
            ordered: None,
            label: None,
            src: None,
            alt: None,
            name: Some(name.into()),
            value: None,
            input_type: Some("text".into()),
            placeholder: None,
            checked: None,
            disabled: None,
            required: None,
            readonly: None,
            multiple: None,
            button_type: None,
            options: vec![],
            children: vec![],
        }
    }

    #[test]
    fn assign_and_match_exact_ref() {
        let doc = normalize_fixture(
            DocumentMetadata {
                document_id: "d".into(),
                revision: "1".into(),
                url: "https://example.com/".into(),
                title: "T".into(),
                ready_state: "complete".into(),
                frames: vec![],
            },
            vec![link_node("a", "/a", "A"), link_node("b", "/b", "B")],
        )
        .expect("doc");
        let lines = build_content_lines(&doc, &HashSet::new());
        let targets: Vec<_> = doc
            .components()
            .filter(|c| is_hintable_component(c))
            .cloned()
            .collect();
        let hints = assign_hints(&lines, 0, 50, &targets);
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].label, "aa");
        assert_eq!(hints[1].label, "as");
        match match_hint(&hints, "aa") {
            HintMatch::Exact(r) => {
                assert_eq!(r, &hints[0].semantic_ref);
                assert!(doc.resolve(r).is_ok());
            }
            other => panic!("expected exact, got {other:?}"),
        }
        assert_eq!(match_hint(&hints, "a"), HintMatch::Partial);
        assert_eq!(match_hint(&hints, "zz"), HintMatch::None);
    }

    #[test]
    fn assign_includes_form_controls_once_per_target() {
        let doc = normalize_fixture(
            DocumentMetadata {
                document_id: "d".into(),
                revision: "1".into(),
                url: "https://example.com/".into(),
                title: "T".into(),
                ready_state: "complete".into(),
                frames: vec![],
            },
            vec![
                link_node("a", "/a", "A very long link label that will wrap"),
                input_node("email", "email"),
            ],
        )
        .expect("doc");
        let lines = build_content_lines(&doc, &HashSet::new());
        let wrapped = crate::tui::content::wrap_content_lines(&lines, 12);
        let link_ref = doc
            .components()
            .find(|c| c.attrs.element_id.as_deref() == Some("a"))
            .unwrap()
            .semantic_ref
            .clone();
        let link_rows = wrapped
            .iter()
            .filter(|l| l.semantic_ref.as_ref() == Some(&link_ref))
            .count();
        assert!(
            link_rows >= 2,
            "expected wrapped multi-line link, got {link_rows} rows"
        );
        let targets: Vec<_> = doc
            .components()
            .filter(|c| is_hintable_component(c))
            .cloned()
            .collect();
        let hints = assign_hints(&wrapped, 0, 50, &targets);
        assert_eq!(hints.len(), 2, "link + input, not one hint per wrap row");
        assert_eq!(
            hints
                .iter()
                .filter(|h| h.semantic_ref == link_ref)
                .count(),
            1,
            "multi-line link must get a single hint"
        );
    }
}
