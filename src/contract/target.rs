//! Element targeting input and resolution metadata for MCP tools.
//!
//! [`PublicTarget`] is the exclusive MCP input (selector or revision-scoped
//! cursor). [`TargetEnvelope`] reports how resolution ran, including stale-cursor
//! recovery. [`TargetStatus`] classifies post-mutation continuity for agents.

use crate::dom::{Cursor, NodeRef};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Deserializer;
use std::borrow::Cow;

/// Resolved target handed between tools, including stale-cursor recovery status.
///
/// Carries the resolution method (`selector` / `cursor`), whether recovery rewrote
/// a stale cursor, and the revision-scoped handles agents reuse for follow-up tools.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetEnvelope {
    /// How the target was resolved (for example `selector` or `cursor`).
    pub method: String,
    /// Resolution outcome: typically `exact`, or a recovery status after rebinding.
    #[serde(default = "default_target_resolution_status")]
    pub resolution_status: String,
    /// Prior handle when stale-cursor recovery rebound to a new node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovered_from: Option<String>,
    /// Revision-scoped cursor for follow-up interaction tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Revision-scoped node reference from snapshot indexing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,
    /// CSS selector when that was the resolution method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Interactive index within the snapshot when resolved by index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

fn default_target_resolution_status() -> String {
    "exact".to_string()
}

/// MCP tool input for targeting an element by selector or revision-scoped cursor.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PublicTarget {
    /// Resolve the target from a selector in the current document.
    Selector { selector: String },
    /// Reuse a revision-scoped cursor from `snapshot` or `inspect_node`.
    Cursor { cursor: Cursor },
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TaggedPublicTarget {
    Selector { selector: String },
    Cursor { cursor: Cursor },
}

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(untagged)]
enum PublicTargetCompat {
    SelectorString(String),
    Tagged(TaggedPublicTarget),
}

impl<'de> serde::Deserialize<'de> for PublicTarget {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match PublicTargetCompat::deserialize(deserializer)? {
            PublicTargetCompat::SelectorString(selector) => Ok(Self::Selector { selector }),
            PublicTargetCompat::Tagged(TaggedPublicTarget::Selector { selector }) => {
                Ok(Self::Selector { selector })
            }
            PublicTargetCompat::Tagged(TaggedPublicTarget::Cursor { cursor }) => {
                Ok(Self::Cursor { cursor })
            }
        }
    }
}

impl JsonSchema for PublicTarget {
    fn schema_name() -> Cow<'static, str> {
        "PublicTarget".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        PublicTargetCompat::json_schema(generator)
    }
}

impl PublicTarget {
    /// Split into exclusive selector or cursor handles for shared target resolution.
    pub(crate) fn into_selector_or_cursor(self) -> (Option<String>, Option<Cursor>) {
        match self {
            Self::Selector { selector } => (Some(selector), None),
            Self::Cursor { cursor } => (None, Some(cursor)),
        }
    }
}

/// Whether an interaction left the resolved target unchanged, rebound, or detached.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    /// Post-action probe still resolves the same node as `target_before`.
    Same,
    /// Target was rebound (for example via stale-cursor recovery) to a different node.
    Rebound,
    /// Target no longer resolves after the mutation.
    Detached,
    /// Post-action target status could not be determined.
    Unknown,
}
