//! Opaque, fail-closed `semantic_ref` identities scoped to a document revision.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};

const REF_VERSION: &str = "sref1";

/// Opaque `semantic_ref` for one component within a captured document revision.
///
/// Consumers must treat the string form as opaque. Only the document's resolution
/// API may interpret it. Resolution is fail-closed and never retargets by text
/// similarity or structural guesswork.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticRef(String);

impl SemanticRef {
    /// Create a reference from a previously serialized opaque token.
    pub fn from_opaque(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the opaque token string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn encode(payload: &SemanticRefPayload) -> Self {
        Self(format!(
            "{REF_VERSION}.{}..{}..{}..{}",
            encode_field(&payload.document_id),
            encode_field(&payload.revision),
            encode_field(payload.identity.kind_token()),
            encode_field(&payload.identity.value),
        ))
    }

    pub(crate) fn decode(&self) -> Result<SemanticRefPayload, SemanticRefError> {
        let rest = self
            .0
            .strip_prefix(&format!("{REF_VERSION}."))
            .ok_or(SemanticRefError::Malformed)?;
        let parts: Vec<&str> = rest.split("..").collect();
        if parts.len() != 4 {
            return Err(SemanticRefError::Malformed);
        }

        let document_id = decode_field(parts[0]).ok_or(SemanticRefError::Malformed)?;
        let revision = decode_field(parts[1]).ok_or(SemanticRefError::Malformed)?;
        let kind = decode_field(parts[2]).ok_or(SemanticRefError::Malformed)?;
        let value = decode_field(parts[3]).ok_or(SemanticRefError::Malformed)?;
        let identity = SemanticIdentity::from_parts(&kind, value)?;

        Ok(SemanticRefPayload {
            document_id,
            revision,
            identity,
        })
    }

    /// Return the author-supplied DOM id encoded by this reference. Fingerprint
    /// identities intentionally have no browser selector: guessing a DOM node
    /// from a structural fingerprint would violate fail-closed interaction.
    pub(crate) fn author_id(&self) -> Result<Option<String>, SemanticRefError> {
        let payload = self.decode()?;
        Ok(match payload.identity.kind {
            SemanticIdentityKind::AuthorId => Some(payload.identity.value),
            SemanticIdentityKind::Fingerprint => None,
        })
    }
}

impl fmt::Display for SemanticRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SemanticRef {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Fail-closed outcomes when resolving a `semantic_ref` against a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticRefError {
    /// The reference string is not a well-formed semantic identity token.
    Malformed,
    /// The reference belongs to a different document id.
    WrongDocument { expected: String, actual: String },
    /// The reference targets a different document revision.
    Stale {
        expected_revision: String,
        actual_revision: String,
    },
    /// The document/revision match but no component owns this identity.
    Unknown,
    /// More than one component claims the same identity (should not occur after capture).
    Ambiguous,
}

impl fmt::Display for SemanticRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => write!(f, "semantic_ref is malformed"),
            Self::WrongDocument { expected, actual } => write!(
                f,
                "semantic_ref targets document {actual}, current document is {expected}"
            ),
            Self::Stale {
                expected_revision,
                actual_revision,
            } => write!(
                f,
                "semantic_ref targets revision {actual_revision}, current revision is {expected_revision}"
            ),
            Self::Unknown => write!(f, "semantic_ref is unknown in the current document"),
            Self::Ambiguous => write!(f, "semantic_ref is ambiguous in the current document"),
        }
    }
}

impl std::error::Error for SemanticRefError {}

/// Document-scoped identity key used to mint and resolve opaque `semantic_ref`s.
///
/// Prefer unique author DOM ids when available; otherwise use a structural
/// fingerprint. The key is never retargeted by display text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct SemanticIdentity {
    kind: SemanticIdentityKind,
    value: String,
}

/// How a component is identified within a document revision.
///
/// `AuthorId` is a unique author-supplied DOM id. `Fingerprint` is a durable
/// structural digest (scope + ancestry + signature) used when no unique id is
/// available. Interaction must not invent a selector from a fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum SemanticIdentityKind {
    AuthorId,
    Fingerprint,
}

