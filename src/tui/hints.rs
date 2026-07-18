//! Deterministic two-key link hints (Vimari-style).

use crate::semantic::{SemanticComponent, SemanticRef};
use crate::tui::content::ContentLine;
use std::collections::HashMap;

/// Hint alphabet for two-key labels (home-row friendly, deterministic).
const HINT_CHARS: &[u8] = b"asdfgqwertzxcvb";

/// One assigned two-key hint over a viewport-visible link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkHint {
    /// Deterministic two-character label (e.g. `aa`, `as`).
    pub label: String,
    /// Exact `semantic_ref` of the link to follow or open in a new tab.
    pub semantic_ref: SemanticRef,
}

/// Assign two-key labels to links currently visible in the viewport.
///
/// Labels are deterministic for a given ordered set of links (document order
/// among visible lines). Maximum `HINT_CHARS.len()^2` simultaneous hints.
pub fn assign_hints(
    lines: &[ContentLine],
    scroll_y: usize,
    viewport_height: usize,
    links: &[SemanticComponent],
) -> Vec<LinkHint> {
    let visible_end = scroll_y.saturating_add(viewport_height.max(1));
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

    let mut link_by_ref: HashMap<&SemanticRef, &SemanticComponent> = HashMap::new();
    for link in links {
        link_by_ref.insert(&link.semantic_ref, link);
    }

    let mut ordered: Vec<SemanticRef> = visible_refs
        .into_iter()
        .filter(|r| link_by_ref.contains_key(r))
        .collect();

    // Preserve document order of links, not raw line encounter order of other nodes.
    ordered.sort_by_key(|r| {
        links
            .iter()
            .position(|l| &l.semantic_ref == r)
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
        let links: Vec<_> = doc
            .components()
            .filter(|c| c.kind == crate::semantic::SemanticKind::Link)
            .cloned()
            .collect();
        let hints = assign_hints(&lines, 0, 50, &links);
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
}
