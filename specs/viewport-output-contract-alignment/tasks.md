# Tasks - viewport-output-contract-alignment

Meta:
- Spec: `viewport-output-contract-alignment` - Canonical viewport-related output names
- Depends on: `set-viewport`, `tool-output-contract-normalization`
- Global scope:
  - `src/tools/set_viewport.rs`
  - `src/tools/scroll.rs`
  - `src/tools/mod.rs`
  - `src/mcp/mod.rs`
  - `tests/**`
  - `README.md`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add canonical viewport metrics output for set_viewport (owner: mayor) (scope: `src/tools/set_viewport.rs`, `src/tools/mod.rs`, `src/mcp/mod.rs`) (depends: -)
  - Covers: R001, R003, R005, R006
  - Completed_at: 2026-04-24T10:40:57Z
  - Completion note: `set_viewport` now returns canonical `viewport_metrics_after` while preserving `viewport_after` as an equal compatibility alias.
  - Validation result: `cargo test --lib set_viewport` and `cargo test --lib test_set_viewport_schema_is_exported_via_mcp` passed.
  - Implemented_by: mayor
  - Verified_by: mayor
  - Verification_status: passed

- [x] T002: Add canonical scroll state output for scroll (owner: mayor) (scope: `src/tools/scroll.rs`, `tests/**`) (depends: -)
  - Covers: R002, R003, R005, R006
  - Completed_at: 2026-04-24T10:40:57Z
  - Completion note: `scroll` now returns canonical `scroll_after` while preserving `viewport_after` as an equal compatibility alias.
  - Validation result: `cargo test --lib scroll` passed.
  - Implemented_by: mayor
  - Verified_by: mayor
  - Verification_status: passed

- [x] T003: Update docs and schema assertions for canonical names (owner: mayor) (scope: `README.md`, `src/mcp/mod.rs`, `src/tools/mod.rs`) (depends: T001, T002)
  - Covers: R003, R004, R005
  - Completed_at: 2026-04-24T10:40:57Z
  - Completion note: README and schema/description assertions now steer new clients to `viewport_metrics_after` and `scroll_after`, while documenting compatibility aliases.
  - Validation result: `cargo check --locked` plus the focused schema tests passed.
  - Implemented_by: mayor
  - Verified_by: mayor
  - Verification_status: passed