impl SemanticIdentity {
    pub(crate) fn author_id(id: impl Into<String>) -> Self {
        Self {
            kind: SemanticIdentityKind::AuthorId,
            value: id.into(),
        }
    }

    pub(crate) fn fingerprint(value: impl Into<String>) -> Self {
        Self {
            kind: SemanticIdentityKind::Fingerprint,
            value: value.into(),
        }
    }

    pub(crate) fn from_parts(kind: &str, value: String) -> Result<Self, SemanticRefError> {
        let kind = match kind {
            "id" => SemanticIdentityKind::AuthorId,
            "fp" => SemanticIdentityKind::Fingerprint,
            _ => return Err(SemanticRefError::Malformed),
        };
        if value.is_empty() {
            return Err(SemanticRefError::Malformed);
        }
        Ok(Self { kind, value })
    }

    fn kind_token(&self) -> &'static str {
        match self.kind {
            SemanticIdentityKind::AuthorId => "id",
            SemanticIdentityKind::Fingerprint => "fp",
        }
    }
}

/// Decoded `semantic_ref` fields used only inside capture and resolution.
///
/// Encodes document id, revision, and identity kind/value into the opaque token.
/// Callers outside identity/resolution must treat the string form as opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticRefPayload {
    pub document_id: String,
    pub revision: String,
    pub identity: SemanticIdentity,
}

/// Build a durable fingerprint from document scope plus normalized semantic ancestry.
pub(crate) fn fingerprint_identity(
    document_scope: &str,
    ancestry: &[String],
    local_signature: &str,
) -> SemanticIdentity {
    let mut hasher = DefaultHasher::new();
    document_scope.hash(&mut hasher);
    for step in ancestry {
        step.hash(&mut hasher);
    }
    local_signature.hash(&mut hasher);
    let digest = hasher.finish();
    SemanticIdentity::fingerprint(format!("{digest:016x}"))
}

/// Origin + path scope used for fingerprint identity (excludes query/fragment volatility).
pub(crate) fn document_scope_from_url(url: &str) -> String {
    let without_fragment = url.split('#').next().unwrap_or(url);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    without_query.to_string()
}

fn encode_field(value: &str) -> String {
    // Percent-encode reserved separators so decode is unambiguous without extra crates.
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            other => {
                out.push('%');
                out.push(hex_digit(other >> 4));
                out.push(hex_digit(other & 0x0f));
            }
        }
    }
    out
}

fn decode_field(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hi = from_hex(bytes[i + 1])?;
                let lo = from_hex(bytes[i + 2])?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') => {
                out.push(b);
                i += 1;
            }
            _ => return None,
        }
    }
    String::from_utf8(out).ok()
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => '0',
    }
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_ref_round_trips_payload() {
        let payload = SemanticRefPayload {
            document_id: "doc/1".to_string(),
            revision: "main:3|frame0:expanded".to_string(),
            identity: SemanticIdentity::author_id("hero"),
        };
        let encoded = SemanticRef::encode(&payload);
        let decoded = encoded.decode().expect("decode");
        assert_eq!(decoded, payload);
        assert!(encoded.as_str().starts_with("sref1."));
    }

    #[test]
    fn malformed_refs_fail_closed() {
        assert_eq!(
            SemanticRef::from_opaque("not-a-ref").decode(),
            Err(SemanticRefError::Malformed)
        );
        assert_eq!(
            SemanticRef::from_opaque("sref1.only-one-part").decode(),
            Err(SemanticRefError::Malformed)
        );
    }

    #[test]
    fn fingerprint_is_deterministic_for_same_ancestry() {
        let a = fingerprint_identity(
            "https://example.com/page",
            &["nav".into(), "a[0]".into()],
            "a|/home",
        );
        let b = fingerprint_identity(
            "https://example.com/page",
            &["nav".into(), "a[0]".into()],
            "a|/home",
        );
        let c = fingerprint_identity(
            "https://example.com/other",
            &["nav".into(), "a[0]".into()],
            "a|/home",
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
