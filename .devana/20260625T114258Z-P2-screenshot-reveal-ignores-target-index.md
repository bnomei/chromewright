DEVANA-FINDING: v1
DEVANA-STATE: fixed | P2 | high | security=no
DEVANA-KEY: src/tools/screenshot.rs:385 | screenshot-reveal-ignores-target-index

# Screenshot element reveal ignores target_index and iframe scopes

## Finding

In `screenshot` element mode, when the target is off-screen the tool scrolls it into view via `build_reveal_target_js`, which resolves the element with plain `document.querySelector(selector)` only. The subsequent inspect and capture paths use `build_inspect_node_js` with the browser kernel, which honors `target_index` and `querySelectorAcrossScopes` (including iframes). Reveal and capture can therefore disagree about which element was scrolled.

## Violated Invariant Or Contract

Element-mode screenshot should reveal and clip the same DOM node that `inspect_node` would resolve for the provided `target` (selector or cursor), including disambiguation by `target_index` and cross-frame lookup consistent with other DOM tools.

## Oracle

`inspect_node.js` and `browser_kernel.js` use `resolveTargetMatch` / `querySelectorAcrossScopes` with `target_index`. Interaction tools (`click.js`, `scroll_target_into_view.js`) use the same kernel. `screenshot.rs` tests assert inspect uses `searchActionableIndex` but reveal is a separate inline script.

## Counterexample

Active page has two buttons sharing selector `#actions > button:nth-child(1)` at actionable indices 2 and 5; index 5 is below the fold. Call `screenshot` with `mode: "element"` and `target: { "kind": "cursor", "cursor": <index 5 cursor> }`. Reveal runs `querySelector` and scrolls the first matching button (index 2). Inspect runs with `target_index: 5` and returns layout for index 5, which remains off-screen. The tool fails with `target_not_in_viewport` or, if layout checks pass inconsistently, could clip the wrong box.

For an element inside a same-origin iframe, inspect can resolve via `querySelectorAcrossScopes` while reveal's `document.querySelector` returns null or a main-document collision, causing reveal failure despite a valid cursor.

## Why It Might Matter

Agents following snapshot cursors for off-screen element screenshots get flaky failures or wrong crops on pages with duplicate selector strings or iframe-hosted targets—cases other tools handle via the shared kernel.

## Proof

Control-flow trace: `inspect_element_target` → `resolve_target_with_cursor` (carries `target_index`) → off-screen branch → `reveal_target_in_viewport(&target.selector, ...)` (drops index) → `build_reveal_target_js` (`document.querySelector` only) → later `inspect_target_payload(..., target_index, ...)` with kernel-based resolution.

Contract mismatch: same tool, same target, two different resolution algorithms on the reveal vs inspect steps.

## Counterevidence Checked

`StaleCursorPolicy::DenyRebind` prevents stale cursor rebound in screenshot mode, but does not fix index/iframe skew on reveal. Viewport/full_page/region modes do not use reveal. When the target is already in the viewport, reveal is skipped and the bug does not trigger.

## Suggested Next Step

Reuse `scroll_target_into_view.js` / `resolveTargetElement` (with `selector` and `target_index`) for reveal, or route reveal through `evaluate_on_tab` with the same config as `inspect_target_payload`.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. Screenshot element-mode reveal now resolves through the
  shared browser kernel instead of a bare `document.querySelector`. Added
  `src/tools/screenshot_reveal_target.js` (kernel template using
  `resolveTargetElement(config)`), rendered via `render_browser_kernel_script`
  with config `{selector, target_index}`. `reveal_target_in_viewport` /
  `build_reveal_target_js` now take `target_index`, which the element path
  already computes for `inspect_target_payload`. Verified that inspect and
  reveal share `resolveTargetMatch` (inspect_node.js:306, browser_kernel.js:354),
  so they now resolve the same node — honoring `target_index` disambiguation and
  cross-frame (iframe) `querySelectorAcrossScopes` lookup. The rich reveal
  payload (scroll_y_before/after, visible_in_viewport, success/code) is
  preserved for failure reporting. Added a unit test asserting the reveal JS
  embeds the kernel and target_index and no longer uses plain querySelector.
- 2026-06-26: reopened by audit. The reveal-specific `document.querySelector`
  mismatch is fixed: reveal and inspect now share `resolveTargetMatch` with
  `{selector, target_index}`. A residual same-origin iframe duplicate-selector path
  remains because the shared resolver trusts iframe selector matches without
  checking their actionable index. For an offscreen second `<button id="save">`
  inside a same-origin iframe, reveal and inspect can both resolve the first iframe
  `#save` collision while reporting/using the requested `target_index`, because
  `resolveTargetMatch` skips reconciliation for `frame_depth > 0` and
  `inspect_node.js` reports `config.target_index` for iframe matches instead of
  computing the resolved element's actual actionable index.
- 2026-06-26: fixed. The shared `resolveTargetMatch` now reconciles
  `target_index` for selector matches regardless of `frame_depth`, so
  screenshot reveal and inspect both inherit the same iframe-aware disambiguation.
  The reopened offscreen second-`#save` same-origin iframe path is blocked: the
  first iframe selector match is compared to the requested actionable index and
  falls back to `searchActionableIndex(target_index)` on mismatch before reveal or
  inspect uses the element.

DEVANA-KEY: src/tools/screenshot.rs:385 | screenshot-reveal-ignores-target-index
DEVANA-SUMMARY: fixed | P2 | high | element screenshot reveal uses the shared kernel, whose target_index reconciliation now covers same-origin iframe selector collisions too.
