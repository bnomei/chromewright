# Tasks - set-viewport-schema-constraints

Meta:
- Spec: `set-viewport-schema-constraints` - Schema-expressed `set_viewport` constraints
- Depends on: `set-viewport`
- Global scope:
  - `src/browser/backend.rs`
  - `src/tools/set_viewport.rs`
  - `src/tools/mod.rs`
  - `src/mcp/mod.rs`
  - `README.md`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add schema-visible numeric constraints for set_viewport params (owner: mayor) (scope: `src/browser/backend.rs`, `src/tools/set_viewport.rs`) (depends: -)
  - Covers: R001, R002, R003, R005, R007
  - Completed_at: 2026-04-24T10:37:56Z
  - Completion note: `SetViewportParams` now advertises width/height minimum `1`, width/height maximum `VIEWPORT_DIMENSION_MAX`, and `device_scale_factor` `exclusiveMinimum = 0` through its JSON schema.
  - Validation result: `cargo test --lib test_set_viewport_schema_exposes_breakpoint_contract`, `cargo test --lib test_set_viewport_schema_is_exported_via_mcp`, `cargo test --lib set_viewport`, and `cargo check --locked` passed.
  - Implemented_by: mayor
  - Verified_by: mayor
  - Verification_status: passed

- [x] T002: Represent or document reset-only schema semantics (owner: mayor) (scope: `src/tools/set_viewport.rs`, `src/tools/mod.rs`, `src/mcp/mod.rs`) (depends: T001)
  - Covers: R004, R005, R006, R007
  - Completed_at: 2026-04-24T10:37:56Z
  - Completion note: The reset field schema description and tool description now state the reset-only rule, and registry/MCP schema tests assert that wording remains advertised.
  - Validation result: `cargo test --lib test_set_viewport_schema_exposes_breakpoint_contract` and `cargo test --lib test_set_viewport_schema_is_exported_via_mcp` passed.
  - Implemented_by: mayor
  - Verified_by: mayor
  - Verification_status: passed

- [x] T003: Refresh docs for schema-visible constraints (owner: mayor) (scope: `README.md`, `specs/set-viewport-schema-constraints/tasks.md`) (depends: T001, T002)
  - Covers: R006, R007
  - Completed_at: 2026-04-24T10:37:56Z
  - Completion note: README now documents bounded positive viewport dimensions, positive DPR, and reset-only `tab_id` semantics without duplicating the full schema.
  - Validation result: `cargo check --locked` and the schema tests from T002 passed.
  - Implemented_by: mayor
  - Verified_by: mayor
  - Verification_status: passed
