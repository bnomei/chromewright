# Perf, Memory, and Security Review

This note is a static inspection of likely bottlenecks in the current codebase. It is based on code review, not runtime profiling, so the ranking reflects expected impact under normal MCP and agent-driven workloads rather than benchmark data.

## Highest Priority

1. Global browser-session mutex plus blocking waits serializes the whole MCP server
   - Evidence: `BrowserServer` stores `Arc<Mutex<BrowserSession>>` and each generated MCP tool wrapper holds the mutex guard for the full tool execution (`src/mcp/handler.rs:18-19`, `src/mcp/handler.rs:55-57`, `src/mcp/mod.rs:71-80`). Long-running paths sleep in place with `std::thread::sleep(...)` inside navigation and wait loops (`src/browser/session.rs:224-241`, `src/browser/session.rs:244-269`, `src/tools/wait.rs:140-168`, `src/tools/wait.rs:192-225`, `src/tools/markdown.rs:176-208`). The server binary also uses Tokio `current_thread` mode (`src/bin/mcp_server.rs:89`).
   - Why it matters: one slow `wait`, `navigate`, or `get_markdown` call can block every other tool invocation and reduce transport responsiveness. This is both a throughput and tail-latency problem.
   - Fix direction: replace the blocking mutex with an async-aware coordination model or move browser work to a dedicated worker thread; keep lock scopes narrow; replace `std::thread::sleep` with async waits or polling outside the critical section.

2. Full DOM extraction is the dominant CPU and allocation hot path, and it is refreshed aggressively
   - Evidence: `ToolContext::refresh_dom()` always invalidates and re-extracts the full DOM (`src/tools/mod.rs:90-106`). Extraction runs a large JS snapshot script and round-trips a JSON string back into Rust for parsing (`src/browser/session.rs:273-280`, `src/dom/tree.rs:127-172`). The script walks the DOM, computes styles and bounding boxes, expands same-origin iframes, and serializes the whole result (`src/dom/extract_dom.js:3`, `src/dom/extract_dom.js:54-113`, `src/dom/extract_dom.js:438-565`, `src/dom/extract_dom.js:569-621`, `src/dom/extract_dom.js:711-760`).
   - Why it matters: on large or dynamic pages this is a lot of JS work, JSON allocation, CDP transfer, and Rust deserialization for every refresh.
   - Fix direction: split "cheap revision probe" from "full snapshot"; cache by document revision; only rebuild selectors and node summaries when a caller explicitly asks for a snapshot.

3. `wait(revision_changed)` remains a narrower follow-up hotspot after the landed wait-predicate split
   - Evidence: `WaitCondition::RevisionChanged` still polls every 50 ms and compares frame-aware metadata revisions (`src/tools/wait.rs:137-188`), while the other wait conditions now dispatch through condition-specific browser predicates instead of one monolithic state bundle (`src/tools/wait.rs:247-259`, `src/tools/wait.rs:284-353`).
   - Why it matters: the broad "compute visibility, text, and value for every wait condition" issue is closed, but revision polling is still a steady-state loop worth revisiting if it shows up in profiling.
   - Fix direction: if profiling justifies more work here, add a lighter-weight revision-token probe instead of routing through the general document metadata path.

4. Successful tool responses rebuild large document envelopes even when callers may only need success or failure
   - Evidence: `build_document_envelope()` clones document metadata, optionally renders the full YAML snapshot, rebuilds the agent-facing node list, and recounts interactives (`src/tools/mod.rs:339-351`). Mutating tools like `navigate`, `click`, `input`, and `select` all refresh the DOM and then build this envelope, usually with `include_snapshot = true` (`src/tools/navigate.rs:49-56`, `src/tools/click.rs:62-68`, `src/tools/input.rs:79-88`, `src/tools/select.rs:86-99`).
   - Why it matters: each successful action does several full-tree passes and allocates large strings and vectors. That increases latency and heap pressure for routine clicks and form fills.
   - Fix direction: make snapshot and node lists opt-in; return only `document_id`, `revision`, and target metadata by default; precompute node summaries during extraction if they are frequently needed.

## Landed In `frame-aware-metadata-and-wait-polling`

- Frame-aware metadata reads no longer rescan the full same-origin iframe tree on every steady-state call.
  - Landed work: `src/dom/document_metadata.js` now keeps an in-page frame tracker that separates frame discovery from live revision sampling and rebuilds only when iframe membership or navigation invalidates the tracker.
  - Residual note: full DOM extraction is still expensive when a caller explicitly asks for a snapshot, but minimal metadata reads no longer pay the recursive iframe walk on every call.

- Wait polling no longer computes one monolithic visibility/text/value bundle for every condition.
  - Landed work: `src/tools/wait.rs` now emits condition-specific browser predicates so `present`, `enabled`, `editable`, `text_contains`, and `value_equals` only read the fields they need, while `visible` keeps the layout/style-sensitive path.
  - Residual note: the remaining wait-specific hotspot is `revision_changed`, which is now the exception rather than the default behavior for every wait condition.

