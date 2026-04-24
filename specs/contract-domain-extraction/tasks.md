# Tasks - contract-domain-extraction

Meta:
- Spec: `contract-domain-extraction` - Pure contract DTO module
- Depends on: none
- Global scope:
  - `src/contract/**`
  - `src/lib.rs`
  - `src/tools/core/**`
  - `src/tools/core/mod.rs`
  - `src/tools/mod.rs`
  - `src/browser/**`
  - `src/dom/**`
  - `src/mcp/**`
  - `tests/**`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Scaffold the pure contract module and migrate document/target DTOs (owner: mayor) (scope: `src/contract/**`, `src/tools/core/mod.rs`, `src/tools/mod.rs`, `src/lib.rs`, `tests/**`) (depends: -) (started: 2026-04-24T11:00:26Z, completed: 2026-04-24T11:08:31Z)
  - Covers: R001, R002, R003, R004, R005, R006, R008
  - Context: Move passive schema-facing DTOs only. Do not move target resolution, registry composition, `ToolContext`, or browser-session behavior.
  - Reuse_targets: `DocumentEnvelope`, `DocumentResult`, `DocumentActionResult`, `TargetedActionResult`, `SnapshotScope`, `SnapshotMode`, `TargetStatus`, `PublicTarget`, `TaggedPublicTarget`, `PublicTargetCompat`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: validator
  - Verification_status: passed
  - DoD: DTO definitions live under `src/contract`; old imports still work through compatibility re-exports; public schemas are unchanged.
  - Validation: Run `cargo check --locked --no-default-features`, `cargo check --locked --no-default-features --features mcp-handler`, and focused tool schema tests.
  - Evidence: `cargo check --locked --no-default-features` passed; `cargo check --locked --no-default-features --features mcp-handler` passed; `cargo test --lib public_target` passed; `cargo test --lib test_snapshot_schema_exposes_mode_and_scope_contract` passed.
  - Escalate if: Moving any target DTO requires moving target-resolution behavior or widening public APIs beyond compatibility re-exports.

- [x] T002: Migrate viewport DTOs after viewport contract specs land (owner: mayor) (scope: `src/contract/**`, `src/browser/backend.rs`, `src/browser/mod.rs`, `src/tools/set_viewport.rs`, `src/tools/scroll.rs`, `tests/**`) (depends: spec:set-viewport-schema-constraints, spec:viewport-output-contract-alignment, spec:viewport-metrics-backend-api) (started: 2026-04-24T11:00:26Z, completed: 2026-04-24T11:08:31Z)
  - Covers: R001, R002, R003, R004, R005, R007, R008
  - Context: Keep runtime validation and Chrome/CDP implementation out of `contract`. Move only viewport request/result/value DTOs once schema-visible constraints and canonical output names are settled.
  - Reuse_targets: `ViewportMetrics`, `ViewportOrientation`, `ViewportEmulation`, `ViewportEmulationRequest`, `ViewportResetRequest`, `ViewportOperationResult`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: validator
  - Verification_status: passed
  - DoD: Viewport DTOs have a pure contract home; browser backend keeps validation/IO helpers; compatibility re-exports preserve existing imports.
  - Validation: Run `cargo test --lib viewport`, `cargo test --lib set_viewport`, and `cargo check --locked`.
  - Evidence: `cargo test --lib viewport` passed; `cargo test --lib set_viewport` passed; `cargo check --locked` passed.
  - Escalate if: The move creates a dependency from `contract` back to `browser`, `tools`, or `mcp`.

- [x] T003: Confirm contract extraction did not drift schemas or supported exports (owner: mayor) (scope: `src/lib.rs`, `src/mcp/**`, `src/tools/mod.rs`, `tests/**`, `specs/contract-domain-extraction/tasks.md`) (depends: T001, T002) (started: 2026-04-24T11:00:26Z, completed: 2026-04-24T11:08:31Z)
  - Covers: R001, R003, R005, R008
  - Context: This is a refactor closeout. It should not remove compatibility aliases.
  - Reuse_targets: existing MCP schema tests, tool registry tests, no-default-feature compile checks
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - Verification_mode: required
  - Verification_status: passed
  - DoD: The ledger records exact validation commands and confirms no unapproved public schema drift.
  - Validation: Run the full verification plan from `design.md`.
  - Evidence: `cargo test --lib test_set_viewport_schema_is_exported_via_mcp`, `cargo test --lib test_snapshot_schema_exposes_mode_and_scope_contract`, `cargo test --lib tool_registry`, and `cargo test --lib public_target` passed; `tool_registry` currently has zero matching test names but the command completed successfully.
  - Escalate if: Any public schema or crate-root export changes beyond compatibility moves appear in the diff.
