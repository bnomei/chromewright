DEVANA-FINDING: v1
DEVANA-STATE: fixed | P2 | medium | security=yes
DEVANA-KEY: src/browser/session/history.rs:50 | history-navigation-skips-url-gate

# go_back and go_forward bypass URL safety validation

## Finding

`navigate` and `new_tab` route URLs through `validate_navigation_url` and the `allow_unsafe` gate. `go_back` and `go_forward` invoke `window.history.back()` / `forward()` with no destination validation and no `allow_unsafe` parameter on the tool surface. The browser may land on `file:`, `data:`, or other entries already present in joint session history.

## Violated Invariant Or Contract

If `allow_unsafe=false` is the default safety posture for opening non-http(s) destinations, history navigation should not re-enter blocked schemes without the same per-request opt-in.

## Oracle

README ties `allow_unsafe` explicitly to `navigate` and `new_tab`. `GoBackTool` / `GoForwardTool` expose empty parameter structs with no safety fields. `history.rs` only invalidates snapshot cache after history moves.

## Counterexample

Attach-mode session on a profile that previously visited `file:///Users/agent/secrets.txt` (or agent used `navigate` with `allow_unsafe=true` earlier). Agent calls `go_back` with `{}`. History returns to the `file:` document. Reading tools (`get_markdown`, `extract`, `snapshot`) can exfiltrate local file content without any `allow_unsafe` on the history call.

## Why It Might Matter

Policies that block `file:`/`data:` only on forward navigation remain bypassable via history in long-lived browser profiles—especially attach mode with pre-existing user tabs.

## Proof

Cross-entry mismatch: `navigate` enforces `validate_navigation_url`; `go_back_with_metrics` path is `evaluate(history.back JS)` → `wait_for_history_settle` with no URL scheme check on the destination.

## Counterevidence Checked

History must already contain the unsafe entry; agents cannot `go_back` to a URL never visited. `allow_unsafe` on a prior `navigate` may have been intentional. Some sessions may have empty or single-entry history, in which case the path is inert.

## Suggested Next Step

After history settles, read `document_metadata().url` and reject or require `allow_unsafe` when the landed scheme is not http(s), mirroring `validate_navigation_url`.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. `go_back`/`go_forward` tools now expose `allow_unsafe`
  (default false). `BrowserSession::history_navigate` reads the landed URL after
  settle and, when `allow_unsafe=false` and the scheme is non-http(s), reverses
  the move (history.forward/back) to restore the prior entry and returns
  `InvalidArgument`, mirroring `validate_navigation_url`. `about:` is treated as
  safe (inert, common initial entry; no content to exfiltrate). The low-level
  `BrowserSession::go_back`/`go_forward` wrappers pass `allow_unsafe=true`,
  matching the existing pattern where `BrowserSession::navigate` is ungated and
  the gate lives at the tool surface. README updated. Added unit tests for
  block, opt-in, and scheme classification; existing integration tests pass
  `allow_unsafe=true` since they intentionally use `data:` pages.
  Residual: reading tools (`get_markdown`/`snapshot`/`extract`) still do not gate
  schemes on their own; a page already loaded via an earlier `allow_unsafe`
  navigate remains readable. That is out of scope for the history-gate finding.

DEVANA-KEY: src/browser/session/history.rs:50 | history-navigation-skips-url-gate
DEVANA-SUMMARY: fixed | P2 | medium | go_back/go_forward now apply the allow_unsafe scheme gate, reverting and rejecting history moves that land on non-http(s) destinations.