# Tasks - snapshot-projector-extraction

Meta:
- Spec: `snapshot-projector-extraction` - Pure snapshot projection module
- Depends on: `contract-domain-extraction`, `viewport-metrics-backend-api`
- Global scope:
  - `src/tools/core/mod.rs`
  - `src/tools/core/snapshot_projection.rs`
  - `src/browser/session/cache.rs`
  - `src/dom/**`
  - `tests/**`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- [ ] T001: Extract snapshot projection and scoped rendering into a pure module (owner: unassigned) (scope: `src/tools/core/mod.rs`, `src/tools/core/snapshot_projection.rs`) (depends: spec:contract-domain-extraction, spec:viewport-metrics-backend-api)
  - Covers: R001, R002, R003, R004, R005, R007
  - Context: Move only the cohesive projection/scoped-rendering cluster. Do not move target resolution, registry, result normalization, live browser reads, or cache mutation in this task.
  - Reuse_targets: `SnapshotProjection`, `snapshot_projection`, `delta_snapshot_projection`, `snapshot_cache_entry_from_projection`, `render_scoped_snapshot_root`, `delta_snapshot_text`, `delta_snapshot_nodes`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: validator
  - Verification_status: pending
  - DoD: Projector code is callable with explicit inputs and has no dependency on `ToolContext` or `BrowserSession`; behavior is unchanged.
  - Validation: Run focused snapshot mode tests and `cargo check --locked`.
  - Escalate if: Extraction requires widening public APIs beyond `pub(crate)` or changing snapshot cache invalidation behavior.

- [ ] T002: Add deterministic projector fixture tests (owner: unassigned) (scope: `src/tools/core/snapshot_projection.rs`, `tests/**`) (depends: T001)
  - Covers: R001, R004, R005, R006, R007
  - Context: These tests should prove projection policy without live Chrome. Reuse existing fake DOM builders where possible.
  - Reuse_targets: existing snapshot projection unit tests in `src/tools/core/mod.rs`, `SnapshotNode`, `DomTree`, `SnapshotCacheEntry`
  - Autonomy: standard
  - Risk: low
  - Complexity: medium
  - Verification_mode: validator
  - Verification_status: pending
  - DoD: Viewport, full, delta fallback, delta diff, scoped rendering, and cache-entry conversion have deterministic projector-level coverage.
  - Validation: Run projector tests plus the focused snapshot tests from `design.md`.
  - Escalate if: Current test helpers cannot build representative `DomTree` fixtures without broad DOM extraction changes.

- [ ] T003: Confirm snapshot envelope behavior and schemas stayed stable (owner: unassigned) (scope: `src/tools/core/mod.rs`, `src/mcp/**`, `tests/**`, `specs/snapshot-projector-extraction/tasks.md`) (depends: T001, T002)
  - Covers: R001, R003, R004, R005, R007
  - Context: This is a refactor closeout. `build_document_envelope` should remain the only place coordinating browser reads, cache lookup/update, and operation metrics.
  - Reuse_targets: MCP schema tests, snapshot schema tests, existing document envelope tests
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - Verification_mode: required
  - Verification_status: pending
  - DoD: Validation evidence shows schemas and serialized snapshot behavior are unchanged.
  - Validation: Run the full verification plan from `design.md`.
  - Escalate if: Any public snapshot payload shape changes appear in the implementation diff.

## Done

- (none)

