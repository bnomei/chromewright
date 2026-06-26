DEVANA-FINDING: v1
DEVANA-STATE: fixed | P2 | high | security=no
DEVANA-KEY: src/tools/browser_kernel.js:357 | interaction-selector-first-wrong-node

# Interaction tools resolve targets selector-first, ignoring the disambiguating index/cursor/node_ref

## Finding

`click`, `input`, `select`, and `hover` are designed to be targeted precisely by a
revision-scoped `cursor`/`node_ref` or by `index` — the index is the agent's stable
handle for *which* of several matching elements to act on. But the in-page resolver
is selector-first and treats the index only as a fallback.

In `src/tools/click.rs:142-154` the tool dispatches an interaction with both the
resolved `selector` and a `target_index` (`cursor.index` or `index`). In the page,
`resolveTargetMatch(config)` (`src/tools/browser_kernel.js:354-385`) does:

```js
if (config.selector) {
  selectorSearch = querySelectorAcrossScopes(config.selector, ...);
  if (selectorMatch && selectorMatch.element && selectorMatch.element.isConnected) {
    return { match: selectorMatch, ... };          // <-- returns FIRST selector match
  }
}
if (typeof config.target_index === 'number') {
  return { match: searchActionableIndex(config.target_index), ... };  // only if selector missed
}
```

So `target_index` is consulted **only when the selector fails to match any connected
element**. Whenever the selector matches the first element in document order, that
element is acted on regardless of the index the caller passed. `resolveTargetElement`
(`browser_kernel.js:387-395`) and `click.js:6,23` then call `.click()` on it.

The selectors are not guaranteed unique. `buildSelector`
(`src/dom/extract_dom.js:987-1021`) returns `'#' + id` for **any** element with an
`id`, with no uniqueness check (duplicate `id`s are legal HTML and `querySelector`
returns the first), and the path fallback only appends `:nth-child` for same-tagName
siblings and stops at `document.body`, so two distinct subtrees can yield identical
selectors.

The same selector-first resolver backs `input.js`, `select.js`, and `hover.js`, so
the defect is shared across all four mutating interaction tools.

## Violated Invariant Or Contract

When a caller targets an actionable element by `index`, `node_ref`, or `cursor`, the
action must operate on *that indexed node*. The index/cursor is the disambiguator
precisely for the case where a selector is ambiguous.

## Oracle

`inspect_node` already guards against exactly this divergence:
`reconcile_target_with_probe` / `probe_matches_actionable_target`
(`src/tools/services/inspection.rs`) downgrades a resolved target to a selector-only
result when `payload.actionable_index != fingerprint.index`. The codebase treats
selector-first-vs-index divergence as a real hazard for the read path, but the
mutating tools (`click`/`input`/`select`/`hover`) apply no equivalent check before
committing the action.

## Counterexample

DOM with two actionable buttons sharing an `id` (legal, common in templated pages):

```html
<button id="save">Save draft</button>     <!-- actionable index 0, document order first -->
<button id="save">Delete account</button> <!-- actionable index 1 -->
```

1. `snapshot` records both; `buildSelector` gives each `selector = "#save"`.
2. Agent calls `click` with the `cursor`/`node_ref` for index 1 (or `index: 1`).
3. `resolve_interaction_target` → `ResolvedTarget { selector: "#save", target_index: 1 }`
   (`click.rs:114-153`).
4. In-page `resolveTargetMatch`: `querySelectorAcrossScopes("#save")` returns the
   **first** `#save` (index 0, "Save draft"), which is connected, so `target_index: 1`
   is never used.
5. `click.js:23` clicks "Save draft". The agent asked for index 1 ("Delete account");
   index 0 is activated instead, and the tool returns `{ success: true }`.

## Why It Might Matter

The action lands on the wrong element and is reported as success, with no signal to
the agent. When the colliding elements have different consequences (save vs delete,
two rows' action buttons, repeated form controls), this is a silent
wrong-action/data-integrity outcome on pages with non-unique selectors.

## Proof

- Control-flow trace: `click.rs:142` → `TargetedInteractionRequest{selector,target_index}`
  → `browser_kernel.js:357` selector-first return → `click.js:23` clicks first match.
- Contract mismatch: index/cursor targeting promises a specific node; the resolver
  honors the selector and discards the index when the selector matches.
- Cross-entry mismatch: `inspect_node` reconciles `actionable_index` vs resolved
  element; `click`/`input`/`select`/`hover` do not.

## Counterevidence Checked

- `StaleCursorPolicy::DenyRebind` does not apply: the cursor is current (matching
  revision); it is merely non-unique, so the stale path is never entered.
- Revision/`MutationObserver` bumps do not help: this is a same-revision,
  same-snapshot ambiguity, not a staleness case.
- Bounds checks pass: the index is in range and `cursor_for_index` succeeds; the
  index is simply ignored downstream.
- `wait_for_actionability` runs the same selector+index probe first, but it is itself
  selector-first, so it validates and then clicks the *same* wrong element — it does
  not catch the mismatch.
- Strongest reason this might be false: on standards-compliant pages where
  `buildSelector` yields document-unique selectors for actionable elements, the first
  selector match coincides with the indexed element and the divergence cannot occur.
  The bug requires a non-unique selector (duplicate `id`, or colliding `:nth-child`
  path) — common in the wild but not guaranteed. This bounds frequency, not
  reachability: the resolver structurally cannot honor the index when the selector is
  ambiguous, and inspect_node's existing guard shows the maintainers consider the
  divergence real.

## Suggested Next Step

When `target_index` is supplied alongside a selector, verify the selector match's
actionable index equals `target_index` (reusing `findActionableIndexForElement`,
already present in `browser_kernel.js:219-230`) and fall back to
`searchActionableIndex(target_index)` on mismatch — or have the mutating tools apply
the same reconciliation `inspect_node` uses.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2
`DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` prefix. Keep `DEVANA-KEY:` stable
unless the same finding moved. Add dated notes below.

## Status Notes

- 2026-06-25: open by Devana. Static source inspection; selector-first resolution and
  non-unique `buildSelector` output both confirmed in code.
- 2026-06-26: fixed. Implemented the report's primary suggestion in the shared
  `resolveTargetMatch` (browser_kernel.js). When `target_index` is supplied
  alongside a selector and the first selector match is a main-frame actionable
  node (`frame_depth === 0`), its actionable index is computed with the existing
  `findActionableIndexForElement`; on divergence the resolver falls back to
  `searchActionableIndex(target_index)` so the action lands on the intended node
  instead of the first collision. iframe matches (`frame_depth > 0`) are trusted
  as-is because the actionable index is main-frame scoped, and an out-of-range
  index keeps the selector match as a best-effort fallback. Fixing it in the
  shared resolver fixes click/input/select/hover and scroll_target_into_view at
  once, and makes inspect_node's downstream reconcile a confirmation rather than
  a downgrade. The common unique-selector case is unchanged (indices match → no
  fallback). Added a kernel string-contains test for the reconciliation; all lib
  + runnable integration tests pass (real-DOM interaction tests are #[ignore]).
  Residual: `buildSelector` (extract_dom.js) can still emit non-unique selectors;
  that is the underlying cause but the resolver reconciliation makes it
  non-harmful for indexed targeting, so a buildSelector-uniqueness change is left
  as a separate improvement.

DEVANA-KEY: src/tools/browser_kernel.js:357 | interaction-selector-first-wrong-node
DEVANA-SUMMARY: fixed | P2 | high | resolveTargetMatch now reconciles a non-unique selector match against target_index, so click/input/select/hover act on the indexed node instead of the first selector collision.
