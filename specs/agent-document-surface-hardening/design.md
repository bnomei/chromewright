# Design — agent-document-surface-hardening

## Why This Is A Spec

This work is bounded enough for a spec rather than a loop:

- the failure modes are already visible in the current code
- the user wants document-surface improvements before wider tab work
- the desired outcome is a clearer contract, not an open-ended exploration
- the acceptance criteria can be expressed as concrete tool and test behavior

## Scope Boundary

This spec intentionally does not solve tab ownership first.

- In scope: the document model inside the targeted tab/frame.
- Out of scope: stable tab IDs, background-tab orchestration, and removal of the current-tab abstraction across the whole session layer.

Reason:

- many of the current agent failures are caused by the document contract itself, not only by tab selection
- a better document model is reusable whether tab addressing remains current-tab based for one more wave or moves to stable tab IDs later

## Current Findings

### 1. The current element identity model is unstable

Evidence:

- `ToolContext` exposes a cached `DomTree` at `src/tools/mod.rs:63`.
- the public docs encourage reusing a single `ToolContext` across multiple tool calls at `src/lib.rs:59`.
- DOM indices are assigned broadly in `src/dom/extract_dom.js:279`, including container-like roles such as `article`, `region`, `heading`, `listitem`, and `generic`.
- selector recovery is heuristic in `src/dom/extract_dom.js:650`; it prefers `#id`, otherwise tag plus first class plus occasional `:nth-child(...)`.

Implication:

- an agent targets numeric indices, but execution ultimately depends on a fragile selector map
- the index space is noisier than "actionable controls", which increases planning error
- direct Rust consumers can accidentally reuse stale document state after mutations

### 2. Post-action state is inconsistent and too weak

Evidence:

- `click` returns only selector/index metadata at `src/tools/click.rs:49`.
- `navigate` returns only a snapshot at `src/tools/navigate.rs:50`.
- `input` manually invalidates DOM and returns a fresh snapshot at `src/tools/input.rs:70`.
- `select` returns selector/value/selected text only at `src/tools/select.rs:76`.

Implication:

- tools do not share a stable "what changed in the document" contract
- agents must guess whether to resnapshot, whether navigation happened, and whether a cached reference is still usable

### 3. Wait semantics are underpowered and timing-dependent

Evidence:

- `wait` only waits for CSS-selector existence at `src/tools/wait.rs:31`.
- `go_back` and `go_forward` rely on JavaScript history calls plus a fixed sleep starting at `src/browser/session.rs:243`.
- `get_markdown` waits with a fixed 1000 ms sleep at `src/tools/markdown.rs:52`.

Implication:

- success depends on timing heuristics rather than explicit postconditions
- agents do not get a reliable signal for "navigation settled", "value changed", or "target became interactable"

### 4. Frame-aware document assembly exists but is not wired into the main path

Evidence:

- DOM extraction collects iframe indices at `src/dom/extract_dom.js:627`.
- `DomTree::assemble_with_iframes(...)` exists at `src/dom/tree.rs:211`.
- `BrowserSession::extract_dom()` still uses `DomTree::from_tab(&self.tab()?)` at `src/browser/session.rs:203`.

Implication:

- the repository has partial infrastructure for multi-frame document assembly
- agents currently see an incomplete document model for iframe-heavy pages

### 5. Advanced/operator tools are mixed into the same surface as high-level agent tools

Evidence:

- `evaluate` executes arbitrary JavaScript at `src/tools/evaluate.rs:27`.
- `screenshot` writes bytes to an arbitrary filesystem path at `src/tools/screenshot.rs:26`.

Implication:

- the default tool surface mixes safe, high-level document actions with low-level operator escape hatches
- this makes it harder to define a clean agent contract and harder to reason about postconditions

### 6. Test contracts are already drifting

Evidence:

