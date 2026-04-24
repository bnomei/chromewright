use crate::browser::BrowserSession;
pub(crate) use crate::browser::commands::{
    ActionabilityDiagnostics, ActionabilityPredicate, ActionabilityProbeRequest,
    ActionabilityProbeResult,
};
use crate::browser::commands::{BrowserCommand, BrowserCommandResult};
use crate::error::{BrowserError, Result};

#[derive(Debug, Clone)]
pub(crate) struct ActionabilityRequest<'a> {
    pub selector: &'a str,
    pub target_index: Option<usize>,
    pub predicates: &'a [ActionabilityPredicate],
    pub expected_text: Option<&'a str>,
    pub expected_value: Option<&'a str>,
}

#[cfg(test)]
pub(crate) fn build_actionability_probe_js(request: &ActionabilityRequest<'_>) -> String {
    BrowserCommand::ActionabilityProbe(command_request(request)).render_script()
}

pub(crate) fn probe_actionability(
    session: &BrowserSession,
    request: &ActionabilityRequest<'_>,
) -> Result<ActionabilityProbeResult> {
    let result = session
        .execute_command(BrowserCommand::ActionabilityProbe(command_request(request)))
        .map_err(|e| match e {
            BrowserError::EvaluationFailed(reason) => BrowserError::ToolExecutionFailed {
                tool: "actionability".to_string(),
                reason,
            },
            other => other,
        })?;

    let BrowserCommandResult::ActionabilityProbe(probe) = result else {
        return Err(BrowserError::ToolExecutionFailed {
            tool: "actionability".to_string(),
            reason: "Browser command returned an unexpected result for actionability".to_string(),
        });
    };

    validate_probe_payload(request, &probe)?;

    Ok(probe)
}

fn command_request(request: &ActionabilityRequest<'_>) -> ActionabilityProbeRequest {
    ActionabilityProbeRequest {
        selector: request.selector.to_string(),
        target_index: request.target_index,
        predicates: request.predicates.to_vec(),
        expected_text: request.expected_text.map(str::to_string),
        expected_value: request.expected_value.map(str::to_string),
    }
}

