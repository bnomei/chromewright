DEVANA-FINDING: v1
DEVANA-STATE: fixed | P2 | high | security=no
DEVANA-KEY: src/tools/set_viewport.rs:149 | set-viewport-document-wrong-tab

# set_viewport with tab_id returns active-tab document metadata

## Finding

`set_viewport` can apply emulation to an inactive tab via `tab_id`, but the success payload's `result.document` envelope is built from `build_document_envelope`, which always reads DOM/metadata from the active tab (`ToolContext::get_dom` / `session.extract_dom()`). The response advertises `tab_id` for the emulated tab while embedding another tab's `document_id`, `revision`, and `url`.

## Violated Invariant Or Contract

When `tab_id` targets a specific tab, tool output document metadata should describe that tab's document, not whichever tab happens to be active.

## Oracle

`BrowserSession::apply_viewport_emulation` correctly routes CDP emulation by `tab_id` (test `test_apply_viewport_emulation_can_target_inactive_tab_without_activation`). `SetViewportOutput` includes both `tab_id` and flattened `DocumentActionResult` with `document`. README says successful calls return `viewport_metrics_after` for the affected tab and agents use `snapshot` for DOM state on the active tab.

## Counterexample

Session has tab A (active) on `https://a.example` and tab B on `https://b.example`. Call `set_viewport` with `tab_id: "<B>"`, `width: 375`, `height: 812`, `reset: false` without switching tabs. CDP applies metrics to tab B, but `result.document.url` and `document_id` come from tab A's `document_metadata()`. An agent trusting the envelope may chain DOM tools against the wrong document while believing viewport changed on B.

## Why It Might Matter

Multi-tab flows that resize a background tab before capture create inconsistent tool results: viewport metrics apply to B while document identity fields describe A, breaking revision-scoped cursor reuse and tab-scoped planning.

## Proof

Dataflow trace: MCP `tab_id` → `ViewportEmulationRequest.tab_id` → backend `with_specific_tab_operation` (correct tab) → `context.invalidate_dom()` → `build_document_envelope(..., minimal)` → `get_dom()` → `session.extract_dom()` (active tab only) → `DocumentActionResult.document` sink. `tab_id` in output is taken from `operation.tab_id`, not cross-checked with envelope source.

## Counterevidence Checked

When `tab_id` is omitted, active tab emulation and envelope source align. `switch_tab` invalidates `ToolContext.dom_tree` but does not fix this cross-tab mismatch because `set_viewport` never extracts DOM for the requested `tab_id`. Viewport metrics themselves are read from the correct tab via backend APIs.

## Suggested Next Step

Build the minimal document envelope from `document_metadata` / `extract_dom_for_tab` for `operation.tab_id`, or omit `result.document` when `tab_id` differs from the active tab and document that in the tool contract.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. `set_viewport` now sources the response document from
  `operation.tab_id` (the actually-emulated tab) instead of the active-tab
  `build_document_envelope(.., minimal)`. Added `document_metadata_for_tab` to
  the `SessionBackend` trait (default falls back to active-tab metadata or
  `BackendUnsupported`), with a real-backend override via
  `with_specific_tab_operation` — symmetric to the existing
  `extract_dom_for_tab` — and a fake-backend override via `document_for_tab`,
  plus a `BrowserSession::document_metadata_for_tab` wrapper. Chose option A
  (correct per-tab metadata, cheaper than a full `extract_dom_for_tab`) over
  option B (omit `result.document`) so multi-tab flows keep a coherent
  document envelope. Added a test asserting an inactive-tab target returns that
  tab's document_id/url, not the active tab's.

DEVANA-KEY: src/tools/set_viewport.rs:149 | set-viewport-document-wrong-tab
DEVANA-SUMMARY: fixed | P2 | high | set_viewport now returns the emulated tab's document metadata via document_metadata_for_tab instead of the active tab's.