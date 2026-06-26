DEVANA-FINDING: v1
DEVANA-STATE: fixed | P2 | medium | security=no
DEVANA-KEY: src/browser/backend.rs:940 | attach-recovery-replays-mutation

# Attach-session recovery replays non-idempotent mutating commands (double click/input/select/navigate)

## Finding

`with_active_tab_operation` (`src/browser/backend.rs:921-981`) runs a closure against
the active tab and, in attach mode, retries it once if the first attempt fails with a
recoverable page-target-loss error:

```rust
let result = match operation(&tab) {            // first run — side effect may commit here
    Ok(value) => Ok(value),
    Err(error) => {
        if !self.attach_mode || recoverable_page_target_loss_details(...).is_none() || ... {
            Err(error)
        } else {
            match self.recover_active_tab_handle() {
                Ok(recovered_tab) => match operation(&recovered_tab) {  // <-- SAME closure re-run
                    ...
```

The same closure is replayed verbatim. For read operations (`document_metadata`,
`extract_dom`, read-only `evaluate`) this is safe. But the identical retry also wraps
**non-idempotent mutating operations**:

- `execute_command` for `InteractionCommand::Click/Input/Hover/Select`
  (`backend.rs:1218-1270`) — the closure renders the interaction script and runs
  `tab.evaluate(&script, false)`; `click.js:23` performs `element.click()` before
  returning.
- `navigate` (`backend.rs:1144-1152`) — runs `tab.navigate_to(url)`.
- `press_key` (elsewhere via `with_active_tab_operation`).

`recoverable_page_target_loss_details` (`backend.rs:1116-1137`) classifies any error
whose message contains `"target closed"`, `"connection closed"`,
`"session closed. most likely the page has been closed"`, etc. as recoverable — exactly
the errors produced when a click/submit tears down or navigates the target *after* the
side effect has already been dispatched page-side.

## Violated Invariant Or Contract

A single tool call to a non-idempotent mutating operation must commit its browser-side
effect at most once. The tool registry advertises `idempotent_hint: false` for these
tools (`src/tools/core/mod.rs`), i.e. callers are told the operation is unsafe to
auto-retry — yet the backend auto-retries it.

## Oracle

The retry-once behavior is intentional and unit-tested (`backend.rs` tests assert the
operation runs twice on recovery). Those tests validate *read* recovery; nothing
distinguishes idempotent reads from side-effecting mutations, so the same replay is
applied to operations the public contract marks non-idempotent.

## Counterexample

Attach mode, agent issues `click` on a control whose handler commits a side effect and
then navigates/tears down the page (e.g. "Place order", a form submit):

1. `execute_command(Click)` → closure runs `tab.evaluate(click_js)`; `element.click()`
   fires and the server-side effect commits.
2. The navigation/teardown races the evaluate response read, which fails with
   `"target closed"` / `"connection closed"`.
3. `recoverable_page_target_loss_details` returns `Some`, inventory is available, so
   `recover_active_tab_handle()` reacquires the tab and `operation(&recovered_tab)`
   re-runs the **same** click script.
4. If the reacquired page still matches the target, `element.click()` fires a second
   time → duplicate submit/activation. `navigate` retried the same way re-issues the
   navigation.

## Why It Might Matter

Duplicate side effects on a transient transport drop: double form submission, double
"buy"/"confirm", repeated state mutation — classic non-idempotent-retry hazard. Impact
is bounded (attach mode only; only the recoverable-error substring set; only the narrow
window where the first effect commits but the response is lost), hence medium
confidence.

## Proof

- Control-flow trace: `click.rs:143` → `execute_command(Click)` →
  `with_active_tab_operation` (`backend.rs:920`) first run commits the click → recoverable
  error → `backend.rs:940` replays the same closure → second `element.click()`.
- Contract mismatch: tools declare `idempotent_hint: false`; the backend retries them as
  if idempotent.

## Counterevidence Checked

- Launch mode never retries: `!self.attach_mode` short-circuits (`backend.rs:933`), so
  only attach sessions are affected.
- Only the recoverable substring set triggers retry; unrelated failures propagate.
- For reads the replay is correct and desirable — the bug is specifically the uniform
  application to mutating commands.
- Strongest reason this might be false: a `"connection closed"` after a click often
  means the page (and thus the in-flight effect) was lost with the target, so a replay
  may be the intended best-effort and frequently lands on a detached target returning
  `success:false` harmlessly. But the window where the first effect commits server-side
  and the second replay also lands is real and undetected, and the operations are
  explicitly contract-marked non-idempotent.

## Suggested Next Step

Gate the retry on idempotency: thread the operation's `idempotent_hint` (or a per-call
flag) into `with_active_tab_operation` so non-idempotent mutations surface the
page-target-loss error to the caller instead of being silently replayed; reserve
replay for read/query operations.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` prefix. Keep `DEVANA-KEY:` stable
unless the same finding moved. Add dated notes below.

## Status Notes

- 2026-06-25: open by Devana. Static inspection; replay loop and mutating callers
  (Click/Input/Select/Hover, navigate, press_key) confirmed in code.
- 2026-06-26: fixed. Introduced a `TabOpRetry` policy (`ReplaySafe` vs
  `Mutation`). `with_active_tab_operation_retry` now, for `Mutation`, surfaces a
  degraded `attach_session_page_target_loss` error WITHOUT re-running the
  closure on a recoverable loss — the first attempt may have already committed.
  Reads keep the replay-once behavior via `with_active_tab_operation`; mutations
  use the new `with_active_tab_mutation`. Wired non-idempotent callers: `navigate`,
  `press_key`, and `execute_command` (the latter dispatches per
  `BrowserCommand::is_idempotent()` — probes replay, Click/Input/Hover/Select do
  not). Added `is_idempotent()` classification + tests, a mutation-policy
  simulation test asserting the operation runs exactly once and returns a
  degraded/non-recoverable error, and kept the existing read-replay tests green.
  Scope note: `evaluate`/`evaluate_on_tab` remain `ReplaySafe`, matching the
  report's framing that read-only evaluate replay is safe. Internal
  non-idempotent uses of evaluate (scroll, history back/forward) are a separate
  consideration; a target-loss mid-scroll/history is far less likely than during
  a click/submit, and gating them would require per-call idempotency on the raw
  evaluate surface. Not addressed here to keep the fix within this finding.

DEVANA-KEY: src/browser/backend.rs:940 | attach-recovery-replays-mutation
DEVANA-SUMMARY: fixed | P2 | medium | non-idempotent active-tab mutations (navigate/press_key/click/input/hover/select) no longer auto-replay after a recoverable attach-mode page-target loss; they surface a degraded error instead.
