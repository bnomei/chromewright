# Design — performance-and-surface-hardening

## Summary

This wave removes the most obvious avoidable costs called out in `docs/perf.md` without changing
the revision-scoped document model introduced by `agent-document-surface-hardening`.

The implementation has four structural themes:

1. make cheap metadata and revision reads first-class
2. stop rebuilding full DOM snapshots for ordinary action responses
3. reduce repeated browser-side work with tab and markdown caches
4. require explicit unsafe opt-ins on high-risk tool paths

## Distilled discovery

- The current MCP wrapper stores `Arc<Mutex<BrowserSession>>` and every tool wrapper locks it for the full call, while several paths sleep in-place (`src/mcp/handler.rs`, `src/mcp/mod.rs`, `src/tools/wait.rs`, `src/tools/markdown.rs`, `src/browser/session.rs`).
- `ToolContext::refresh_dom()` always rebuilds a full `DomTree`, and most action tools call it before constructing result payloads (`src/tools/mod.rs`, `src/tools/click.rs`, `src/tools/input.rs`, `src/tools/select.rs`, `src/tools/navigate.rs`, `src/tools/go_back.rs`, `src/tools/go_forward.rs`).
- `WaitCondition::RevisionChanged` currently calls `refresh_dom()` every 50 ms until success or timeout (`src/tools/wait.rs`).
- Active-tab resolution probes every tab with JavaScript each time `tab()` is called (`src/browser/session.rs`), and tab-listing does an additional scan (`src/tools/tab_list.rs`).
- Markdown extraction always reruns readiness polling, browser-side readability extraction, and Rust markdown conversion before paginating (`src/tools/markdown.rs`).
- High-level `navigate` and `new_tab` share `normalize_url`, which currently accepts unsafe schemes without an explicit opt-in (`src/tools/utils.rs`, `src/tools/navigate.rs`, `src/tools/new_tab.rs`).
- Operator tools are already opt-in at registry construction time, but once enabled they do not require per-call acknowledgement and screenshots can write to arbitrary paths (`src/tools/mod.rs`, `src/tools/evaluate.rs`, `src/tools/screenshot.rs`).

## Design decisions

### 1. Add a lightweight document metadata/revision path

Introduce a dedicated browser-side metadata script that returns:

- `document_id`
- `revision`
- `url`
- `title`
- `ready_state`
- frame revision/status metadata for same-origin iframes

This script reuses the document-state observer model but does not walk the full DOM tree or build
selectors/nodes. It becomes the source of truth for:

- post-action metadata-only responses
- `wait(revision_changed)`
- markdown cache keys

The existing full DOM extraction path remains the source of truth for snapshots and actionable
node lists.

### 2. Split document-envelope construction into minimal vs full modes

Replace the current boolean `include_snapshot` shape with an explicit envelope options struct.

Two modes matter in this wave:

- minimal:
  - fetch fresh `DocumentMetadata`
  - include target metadata when available
  - omit snapshot text, node summaries, and interactive counts
- full:
  - fetch or reuse the full `DomTree`
  - include snapshot, node summaries, and interactive count

Default high-level action tools move to minimal mode. The snapshot tool remains full mode. This
keeps the public contract stable at the document/target layer while dropping the expensive payload
defaults for ordinary actions.

### 3. Invalidate stale DOM caches without eagerly rebuilding them

After mutations, tools should invalidate the cached `DomTree` but avoid rebuilding it unless they
need the full snapshot surface immediately afterward.

Implications:

- `click`, `input`, `select`, `navigate`, `go_back`, `go_forward`, and successful `wait` results use
  minimal envelopes after invalidation.
- snapshot-oriented paths still force full extraction.

### 4. Cache the active tab hint

Add a validated active-tab hint inside `BrowserSession`.

Read path:

1. if a cached `Arc<Tab>` still exists in the current tab list, use it
2. otherwise probe tabs via JavaScript and refresh the cache

Write path:

- launch initial tab seeds the hint
- `new_tab`, `switch_tab`, and activation-aware helpers update the hint
- closing the active tab clears the hint

