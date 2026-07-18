//! Document and snapshot result envelopes shared by read and mutation tools.

use crate::contract::{TargetEnvelope, TargetStatus, ViewportMetrics};
use crate::dom::{DocumentMetadata, SnapshotNode};

/// Snapshot rendering mode: viewport-biased tree, delta against cache, or full document.
#[derive(
    Debug,
    Clone,
    Copy,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    PartialEq,
    Eq,
    Default,
)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    /// Prefer interactive nodes near the current viewport (default locality bias).
    #[default]
    Viewport,
    /// Diff against the session's revision-keyed snapshot cache base.
    Delta,
    /// Return the full accessibility tree without viewport locality filtering.
    Full,
}

/// Combined document metadata with optional snapshot YAML, node list, and scope summary.
///
/// Primary read envelope for snapshot tools: metadata always present; snapshot/nodes/scope
/// fill in when the caller requested a rendered tree rather than metadata-only.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DocumentEnvelope {
    /// Document identity (`document_id`, revision, URL, title, ready state, frames).
    pub document: DocumentMetadata,
    /// Optional focused target when the snapshot was locality-biased around a handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetEnvelope>,
    /// YAML accessibility tree when a snapshot was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    /// Interactive nodes with cursors corresponding to the snapshot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<SnapshotNode>,
    /// How the snapshot was scoped (mode, viewport bias, frame failures).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<SnapshotScope>,
    /// Total interactive count in the document when known (may exceed returned nodes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_interactive_count: Option<usize>,
}

/// Lightweight tool payload that returns only document identity after a read or navigation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DocumentResult {
    /// Document identity after the operation (`document_id`, revision, URL, title, …).
    pub document: DocumentMetadata,
}

impl DocumentResult {
    /// Wrap document metadata for tools that do not return snapshot or target envelopes.
    pub fn new(document: DocumentMetadata) -> Self {
        Self { document }
    }
}

impl From<DocumentEnvelope> for DocumentResult {
    fn from(envelope: DocumentEnvelope) -> Self {
        Self::new(envelope.document)
    }
}

/// Document metadata plus the action name that produced the post-mutation state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DocumentActionResult {
    /// Flattened document identity envelope.
    #[serde(flatten)]
    pub document_result: DocumentResult,
    /// Tool action label (for example `navigate` or `click`) for agent transcripts.
    pub action: String,
}

impl DocumentActionResult {
    /// Build a post-action document result with a stable action label.
    pub fn new(action: impl Into<String>, document: DocumentMetadata) -> Self {
        Self {
            document_result: DocumentResult::new(document),
            action: action.into(),
        }
    }
}

/// Mutation result that includes target envelopes before/after and resolution status.
///
/// Used by click/input/hover/select tools so agents can see whether the interactive handle
/// remained valid after the action and what the target looked like at each phase.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetedActionResult {
    /// Flattened document + action label after the mutation.
    #[serde(flatten)]
    pub document_action_result: DocumentActionResult,
    /// Target snapshot resolved before the mutation ran.
    pub target_before: TargetEnvelope,
    /// Target snapshot after mutation when the handle still resolves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_after: Option<TargetEnvelope>,
    /// Whether the target stayed valid, drifted, or was lost after the action.
    pub target_status: TargetStatus,
}

impl TargetedActionResult {
    /// Assemble a mutation result with before/after targets and post-action status.
    pub fn new(
        action: impl Into<String>,
        document: DocumentMetadata,
        target_before: TargetEnvelope,
        target_after: Option<TargetEnvelope>,
        target_status: TargetStatus,
    ) -> Self {
        Self {
            document_action_result: DocumentActionResult::new(action, document),
            target_before,
            target_after,
            target_status,
        }
    }
}

/// Describes how a snapshot was scoped, including viewport bias and locality fallbacks.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq)]
pub struct SnapshotScope {
    /// Requested [`SnapshotMode`] (`viewport`, `delta`, or `full`).
    pub mode: SnapshotMode,
    /// Mode actually used when locality fallback downgraded the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_mode: Option<SnapshotMode>,
    /// Whether returned nodes were biased toward the current viewport.
    pub viewport_biased: bool,
    /// Why viewport locality could not be applied (when fallback_mode is set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locality_fallback_reason: Option<String>,
    /// Frames that could not contribute nodes (cross-origin or extraction failure).
    pub unavailable_frame_count: usize,
    /// Interactive nodes included in this response (after locality filtering).
    pub returned_node_count: usize,
    /// Total interactive nodes in the document when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_interactive_count: Option<usize>,
    /// Layout viewport metrics used for locality bias, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ViewportMetrics>,
}
