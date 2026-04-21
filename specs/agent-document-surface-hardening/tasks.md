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

- [ ] T001: Introduce revision-scoped document identity and `NodeRef` targeting (owner: unassigned) (scope: `src/dom/`, `src/tools/snapshot.rs`, `src/tools/mod.rs`, `tests/dom_integration.rs`)
  - Covers: R001, R003
  - Verification_mode: required
  - DoD: snapshots expose a document revision plus node references for actionable nodes, and selector strings are no longer the primary external target identity.
  - Validation: targeted DOM unit tests plus browser-backed snapshot tests that assert `NodeRef` presence and revision reporting.
  - Escalate-if: implementation requires promising cross-mutation persistent node IDs instead of revision-scoped IDs.

- [ ] T002: Reject stale node references and centralize DOM cache invalidation (owner: unassigned) (scope: `src/tools/mod.rs`, `src/tools/click.rs`, `src/tools/input.rs`, `src/tools/select.rs`, `src/tools/hover.rs`, `src/lib.rs`, `tests/browser_tools_integration.rs`)
  - Covers: R002, R007
  - Verification_mode: required
  - DoD: high-level tools fail with structured stale-reference errors when given outdated node refs, and `ToolContext` no longer returns stale DOM after mutations.
  - Validation: unit tests for cache invalidation/versioning plus browser-backed tests that use an old ref after a mutation and observe a structured stale-ref failure.
  - Escalate-if: stale detection cannot be implemented without changing all tool signatures at once.

- [ ] T003: Normalize post-action document envelopes for high-level tools (owner: unassigned) (scope: `src/tools/click.rs`, `src/tools/input.rs`, `src/tools/select.rs`, `src/tools/navigate.rs`, `src/tools/go_back.rs`, `src/tools/go_forward.rs`, `src/mcp/`, `tests/browser_tools_integration.rs`, `tests/navigation_integration.rs`)
  - Covers: R004
  - Verification_mode: required
  - DoD: high-level document tools return a shared result shape that includes document metadata and resulting revision; tool-specific details are additive, not bespoke.
  - Validation: adapter and integration tests that assert a uniform envelope across at least `navigate`, `click`, `input`, and `select`.
  - Escalate-if: MCP structured output constraints force a separate transport-only shape.

- [ ] T004: Replace timing heuristics with explicit document wait predicates (owner: unassigned) (scope: `src/tools/wait.rs`, `src/browser/session.rs`, `src/tools/markdown.rs`, `tests/navigation_integration.rs`, `tests/markdown_integration.rs`)
  - Covers: R005
  - Verification_mode: required
  - DoD: the wait path can express navigation settle and node-state predicates, and the fixed sleeps in scoped paths are removed or reduced to bounded fallbacks with explicit rationale.
  - Validation: integration tests covering wait-for-navigation-settle and at least one node-state predicate; regression tests for markdown extraction on dynamic pages.
  - Escalate-if: `headless_chrome` lacks a reliable signal for a required predicate and a repo-owned fallback becomes too speculative.

- [ ] T005: Wire iframe-aware document assembly into the snapshot path (owner: unassigned) (scope: `src/dom/`, `src/browser/session.rs`, `src/tools/snapshot.rs`, `tests/dom_integration.rs`)
  - Covers: R006
  - Verification_mode: required
  - DoD: same-origin iframe content is either assembled into the document view with explicit frame boundaries or reported as unavailable in a structured way.
  - Validation: browser-backed DOM tests with a same-origin iframe fixture and assertions about frame markers or unavailability reporting.
  - Escalate-if: cross-origin iframe limits are confused with same-origin implementation gaps.

- [ ] T006: Separate advanced/operator surfaces from the default high-level agent contract (owner: unassigned) (scope: `src/tools/evaluate.rs`, `src/tools/screenshot.rs`, `src/tools/mod.rs`, `src/lib.rs`, `README.md`, `tests/`)
  - Covers: R008
  - Verification_mode: mayor
  - DoD: docs and tool registration make the default document interaction surface distinct from raw JS evaluation and filesystem-bound screenshot capture.
  - Validation: documentation updates plus focused registration/contract tests.
  - Escalate-if: tool discovery constraints require keeping a single undifferentiated tool registry.

- [ ] T007: Refresh browser-backed contract tests around the new document model (owner: unassigned) (scope: `tests/`, `src/tools/`, `src/dom/`)
  - Covers: R009
  - Verification_mode: required
  - DoD: integration tests lock in the intended document contract and remove the current tolerance for drifting or optional index behavior.
  - Validation: `cargo test --locked --all-features` with the updated contract tests enabled where environment support exists.
  - Escalate-if: the environment-dependent browser tests still cannot provide deterministic signal after the launch baseline fixes.

## Done

- [x] P000: Capture repository evidence and write the document-surface hardening spec (owner: mayor) (scope: `specs/agent-document-surface-hardening/`) (depends: -)
  - Covers: R001, R002, R003, R004, R005, R006, R007, R008, R009
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: the repository contains requirements, design, and task artifacts that turn the document-layer analysis into an implementation-ready plan.
  - Validation: repository discovery across `src/dom/`, `src/browser/`, `src/tools/`, and `tests/`
  - Notes: Tab identity and multi-agent tab ownership are intentionally left out of the first execution wave so the next changes can focus on the in-document agent contract.
