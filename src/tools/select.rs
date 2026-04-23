use crate::dom::{Cursor, NodeRef};
use crate::error::{BrowserError, Result};
use crate::tools::{
    TargetResolution, Tool, ToolContext, ToolResult,
    actionability::ActionabilityPredicate,
    browser_kernel::render_browser_kernel_script,
    core::PublicTarget,
    core::TargetedActionResult,
    services::interaction::{
        ActionabilityWaitState, DEFAULT_ACTIONABILITY_TIMEOUT_MS, build_actionability_failure,
        build_interaction_failure, build_interaction_handoff, decode_action_result,
        resolve_interaction_target, wait_for_actionability,
    },
};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Parameters for the select tool
#[derive(Debug, Clone, Serialize)]
pub struct SelectParams {
    /// CSS selector (use either this or index, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Element index from DOM tree (use either this or selector, not both)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Revision-scoped node reference from the snapshot tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_ref: Option<NodeRef>,

    /// Cursor from the snapshot or inspect_node tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,

    /// Value to select in the dropdown
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StrictSelectParams {
    /// Target dropdown to operate on.
    pub target: PublicTarget,
    /// Value to select in the dropdown.
    pub value: String,
}

impl From<StrictSelectParams> for SelectParams {
    fn from(params: StrictSelectParams) -> Self {
        let (selector, cursor) = params.target.into_selector_or_cursor();
        Self {
            selector,
            index: None,
            node_ref: None,
            cursor,
            value: params.value,
        }
    }
}

impl<'de> Deserialize<'de> for SelectParams {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictSelectParams::deserialize(deserializer).map(Into::into)
    }
}

impl JsonSchema for SelectParams {
    fn schema_name() -> Cow<'static, str> {
        "SelectParams".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        StrictSelectParams::json_schema(generator)
    }
}

/// Tool for selecting dropdown options
#[derive(Default)]
pub struct SelectTool;

const SELECT_JS: &str = include_str!("select.js");

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SelectOutput {
    #[serde(flatten)]
    pub result: TargetedActionResult,
    pub value: String,
    pub selected_text: Option<String>,
}

impl Tool for SelectTool {
    type Params = SelectParams;
    type Output = SelectOutput;

    fn name(&self) -> &str {
        "select"
    }

    fn description(&self) -> &str {
        "Choose a dropdown value. Usually after snapshot; next wait or snapshot."
    }

    fn execute_typed(&self, params: SelectParams, context: &mut ToolContext) -> Result<ToolResult> {
        let SelectParams {
            selector,
            index,
            node_ref,
            cursor,
            value,
        } = params;
        let target =
            match resolve_interaction_target("select", selector, index, node_ref, cursor, context)?
            {
                TargetResolution::Resolved(target) => target,
                TargetResolution::Failure(failure) => return Ok(context.finish(failure)),
            };

        let predicates = select_actionability_predicates();
        match wait_for_actionability(
            context,
            &target,
            predicates,
            DEFAULT_ACTIONABILITY_TIMEOUT_MS,
        )? {
            ActionabilityWaitState::Ready => {}
            ActionabilityWaitState::TimedOut(probe) => {
                return build_actionability_failure(
                    "select",
                    context.session,
                    &target,
                    &probe,
                    predicates,
                    None,
                )
                .map(|result| context.finish(result));
            }
        }

        let select_config = serde_json::json!({
            "selector": target.selector,
            "target_index": target.cursor.as_ref().map(|cursor| cursor.index).or(target.index),
            "value": value,
        });
        let select_js = build_select_js(&select_config);

        context.record_browser_evaluation();
        let result = context
            .session
            .evaluate(&select_js, false)
            .map_err(|e| match e {
                BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
                    tool: "select".to_string(),
                    reason,
                },
                other => other,
            })?;

        match parse_select_result(result.value)? {
            SelectParseResult::Success(selected_text) => {
                let handoff = build_interaction_handoff(context, &target)?;
                Ok(context.finish(ToolResult::success_with(SelectOutput {
                    result: TargetedActionResult::new(
                        "select",
                        handoff.document,
                        handoff.target_before,
                        handoff.target_after,
                        handoff.target_status,
                    ),
                    value,
                    selected_text,
                })))
            }
            SelectParseResult::Failure { code, error } => build_interaction_failure(
                "select",
                context.session,
                &target,
                code,
                error,
                Vec::new(),
                None,
            )
            .map(|result| context.finish(result)),
        }
    }
}

fn build_select_js(config: &serde_json::Value) -> String {
    render_browser_kernel_script(SELECT_JS, "__SELECT_CONFIG__", config)
}

