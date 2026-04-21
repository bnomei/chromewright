# Tasks — agent-document-surface-hardening

Meta:
- Spec: agent-document-surface-hardening — harden the agent-facing document interaction contract before tab-addressing work
- Depends on: `specs/dependency-modernization-and-architecture/`
- Global scope:
  - `src/dom/`
  - `src/browser/`
  - `src/tools/`
  - `src/lib.rs`
  - `tests/`
  - `specs/agent-document-surface-hardening/`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T007: Refresh browser-backed contract tests around the new document model (owner: mayor) (scope: `tests/`, `src/tools/`, `src/dom/`) (depends: T001, T002, T003, T004, T005)
  - Covers: R009
  - Verification_mode: required
  - Verification_status: passed
  - DoD: integration tests lock in the intended document contract and remove the current tolerance for drifting or optional index behavior.
  - Validation: `cargo test --locked --all-features`, `cargo test --locked --all-features --test dom_integration test_snapshot_tool_exposes_document_metadata_and_node_refs -- --ignored --exact`, `cargo test --locked --all-features --test dom_integration test_stale_node_ref_returns_structured_failure -- --ignored --exact`, `cargo test --locked --all-features --test dom_integration test_same_origin_iframe_content_is_included_in_snapshot -- --ignored --exact`, `cargo test --locked --all-features --test browser_tools_integration test_wait_tool_text_contains -- --ignored --exact`, `cargo test --locked --all-features --test markdown_integration test_markdown_extraction_waits_for_delayed_content -- --ignored --exact`, `cargo test --locked --all-features --test navigation_integration test_wait_tool_navigation_settled -- --ignored --exact`
  - Notes: Replaced the old “index may fail and that is acceptable” expectations with explicit node-ref, stale-ref, wait-predicate, delayed-markdown, and iframe-snapshot contract tests.

- [x] T006: Separate advanced/operator surfaces from the default high-level agent contract (owner: mayor) (scope: `src/tools/evaluate.rs`, `src/tools/screenshot.rs`, `src/tools/mod.rs`, `src/lib.rs`, `README.md`, `tests/`) (depends: T003)
  - Covers: R008
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: docs and tool registration make the default document interaction surface distinct from raw JS evaluation and filesystem-bound screenshot capture.
  - Validation: `cargo test --locked --all-features`
  - Notes: The default `ToolRegistry` and MCP surface now describe the high-level document contract only, while raw JS evaluation and filesystem-bound screenshots require explicit `register_operator_tools()` / `with_all_tools()` opt-in for direct Rust consumers.

- [x] T005: Wire iframe-aware document assembly into the snapshot path (owner: mayor) (scope: `src/dom/`, `src/browser/session.rs`, `src/tools/snapshot.rs`, `tests/dom_integration.rs`) (depends: T001)
  - Covers: R006
  - Verification_mode: required
  - Verification_status: passed
  - DoD: same-origin iframe content is either assembled into the document view with explicit frame boundaries or reported as unavailable in a structured way.
  - Validation: `cargo test --locked --all-features`, `cargo test --locked --all-features --test dom_integration test_same_origin_iframe_content_is_included_in_snapshot -- --ignored --exact`
  - Notes: Same-origin iframe content is now expanded into the snapshot tree with frame-status metadata, while inaccessible frames are reported explicitly in document metadata.

- [x] T004: Replace timing heuristics with explicit document wait predicates (owner: mayor) (scope: `src/tools/wait.rs`, `src/browser/session.rs`, `src/tools/markdown.rs`, `tests/navigation_integration.rs`, `tests/markdown_integration.rs`) (depends: T003)
  - Covers: R005
  - Verification_mode: required
  - Verification_status: passed
  - DoD: the wait path can express navigation settle and node-state predicates, and the fixed sleeps in scoped paths are removed or reduced to bounded fallbacks with explicit rationale.
  - Validation: `cargo test --locked --all-features`, `cargo test --locked --all-features --test browser_tools_integration test_wait_tool_text_contains -- --ignored --exact`, `cargo test --locked --all-features --test navigation_integration test_wait_tool_navigation_settled -- --ignored --exact`, `cargo test --locked --all-features --test markdown_integration test_markdown_extraction_waits_for_delayed_content -- --ignored --exact`
  - Notes: Added explicit wait predicates, ready-state polling for navigation/history flow, and bounded markdown settle checks for delayed content instead of a fixed one-second sleep.

- [x] T003: Normalize post-action document envelopes for high-level tools (owner: mayor) (scope: `src/tools/click.rs`, `src/tools/input.rs`, `src/tools/select.rs`, `src/tools/navigate.rs`, `src/tools/go_back.rs`, `src/tools/go_forward.rs`, `src/mcp/`, `tests/browser_tools_integration.rs`, `tests/navigation_integration.rs`) (depends: T002)
  - Covers: R004
  - Verification_mode: required
  - Verification_status: passed
  - DoD: high-level document tools return a shared result shape that includes document metadata and resulting revision; tool-specific details are additive, not bespoke.
  - Validation: `cargo test --locked --all-features`, `cargo check --all-features --all-targets --locked`
  - Notes: High-level document actions now return a shared document envelope with metadata, revision, actionable nodes, and optional snapshots, and the MCP adapter preserves structured tool errors when failures include details.

- [x] T002: Reject stale node references and centralize DOM cache invalidation (owner: mayor) (scope: `src/tools/mod.rs`, `src/tools/click.rs`, `src/tools/input.rs`, `src/tools/select.rs`, `src/tools/hover.rs`, `src/lib.rs`, `tests/browser_tools_integration.rs`) (depends: T001)
  - Covers: R002, R007
  - Verification_mode: required
  - Verification_status: passed
  - DoD: high-level tools fail with structured stale-reference errors when given outdated node refs, and `ToolContext` no longer returns stale DOM after mutations.
  - Validation: `cargo test --locked --all-features`, `cargo test --locked --all-features --test dom_integration test_stale_node_ref_returns_structured_failure -- --ignored --exact`
  - Notes: Added revision-scoped target resolution, structured stale-node-ref failures, and shared DOM refresh behavior after high-level mutations.

- [x] T001: Introduce revision-scoped document identity and `NodeRef` targeting (owner: mayor) (scope: `src/dom/`, `src/tools/snapshot.rs`, `src/tools/mod.rs`, `tests/dom_integration.rs`) (depends: -)
  - Covers: R001, R003
  - Verification_mode: required
  - Verification_status: passed
  - DoD: snapshots expose a document revision plus node references for actionable nodes, and selector strings are no longer the primary external target identity.
  - Validation: `cargo test --locked --all-features`, `cargo test --locked --all-features --test dom_integration test_snapshot_tool_exposes_document_metadata_and_node_refs -- --ignored --exact`
  - Notes: The snapshot path now exposes document metadata, revision-scoped `NodeRef`s, and actionable-node summaries while narrowing index assignment to agent-meaningful controls.

- [x] P000: Capture repository evidence and write the document-surface hardening spec (owner: mayor) (scope: `specs/agent-document-surface-hardening/`) (depends: -)
  - Covers: R001, R002, R003, R004, R005, R006, R007, R008, R009
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: the repository contains requirements, design, and task artifacts that turn the document-layer analysis into an implementation-ready plan.
  - Validation: repository discovery across `src/dom/`, `src/browser/`, `src/tools/`, and `tests/`
  - Notes: Tab identity and multi-agent tab ownership are intentionally left out of the first execution wave so the next changes can focus on the in-document agent contract.