fn validate_probe_payload(
    request: &ActionabilityRequest<'_>,
    probe: &ActionabilityProbeResult,
) -> Result<()> {
    if !probe.present {
        return Ok(());
    }

    let missing = request
        .predicates
        .iter()
        .filter_map(|predicate| {
            (probe.predicate(*predicate).is_none()).then_some(predicate.key().to_string())
        })
        .collect::<Vec<_>>();

    if missing.is_empty() {
        return Ok(());
    }

    Err(BrowserError::ToolExecutionFailed {
        tool: "actionability".to_string(),
        reason: format!(
            "Actionability probe returned an incomplete payload for a present target: missing {}",
            missing.join(", ")
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserSession;
    use crate::browser::backend::{ScriptEvaluation, SessionBackend, TabDescriptor};
    use crate::dom::{DocumentMetadata, DomTree};
    use serde_json::Value;
    use std::time::Duration;

    #[test]
    fn test_build_actionability_probe_js_keeps_wait_predicates_narrow() {
        let present_js = build_actionability_probe_js(&ActionabilityRequest {
            selector: "#node",
            target_index: None,
            predicates: &[ActionabilityPredicate::Present],
            expected_text: None,
            expected_value: None,
        });
        let present_config = extract_embedded_config(&present_js);
        assert_eq!(present_config["predicates"], serde_json::json!(["present"]));
        assert_eq!(present_config["text"], Value::Null);
        assert_eq!(present_config["value"], Value::Null);

        let visible_js = build_actionability_probe_js(&ActionabilityRequest {
            selector: "#node",
            target_index: None,
            predicates: &[ActionabilityPredicate::Visible],
            expected_text: None,
            expected_value: None,
        });
        let visible_config = extract_embedded_config(&visible_js);
        assert_eq!(visible_config["predicates"], serde_json::json!(["visible"]));
        assert_eq!(visible_config["text"], Value::Null);
        assert_eq!(visible_config["value"], Value::Null);

        let text_js = build_actionability_probe_js(&ActionabilityRequest {
            selector: "#node",
            target_index: None,
            predicates: &[ActionabilityPredicate::TextContains],
            expected_text: Some("hello"),
            expected_value: None,
        });
        let text_config = extract_embedded_config(&text_js);
        assert_eq!(
            text_config["predicates"],
            serde_json::json!(["text_contains"])
        );
        assert_eq!(text_config["text"].as_str(), Some("hello"));
        assert_eq!(text_config["value"], Value::Null);

        let value_js = build_actionability_probe_js(&ActionabilityRequest {
            selector: "#node",
            target_index: None,
            predicates: &[ActionabilityPredicate::ValueEquals],
            expected_text: None,
            expected_value: Some("expected"),
        });
        let value_config = extract_embedded_config(&value_js);
        assert_eq!(
            value_config["predicates"],
            serde_json::json!(["value_equals"])
        );
        assert_eq!(value_config["text"], Value::Null);
        assert_eq!(value_config["value"].as_str(), Some("expected"));
    }

    #[test]
    fn test_build_actionability_probe_js_supports_interaction_predicates_and_target_index() {
        let js = build_actionability_probe_js(&ActionabilityRequest {
            selector: "#inside",
            target_index: Some(4),
            predicates: &[
                ActionabilityPredicate::Visible,
                ActionabilityPredicate::Enabled,
                ActionabilityPredicate::Stable,
                ActionabilityPredicate::ReceivesEvents,
                ActionabilityPredicate::InViewport,
                ActionabilityPredicate::UnobscuredCenter,
            ],
            expected_text: None,
            expected_value: None,
        });

        let config = extract_embedded_config(&js);
        assert_eq!(config["selector"].as_str(), Some("#inside"));
        assert_eq!(config["target_index"].as_u64(), Some(4));
        assert!(js.contains("searchActionableIndex"));
        assert!(js.contains("querySelectorAcrossScopes"));
        assert!(js.contains("const match = resolveTargetMatch(config).match;"));
        assert!(js.contains("element.ownerDocument.elementFromPoint"));
        assert!(js.contains("hit_target"));
    }

    #[test]
    fn test_actionability_probe_result_reports_requested_predicates() {
        let result = ActionabilityProbeResult {
            present: true,
            visible: Some(true),
            enabled: Some(false),
            editable: Some(false),
            stable: Some(true),
            receives_events: Some(false),
            in_viewport: Some(true),
            unobscured_center: Some(false),
            text_contains: Some(true),
            value_equals: Some(false),
            frame_depth: Some(1),
            diagnostics: None,
        };

        assert_eq!(
            result.predicate(ActionabilityPredicate::Present),
            Some(true)
        );
        assert_eq!(
            result.predicate(ActionabilityPredicate::Visible),
            Some(true)
        );
        assert_eq!(
            result.predicate(ActionabilityPredicate::Enabled),
            Some(false)
        );
        assert_eq!(result.predicate(ActionabilityPredicate::Stable), Some(true));
        assert_eq!(
            result.predicate(ActionabilityPredicate::ReceivesEvents),
            Some(false)
        );
        assert_eq!(
            result.predicate(ActionabilityPredicate::TextContains),
            Some(true)
        );
    }

    struct StaticActionabilityBackend {
        value: serde_json::Value,
    }

    impl SessionBackend for StaticActionabilityBackend {
        fn navigate(&self, _url: &str) -> Result<()> {
            unreachable!("navigate is not used in this test")
        }

        fn wait_for_navigation(&self) -> Result<()> {
            unreachable!("wait_for_navigation is not used in this test")
        }

        fn wait_for_document_ready_with_timeout(&self, _timeout: Duration) -> Result<()> {
            unreachable!("wait_for_document_ready_with_timeout is not used in this test")
        }

        fn document_metadata(&self) -> Result<DocumentMetadata> {
            unreachable!("document_metadata is not used in this test")
        }

        fn extract_dom(&self) -> Result<DomTree> {
            unreachable!("extract_dom is not used in this test")
        }

        fn extract_dom_with_prefix(&self, _prefix: &str) -> Result<DomTree> {
            unreachable!("extract_dom_with_prefix is not used in this test")
        }

        fn evaluate(&self, _script: &str, _await_promise: bool) -> Result<ScriptEvaluation> {
            unreachable!("actionability tests use browser commands, not raw evaluate")
        }

        fn execute_command(&self, command: BrowserCommand) -> Result<BrowserCommandResult> {
            match command {
                BrowserCommand::ActionabilityProbe(_) => {
                    serde_json::from_value::<ActionabilityProbeResult>(self.value.clone())
                        .map(BrowserCommandResult::ActionabilityProbe)
                        .map_err(BrowserError::from)
                }
                _ => unreachable!("only actionability commands are used in this test"),
            }
        }

        fn capture_screenshot(&self, _full_page: bool) -> Result<Vec<u8>> {
            unreachable!("capture_screenshot is not used in this test")
        }

        fn press_key(&self, _key: &str) -> Result<()> {
            unreachable!("press_key is not used in this test")
        }

        fn list_tabs(&self) -> Result<Vec<TabDescriptor>> {
            Ok(vec![TabDescriptor {
                id: "tab-1".to_string(),
                title: "Test Tab".to_string(),
                url: "about:blank".to_string(),
            }])
        }

        fn active_tab(&self) -> Result<TabDescriptor> {
            unreachable!("active_tab is not used in this test")
        }

        fn open_tab(&self, _url: &str) -> Result<TabDescriptor> {
            unreachable!("open_tab is not used in this test")
        }

        fn activate_tab(&self, _tab_id: &str) -> Result<()> {
            unreachable!("activate_tab is not used in this test")
        }

        fn close_tab(&self, _tab_id: &str, _with_unload: bool) -> Result<()> {
            unreachable!("close_tab is not used in this test")
        }

        fn close(&self) -> Result<()> {
            unreachable!("close is not used in this test")
        }
    }

    #[test]
    fn test_probe_actionability_rejects_incomplete_present_target_payloads() {
        let session = BrowserSession::with_test_backend(StaticActionabilityBackend {
            value: serde_json::json!({
                "present": true,
                "frame_depth": 0,
            }),
        });

        let err = probe_actionability(
            &session,
            &ActionabilityRequest {
                selector: "#save",
                target_index: None,
                predicates: &[ActionabilityPredicate::Visible],
                expected_text: None,
                expected_value: None,
            },
        )
        .expect_err("missing requested predicate should fail");

        match err {
            BrowserError::ToolExecutionFailed { tool, reason } => {
                assert_eq!(tool, "actionability");
                assert!(reason.contains("incomplete payload"));
                assert!(reason.contains("visible"));
            }
            other => panic!("unexpected actionability error: {other:?}"),
        }
    }

    #[test]
    fn test_probe_actionability_allows_missing_requested_predicates_for_detached_targets() {
        let session = BrowserSession::with_test_backend(StaticActionabilityBackend {
            value: serde_json::json!({
                "present": false,
                "frame_depth": 0,
            }),
        });

        let probe = probe_actionability(
            &session,
            &ActionabilityRequest {
                selector: "#save",
                target_index: None,
                predicates: &[ActionabilityPredicate::Visible],
                expected_text: None,
                expected_value: None,
            },
        )
        .expect("detached targets may omit requested predicate fields");

        assert!(!probe.present);
        assert_eq!(probe.visible, None);
    }

    fn extract_embedded_config(js: &str) -> Value {
        let marker = "const config = ";
        let start = js
            .find(marker)
            .map(|index| index + marker.len())
            .expect("config marker should exist");
        let end = js[start..]
            .find(';')
            .map(|offset| start + offset)
            .expect("config assignment should end with a semicolon");
        serde_json::from_str(&js[start..end]).expect("embedded config should be valid JSON")
    }
}