enum SelectParseResult {
    Success(Option<String>),
    Failure { code: String, error: String },
}

#[derive(Debug, Deserialize)]
struct RawSelectResult {
    success: bool,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    selected_text: Option<String>,
}

fn parse_select_result(value: Option<serde_json::Value>) -> Result<SelectParseResult> {
    let mut result_json = decode_action_result(
        value,
        serde_json::json!({
            "success": false,
            "code": "target_detached",
            "error": "Element is no longer present"
        }),
    )?;
    promote_legacy_select_fields(&mut result_json);
    let result: RawSelectResult = serde_json::from_value(result_json)?;

    if result.success {
        Ok(SelectParseResult::Success(result.selected_text))
    } else {
        Ok(SelectParseResult::Failure {
            code: result.code.unwrap_or_else(|| "invalid_target".to_string()),
            error: result.error.unwrap_or_else(|| "Select failed".to_string()),
        })
    }
}

fn promote_legacy_select_fields(result_json: &mut serde_json::Value) {
    let Some(object) = result_json.as_object_mut() else {
        return;
    };

    if object.contains_key("selected_text") {
        return;
    }

    if let Some(selected_text) = object.remove("selectedText") {
        object.insert("selected_text".to_string(), selected_text);
    }
}

fn select_actionability_predicates() -> &'static [ActionabilityPredicate] {
    &[
        ActionabilityPredicate::Present,
        ActionabilityPredicate::Visible,
        ActionabilityPredicate::Enabled,
        ActionabilityPredicate::Stable,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::schema_for;
    use serde_json::json;

    #[test]
    fn test_select_params_deserializes_strict_target() {
        let json = serde_json::json!({
            "target": {
                "kind": "selector",
                "selector": "#country-select"
            },
            "value": "us"
        });

        let params: SelectParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.selector, Some("#country-select".to_string()));
        assert_eq!(params.index, None);
        assert_eq!(params.value, "us");
    }

    #[test]
    fn test_select_params_rejects_legacy_public_target_fields() {
        let error = serde_json::from_value::<SelectParams>(json!({
            "selector": "#country-select",
            "value": "us"
        }))
        .expect_err("legacy selector field should be rejected");
        assert!(error.to_string().contains("unknown field `selector`"));

        let schema = schema_for!(SelectParams);
        let schema_json = serde_json::to_value(&schema).expect("schema should serialize");
        let properties = schema_json
            .get("properties")
            .and_then(|value| value.as_object())
            .expect("select params schema should expose properties");
        assert!(properties.contains_key("target"));
        assert!(!properties.contains_key("selector"));
        assert!(!properties.contains_key("index"));
        assert!(!properties.contains_key("node_ref"));
        assert!(!properties.contains_key("cursor"));
    }

    #[test]
    fn test_parse_select_result_success() {
        let result = parse_select_result(Some(serde_json::Value::String(
            r#"{"success":true,"selected_text":"United Kingdom"}"#.to_string(),
        )))
        .expect("select result should parse");

        match result {
            SelectParseResult::Success(selected_text) => {
                assert_eq!(selected_text.as_deref(), Some("United Kingdom"));
            }
            SelectParseResult::Failure { error, .. } => panic!("unexpected failure: {error}"),
        }
    }

    #[test]
    fn test_parse_select_result_failure_uses_code_and_error() {
        let result = parse_select_result(Some(serde_json::json!({
            "success": false,
            "code": "invalid_target",
            "error": "Element is not a SELECT element"
        })))
        .expect("select result should parse");

        match result {
            SelectParseResult::Failure { code, error } => {
                assert_eq!(code, "invalid_target");
                assert_eq!(error, "Element is not a SELECT element");
            }
            SelectParseResult::Success(_) => panic!("expected failure"),
        }
    }

    #[test]
    fn test_decode_tool_result_json_rejects_invalid_json_string() {
        let error = decode_action_result(
            Some(serde_json::Value::String("not-json".to_string())),
            serde_json::json!({}),
        )
        .expect_err("invalid JSON should fail");

        assert!(matches!(error, BrowserError::JsonError(_)));
    }

    #[test]
    fn test_select_js_prefers_selector_before_target_index() {
        let select_js = build_select_js(&serde_json::json!({
            "selector": "#country-select",
            "target_index": 5,
            "value": "us",
        }));

        assert!(select_js.contains("function resolveTargetMatch(config, options)"));
        assert!(select_js.contains("const element = resolveTargetElement(config);"));
        assert!(select_js.contains("querySelectorAcrossScopes("));
        assert!(select_js.contains("searchActionableIndex(config.target_index)"));
    }
}