## Medium Priority

5. Active-tab discovery is O(number of tabs) and relies on JS evaluation on every lookup
   - Evidence: `tab()` delegates to `get_active_tab()`, which clones the tab list and evaluates visibility and focus JS across every tab, with a second pass if needed (`src/browser/session.rs:101-169`). Many helpers call `self.tab()?` repeatedly, including navigation readiness checks (`src/browser/session.rs:205-241`) and wait condition evaluation (`src/tools/wait.rs:260-278`). `tab_list` separately scans tabs again after fetching the active tab (`src/tools/tab_list.rs:39-85`).
   - Why it matters: with many open tabs, seemingly cheap operations accumulate multiple cross-process round trips and repeated tab-list clones.
   - Fix direction: keep a cached active-tab identifier updated by `new_tab`, `switch_tab`, and activation events, and fall back to probing only when the cached handle is invalid.

6. Markdown extraction always rebuilds the full article, even for later pages
   - Evidence: `GetMarkdownTool` injects `Readability.min.js` plus the conversion script on every call (`src/tools/markdown.rs:59-72`), then converts the full extracted HTML to full markdown before slicing out the requested page (`src/tools/markdown.rs:113-172`). It also polls document text length with blocking sleeps before extraction (`src/tools/markdown.rs:176-208`).
   - Why it matters: requesting page 2 or 3 still pays for full extraction and conversion of the whole document. Repeated pagination requests multiply both CPU time and memory usage.
   - Fix direction: cache markdown and extraction results per `document.revision`; paginate cached content; avoid rebuilding the same JS payload every call.

7. Input clearing is implemented as a naive Backspace loop
   - Evidence: when `clear` is set, the tool sends `End` and then `text.len() + 100` Backspace events (`src/tools/input.rs:63-69`).
   - Why it matters: this scales poorly for long text, does unnecessary browser round trips, and is incorrect when existing field contents are longer or shorter than the new text.
   - Fix direction: use a direct JS value replacement for supported inputs, or a true select-all plus delete strategy.

## Security Findings

8. The default MCP surface can navigate to privileged or local schemes
   - Evidence: `normalize_url()` explicitly allows `file://`, `data:`, `about:`, `chrome://`, and `chrome-extension://` (`src/tools/utils.rs:5-15`), and `browser_navigate` is on the default MCP surface (`src/tools/mod.rs:428-455`, `src/mcp/mod.rs:87-95`).
   - Why it matters: if an untrusted MCP client gets access, it can attempt local-file reads or privileged-browser navigation and then combine that with snapshot and extraction tools. Some pages will still be browser-protected, but the default policy is much broader than a normal web-only browser automation surface.
   - Fix direction: default-deny non-HTTP(S) schemes on the MCP surface and gate local or privileged schemes behind an explicit trusted-mode option.

9. Opt-in operator tools expand the blast radius to arbitrary JS execution and arbitrary file writes
   - Evidence: `register_operator_tools()` adds raw `evaluate` and filesystem-bound `screenshot` tools (`src/tools/mod.rs:421-463`). `EvaluateTool` runs arbitrary JS (`src/tools/evaluate.rs:27-42`), and `ScreenshotTool` writes to any supplied path (`src/tools/screenshot.rs:26-50`).
   - Why it matters: this is acceptable only for highly trusted callers. If a consumer enables `with_all_tools()` for convenience, compromise scope jumps from browser automation to local filesystem writes and unrestricted page-side code execution.
   - Fix direction: keep these tools opt-in, add prominent warnings, and consider path allowlists and per-tool capability flags.

## Lower-Priority Memory Note

10. DOM revision tracking and frame invalidation add always-on observers to same-origin documents
   - Evidence: the extraction and metadata scripts install `__browserUseDocumentState` with a `MutationObserver` over `subtree`, `childList`, `attributes`, and `characterData` (`src/dom/extract_dom.js:20-108`, `src/dom/document_metadata.js:18-311`), including same-origin frame documents when expanded or tracked.
   - Why it matters: the callbacks are cheap, but on very dynamic pages they add background work to every DOM mutation and keep extra per-document state alive for the page lifetime.
   - Fix direction: keep this for now because it underpins the landed revision and frame-tracker behavior, but do not treat it as free; if extraction or revision polling moves again, re-check whether the observer footprint is still the right tradeoff.

## Suggested Order Of Attack

1. Stop returning full document envelopes for every successful action by default.
2. Rework the MCP session lock and blocking sleeps so one call cannot stall the whole server.
3. Cache active tab state instead of probing all tabs on each lookup.
4. Cache markdown extraction by document revision.
5. Revisit `wait(revision_changed)` only if profiling shows it is still materially hot after the landed condition-specific predicate work.
6. Tighten scheme handling and keep operator tools heavily gated.