- browser-backed tab tests remain ignored at `tests/tab_management_integration.rs:8`.
- `test_new_tab` still expects `url` and `message` at `tests/tab_management_integration.rs:46`, but the tool no longer returns that shape.
- browser tool tests explicitly accept index-based `select` failure at `tests/browser_tools_integration.rs:256`.

Implication:

- the tool contract is not encoded strongly enough in tests
- document-surface work must land with new browser-backed contract tests, not only unit coverage

## Major Design Decisions

### 1. Introduce revision-scoped `NodeRef`s instead of promising globally stable element IDs

Recommended first step:

- make element references stable within one extracted document revision
- reject stale references once the document revision changes

Do not do this in the first wave:

- promise that the same node keeps the same ID across arbitrary DOM mutations or navigation

Reason:

- revision-scoped references are enough to make agent behavior deterministic
- cross-mutation persistent identity is much more complex and should not block the first hardening pass

### 2. Treat CSS selectors as an internal implementation detail

The public agent contract should target:

- `NodeRef`
- explicit document metadata
- structured stale-reference errors

The implementation may continue to use CSS selectors internally during migration, but selector strings should stop being the primary external identity model.

### 3. Normalize high-level tool results around a document envelope

High-level document tools should converge on a shared result shape such as:

```json
{
  "document": {
    "document_id": "doc_...",
    "revision": 7,
    "url": "https://example.com",
    "title": "Example",
    "ready_state": "complete"
  },
  "target": {
    "node_ref": "node_...",
    "status": "applied"
  },
  "snapshot": "... optional AI rendering ..."
}
```

Exact field names can change during implementation, but the contract must consistently report:

- which document the tool acted on
- which revision resulted
- whether the targeted node remained valid
- whether a new snapshot is returned

### 4. Centralize DOM cache invalidation and stale-ref handling

The current model leaves invalidation to individual tools. That should move into shared logic so that:

- mutations always advance or invalidate the cached revision
- post-mutation lookups cannot silently reuse pre-mutation DOM
- direct `ToolContext` users get the same correctness guarantees as MCP users

### 5. Replace "wait for selector exists" with explicit document predicates

The wait model should support predicates such as:

- navigation settled
- node exists
- node visible
- node editable/enabled
- node text/value changed
- document revision advanced

This gives mutation tools a better postcondition language and reduces the need for fixed sleeps.

### 6. Make frame boundaries explicit in the document model

The document snapshot should either:

- assemble accessible same-origin iframe content into the main tree with frame markers, or
- keep frame nodes explicit and report which frames could not be expanded

What should not happen:

- silently pretending the visible page is single-document when the extraction path knows about iframe boundaries

### 7. Separate the default agent contract from escape hatches

Default high-level tools:

- snapshot
- click
- input
- select
- navigate
- go_back
- go_forward
- wait

Advanced/operator tools:

- evaluate
- raw filesystem screenshot capture

These can remain available, but they should not define the baseline document interaction model.

## Proposed Touchpoints

- `src/dom/extract_dom.js`
- `src/dom/tree.rs`
- `src/browser/session.rs`
- `src/tools/mod.rs`
- `src/tools/snapshot.rs`
- `src/tools/click.rs`
- `src/tools/input.rs`
- `src/tools/select.rs`
- `src/tools/navigate.rs`
- `src/tools/go_back.rs`
- `src/tools/go_forward.rs`
- `src/tools/wait.rs`
- `src/tools/markdown.rs`
- `src/lib.rs`
- `tests/browser_tools_integration.rs`
- `tests/dom_integration.rs`
- `tests/navigation_integration.rs`

## Verification Plan

The first execution wave should prove one critical path end to end:

1. create a snapshot
2. obtain a `NodeRef`
3. act on that node
4. observe either a new document revision or a structured stale-reference error
5. verify the new contract in browser-backed integration tests

If that path becomes reliable for `snapshot`, `click`, `input`, and `select`, the architecture direction is validated before any follow-on tab-addressing work.
