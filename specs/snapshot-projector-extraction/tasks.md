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

- (none)

## Done

- [x] T001: Extract snapshot projection and scoped rendering into a pure module (owner: mayor) (scope: `src/tools/core/mod.rs`, `src/tools/core/snapshot_projection.rs`) (depends: spec:contract-domain-extraction, spec:viewport-metrics-backend-api) (started: 2026-04-24T11:26:29Z, completed: 2026-04-24T11:33:41Z)
  - Covers: R001, R002, R003, R004, R005, R007
  - Verification_status: passed
  - Evidence:
    - `cargo test --lib snapshot_projection`
    - `cargo check --locked`
  - Notes: Extracted projection, delta policy, scoped rendering, snapshot node collection, and cache-entry conversion into `src/tools/core/snapshot_projection.rs`. The projector accepts explicit `DomTree`, mode, prior cache entry, and count inputs and has no `ToolContext` or `BrowserSession` dependency.

- [x] T002: Add deterministic projector fixture tests (owner: mayor) (scope: `src/tools/core/snapshot_projection.rs`, `tests/**`) (depends: T001) (started: 2026-04-24T11:26:29Z, completed: 2026-04-24T11:33:41Z)
  - Covers: R001, R004, R005, R006, R007
  - Verification_status: passed
  - Evidence:
    - `cargo test --lib snapshot_projection`
    - `cargo test --lib build_document_envelope_delta_mode`
  - Notes: Added deterministic projector fixtures for viewport, full, delta fallback, delta diff, cache-entry conversion, and scoped persistent-chrome rendering.

- [x] T003: Confirm snapshot envelope behavior and schemas stayed stable (owner: mayor) (scope: `src/tools/core/mod.rs`, `src/mcp/**`, `tests/**`, `specs/snapshot-projector-extraction/tasks.md`) (depends: T001, T002) (started: 2026-04-24T11:26:29Z, completed: 2026-04-24T11:33:41Z)
  - Covers: R001, R003, R004, R005, R007
  - Verification_status: passed
  - Evidence:
    - `cargo test --lib build_document_envelope_viewport_mode_scopes_snapshot_handles`
    - `cargo test --lib build_document_envelope_full_mode_preserves_exhaustive_snapshot_handles`
    - `cargo test --lib build_document_envelope_delta_mode_falls_back_to_viewport_without_base`
    - `cargo test --lib test_snapshot_schema_exposes_mode_and_scope_contract`
    - `cargo check --locked`
  - Notes: `build_document_envelope` remains the orchestration boundary for DOM reads, viewport metrics, cache lookup/update, and metrics recording.
