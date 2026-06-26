DEVANA-FINDING: v1
DEVANA-STATE: fixed | P2 | medium | security=no
DEVANA-KEY: src/tools/evaluate.rs:57 | evaluate-null-reported-success

# evaluate reports success when CDP returns no value

## Finding

After `confirm_unsafe` passes, `EvaluateTool` maps a missing CDP evaluation payload to `serde_json::Value::Null` and returns `ToolResult::success_with`. Callers cannot distinguish "expression evaluated to null/undefined" from "evaluation produced no value" (detached frame, swallowed exception path, or protocol gap).

## Violated Invariant Or Contract

Operator tools that execute arbitrary JavaScript should fail closed when the browser returns no value unless the contract explicitly documents null as a valid successful result for all no-payload cases.

## Oracle

`get_markdown` extraction treats missing values as `ToolExecutionFailed` with an explicit reason (`services/markdown.rs`). `screenshot` reveal/decode paths error on missing payloads. `evaluate` alone uses `unwrap_or(Value::Null)`.

## Counterexample

CDP `Runtime.evaluate` returns `Ok(ScriptEvaluation { value: None, ... })` because the target context was destroyed mid-flight. MCP receives `{ "success": true, "result": null }` (via structured content). An agent's retry logic keyed on success proceeds as if the script ran and returned JavaScript `null`.

## Why It Might Matter

Automation loops may skip recovery (snapshot, tab_list) after silent no-op evaluations, leaving agents operating on stale page state while believing the script executed.

## Proof

Caller/callee mismatch: `BrowserSession::evaluate` can yield `ScriptEvaluation` without `value`; callee `execute_typed` always wraps success; caller MCP layer sees tool success.

## Counterevidence Checked

Syntax errors and thrown page errors likely still surface as `EvaluationFailed` Err paths. When the script genuinely returns JS `null`, the same payload shape appears—so fixing this requires preserving a distinguishable error channel for missing vs null JSON. `confirm_unsafe` gate still blocks unconfirmed calls.

## Suggested Next Step

Return structured failure when `result.value` is `None`, or include an explicit `value_present: false` field in `EvaluateOutput` for missing payloads.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. `EvaluateOutput` now exposes `value_present: bool` plus the
  CDP `type_name`/`description` so callers can distinguish "script returned JS
  `null`" from "no value produced" (destroyed context, `undefined`, by-reference
  object). Did not fail-closed on `None` because legitimate evaluations
  (`undefined`, void expressions) commonly produce no value; a hard error there
  would break valid operator scripts. Added fake-backend `__devana_no_value__`
  fixture and two tests (`test_evaluate_tool_flags_missing_value`,
  `test_evaluate_tool_marks_value_present`). All 4 evaluate tests pass.

DEVANA-KEY: src/tools/evaluate.rs:57 | evaluate-null-reported-success
DEVANA-SUMMARY: fixed | P2 | medium | evaluate now reports value_present plus type_name/description so a missing CDP value is distinguishable from a real null result.