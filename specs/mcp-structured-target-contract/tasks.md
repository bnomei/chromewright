# Tasks - mcp-structured-target-contract

Meta:
- Spec: `mcp-structured-target-contract` - live MCP parity for structured DOM targets
- Depends on:
  - `strict-tool-inputs-and-defaults`
- Global scope:
  - `src/mcp/`
  - `src/tools/core/mod.rs`
  - `src/tools/inspect_node.rs`
  - `src/tools/screenshot.rs`
  - `README.md`
  - `docs/`
  - `tests/`
  - `specs/mcp-structured-target-contract/`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] P000: Capture live debugging evidence and author the structured-target parity spec (owner: mayor)
  - Covers: R001, R002, R003, R004, R005, R006
  - Verification_mode: mayor
  - Verification_status: passed
  - Validation: discovery across `src/tools/core/mod.rs`, `src/mcp/mod.rs`, `src/tools/inspect_node.rs`, `src/tools/screenshot.rs`, and the live OpenAI article MCP pass.

- [x] T001: Add live MCP contract parity coverage for object-typed `target` fields (owner: mayor)
  - Covers: R001, R002, R004, R006
  - Verification_mode: required
  - Verification_status: passed
  - Validation: `cargo test --locked mcp::tests::`

- [x] T002: Tighten DOM-tool descriptions and examples around canonical structured targets (owner: mayor)
  - Covers: R003, R005, R006
  - Verification_mode: validator
  - Verification_status: passed
  - Validation: README/docs readback plus `cargo test --locked mcp::tests::`
