use crate::contract::{TargetEnvelope, TargetStatus, ViewportMetrics};
use crate::dom::{DocumentMetadata, SnapshotNode};

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
    #[default]
    Viewport,
    Delta,
    Full,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DocumentEnvelope {
    pub document: DocumentMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<SnapshotNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<SnapshotScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_interactive_count: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DocumentResult {
    pub document: DocumentMetadata,
}

impl DocumentResult {
    pub fn new(document: DocumentMetadata) -> Self {
        Self { document }
    }
}

impl From<DocumentEnvelope> for DocumentResult {
    fn from(envelope: DocumentEnvelope) -> Self {
        Self::new(envelope.document)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DocumentActionResult {
    #[serde(flatten)]
    pub document_result: DocumentResult,
    pub action: String,
}

impl DocumentActionResult {
    pub fn new(action: impl Into<String>, document: DocumentMetadata) -> Self {
        Self {
            document_result: DocumentResult::new(document),
            action: action.into(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetedActionResult {
    #[serde(flatten)]
    pub document_action_result: DocumentActionResult,
    pub target_before: TargetEnvelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_after: Option<TargetEnvelope>,
    pub target_status: TargetStatus,
}

impl TargetedActionResult {
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq)]
pub struct SnapshotScope {
    pub mode: SnapshotMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_mode: Option<SnapshotMode>,
    pub viewport_biased: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locality_fallback_reason: Option<String>,
    pub unavailable_frame_count: usize,
    pub returned_node_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_interactive_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ViewportMetrics>,
}