This preserves current fallback behavior while removing repeated full-tab probes from steady-state
tool execution.

### 5. Cache markdown by document revision

Store one markdown cache entry in `BrowserSession`, keyed by `document_id` and `revision`.

Cache contents:

- extracted markdown
- title/url/byline/excerpt/site name fields already returned by the tool

Behavior:

- on cache hit for the current revision, paginate cached markdown only
- on cache miss, run settle + readability extraction + conversion once, then store the result

This avoids redoing the expensive extraction path for page 2, page 3, or repeated reads of an
unchanged document.

### 6. Replace input clear key loops with direct element clearing

When `clear = true`, run a small JavaScript helper against the resolved selector:

- if the element supports `.value`, set it to `""`
- if the element is content-editable, clear its text content
- dispatch `input` and `change` events where appropriate

Then type the requested text normally. This avoids `O(text.len())` backspace loops and behaves
more predictably against long existing content.

### 7. Require explicit unsafe opt-in for high-level navigation

Keep URL normalization separate from scheme policy.

High-level `navigate` and `new_tab` gain an explicit `allow_unsafe` flag. By default, they reject
unsafe schemes such as:

- `file:`
- `chrome:`
- `chrome-extension:`
- other non-web absolute schemes unless explicitly allowed

The implementation may keep `data:` and `about:blank` behind the same explicit opt-in path so test
and advanced usage remain possible without leaving the default MCP/document contract permissive.

`BrowserSession::navigate` stays low-level and does not become the policy boundary for this wave;
the hardening target is the high-level tool surface.

### 8. Require explicit unsafe opt-in for operator tools

Operator tools remain opt-in at registry construction time and also require per-call acknowledgement.

- `evaluate` adds an explicit unsafe acknowledgement field
- `screenshot` adds an explicit unsafe acknowledgement field
- `screenshot` restricts output to a safe relative path rooted under the current working directory

This does not make operator tools "safe", but it narrows accidental misuse and aligns the tool
contract with the repo documentation.

### 9. Runtime/MCP hardening

Move the MCP server runtime off `current_thread` to Tokio `multi_thread`.

The preferred structural improvement is to share `BrowserSession` without a coarse per-tool mutex
when the type graph allows it. If that is not type-safe with `headless_chrome`, the minimum
acceptable change in this wave is:

- do not bind all server progress to a current-thread runtime
- keep locking as narrow as the type graph safely permits

This requirement is intentionally framed around observable runtime behavior rather than forcing a
specific internal concurrency primitive if the dependency types constrain the implementation.

## Data flow

### Minimal action response

```text
tool executes browser action
-> invalidate cached DomTree
-> fetch lightweight DocumentMetadata
-> build minimal DocumentEnvelope
-> return metadata-first result
```

### Full snapshot response

```text
snapshot tool
-> fetch/reuse full DomTree
-> render YAML snapshot
-> compute actionable nodes
-> return full DocumentEnvelope
```

### Revision wait

```text
wait(revision_changed)
-> read current lightweight revision token
-> poll lightweight revision token
-> on change, fetch lightweight DocumentMetadata
-> return minimal DocumentEnvelope
```

### Markdown cache

```text
get_markdown
-> fetch lightweight DocumentMetadata
-> cache hit for (document_id, revision)?
   -> yes: paginate cached markdown
   -> no: settle, extract once, convert once, cache, paginate
```

## Verification plan

- unit tests for unsafe navigation classification and safe screenshot path validation
- unit tests for markdown cache hit behavior where practical
- browser-backed integration tests for:
  - action tools returning metadata-first envelopes without snapshots by default
  - `wait(revision_changed)` succeeding without full snapshot reconstruction semantics regressions
  - tab switching/listing using the cached active tab correctly
  - `new_tab`/`navigate` rejecting unsafe schemes unless explicitly allowed
  - operator tools rejecting calls without explicit unsafe acknowledgement

## Out of scope

- benchmark-driven keep/revert performance tuning loops
- replacing `headless_chrome`
- cross-session or persistent markdown caches
- a full capability/permissions framework for every tool
