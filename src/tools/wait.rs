use crate::dom::{Cursor, NodeRef};
use crate::error::Result;
use crate::tools::{
    TargetEnvelope, Tool, ToolContext, ToolResult, services::interaction::TargetStatus,
    services::wait::execute_wait,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(test)]
pub(crate) use crate::tools::services::wait::{
    condition_name, validate_wait_condition, wait_condition_matches, wait_condition_predicates,
    wait_condition_uses_interaction_scroll,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaitCondition {
    NavigationSettled,
    Present,
    Visible,
    Enabled,
    Editable,
    Actionable,
    Stable,
    ReceivesEvents,
    TextContains,
    ValueEquals,
    RevisionChanged,
}

fn default_condition() -> WaitCondition {
    WaitCondition::Present
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WaitParams {
    /// CSS selector to wait for
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Element index from the current DOM tree
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Revision-scoped node reference from the snapshot tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    /// Cursor from the snapshot or inspect_node tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,

    /// Wait predicate to apply
    #[serde(default = "default_condition")]
    pub condition: WaitCondition,

    /// Expected text fragment for `text_contains`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Expected value for `value_equals`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    /// Baseline revision token for `revision_changed`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_revision: Option<String>,

    /// Timeout in milliseconds (default: 30000)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    30000
}

#[derive(Default)]
pub struct WaitTool;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WaitOutput {
    #[serde(flatten)]
    pub envelope: crate::tools::DocumentEnvelope,
    pub action: String,
    pub condition: String,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_before: Option<TargetEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_after: Option<TargetEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_status: Option<TargetStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_revision: Option<String>,
}

impl Tool for WaitTool {
    type Params = WaitParams;
    type Output = WaitOutput;

    fn name(&self) -> &str {
        "wait"
    }

    fn execute_typed(&self, params: WaitParams, context: &mut ToolContext) -> Result<ToolResult> {
        execute_wait(params, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use crate::tools::actionability::ActionabilityPredicate;
    use serde_json::json;

    #[test]
    fn test_wait_params_defaults() {
        let params: WaitParams =
            serde_json::from_value(json!({})).expect("params should deserialize");

        assert_eq!(params.condition, WaitCondition::Present);
        assert_eq!(params.timeout_ms, 30_000);
        assert!(params.selector.is_none());
        assert!(params.cursor.is_none());
        assert!(params.text.is_none());
        assert!(params.value.is_none());
    }

    #[test]
    fn test_validate_wait_condition_requires_text_and_value() {
        let text_error = validate_wait_condition(&WaitCondition::TextContains, None, None)
            .expect_err("text_contains without text should fail");
        assert!(matches!(
            text_error,
            crate::error::BrowserError::InvalidArgument(_)
        ));
        assert!(text_error.to_string().contains("wait.text"));

        let value_error = validate_wait_condition(&WaitCondition::ValueEquals, None, None)
            .expect_err("value_equals without value should fail");
        assert!(matches!(
            value_error,
            crate::error::BrowserError::InvalidArgument(_)
        ));
        assert!(value_error.to_string().contains("wait.value"));

        validate_wait_condition(&WaitCondition::Present, None, None)
            .expect("present should not require extra arguments");
        validate_wait_condition(&WaitCondition::TextContains, Some("hello"), None)
            .expect("text_contains should accept text");
        validate_wait_condition(&WaitCondition::ValueEquals, None, Some("abc"))
            .expect("value_equals should accept value");
    }

    #[test]
    fn test_condition_name_covers_all_wait_conditions() {
        let cases = [
            (WaitCondition::NavigationSettled, "navigation_settled"),
            (WaitCondition::Present, "present"),
            (WaitCondition::Visible, "visible"),
            (WaitCondition::Enabled, "enabled"),
            (WaitCondition::Editable, "editable"),
            (WaitCondition::Actionable, "actionable"),
            (WaitCondition::Stable, "stable"),
            (WaitCondition::ReceivesEvents, "receives_events"),
            (WaitCondition::TextContains, "text_contains"),
            (WaitCondition::ValueEquals, "value_equals"),
            (WaitCondition::RevisionChanged, "revision_changed"),
        ];

        for (condition, expected) in cases {
            assert_eq!(condition_name(&condition), expected);
        }
    }

    #[test]
    fn test_wait_condition_predicates_reuse_shared_actionability_model() {
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Present),
            [ActionabilityPredicate::Present]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Visible),
            [ActionabilityPredicate::Visible]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Enabled),
            [ActionabilityPredicate::Enabled]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Editable),
            [ActionabilityPredicate::Editable]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Actionable),
            [
                ActionabilityPredicate::Present,
                ActionabilityPredicate::Visible,
                ActionabilityPredicate::Enabled,
                ActionabilityPredicate::Stable,
                ActionabilityPredicate::ReceivesEvents,
                ActionabilityPredicate::UnobscuredCenter,
            ]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Stable),
            [ActionabilityPredicate::Stable]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::ReceivesEvents),
            [ActionabilityPredicate::ReceivesEvents]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::TextContains),
            [ActionabilityPredicate::TextContains]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::ValueEquals),
            [ActionabilityPredicate::ValueEquals]
        );
        assert!(wait_condition_predicates(&WaitCondition::RevisionChanged).is_empty());
        assert!(wait_condition_predicates(&WaitCondition::NavigationSettled).is_empty());
    }

    #[test]
    fn test_wait_condition_predicates_cover_every_targeted_wait_condition() {
        let targeted_conditions = [
            WaitCondition::Present,
            WaitCondition::Visible,
            WaitCondition::Enabled,
            WaitCondition::Editable,
            WaitCondition::Actionable,
            WaitCondition::Stable,
            WaitCondition::ReceivesEvents,
            WaitCondition::TextContains,
            WaitCondition::ValueEquals,
        ];

        for condition in targeted_conditions {
            assert!(
                !wait_condition_predicates(&condition).is_empty(),
                "expected shared predicates for '{}'",
                condition_name(&condition),
            );
        }

        assert_eq!(
            wait_condition_predicates(&WaitCondition::Actionable)
                .iter()
                .map(|predicate| predicate.key())
                .collect::<Vec<_>>(),
            vec![
                "present",
                "visible",
                "enabled",
                "stable",
                "receives_events",
                "unobscured_center",
            ]
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::Stable)[0].key(),
            "stable"
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::ReceivesEvents)[0].key(),
            "receives_events"
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::TextContains)[0].key(),
            "text_contains"
        );
        assert_eq!(
            wait_condition_predicates(&WaitCondition::ValueEquals)[0].key(),
            "value_equals"
        );
    }

    #[test]
    fn test_wait_condition_matches_requires_all_actionable_predicates() {
        let probe = crate::tools::actionability::ActionabilityProbeResult {
            present: true,
            visible: Some(true),
            enabled: Some(true),
            editable: None,
            stable: Some(true),
            receives_events: Some(true),
            in_viewport: None,
            unobscured_center: Some(true),
            text_contains: None,
            value_equals: None,
            frame_depth: Some(0),
            diagnostics: None,
        };
        assert!(wait_condition_matches(
            &WaitCondition::Actionable,
            wait_condition_predicates(&WaitCondition::Actionable),
            &probe
        ));

        let obscured = crate::tools::actionability::ActionabilityProbeResult {
            unobscured_center: Some(false),
            ..probe
        };
        assert!(!wait_condition_matches(
            &WaitCondition::Actionable,
            wait_condition_predicates(&WaitCondition::Actionable),
            &obscured
        ));
    }

    #[test]
    fn test_wait_condition_uses_interaction_scroll_for_event_delivery_checks() {
        assert!(wait_condition_uses_interaction_scroll(
            &WaitCondition::Actionable
        ));
        assert!(wait_condition_uses_interaction_scroll(
            &WaitCondition::ReceivesEvents
        ));
        assert!(!wait_condition_uses_interaction_scroll(
            &WaitCondition::Visible
        ));
        assert!(!wait_condition_uses_interaction_scroll(
            &WaitCondition::Stable
        ));
    }

    #[test]
    fn test_wait_tool_navigation_settled_executes_against_fake_backend() {
        let session = BrowserSession::with_test_backend(FakeSessionBackend::new());
        let tool = WaitTool::default();
        let mut context = ToolContext::new(&session);

        let result = tool
            .execute_typed(
                WaitParams {
                    selector: None,
                    index: None,
                    node_ref: None,
                    cursor: None,
                    condition: WaitCondition::NavigationSettled,
                    text: None,
                    value: None,
                    since_revision: None,
                    timeout_ms: 100,
                },
                &mut context,
            )
            .expect("navigation_settled should succeed");

        assert!(result.success);
        let data = result.data.expect("wait should include data");
        assert_eq!(data["condition"].as_str(), Some("navigation_settled"));
        assert_eq!(data["document"]["ready_state"].as_str(), Some("complete"));
    }
}
