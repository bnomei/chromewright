# Tasks - typed-browser-error-taxonomy

Meta:
- Spec: `typed-browser-error-taxonomy` - Typed browser recovery errors
- Depends on: none
- Global scope:
  - `src/error.rs`
  - `src/browser/backend.rs`
  - `src/browser/backend/**`
  - `src/tools/core/mod.rs`
  - `src/mcp/**`
  - `tests/**`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add typed browser error detail variants (owner: mayor) (scope: `src/error.rs`, `src/browser/backend.rs`, `tests/**`) (depends: -) (started: 2026-04-24T10:48:43Z, completed: 2026-04-24T10:54:47Z)
  - Covers: R001, R003, R004, R007
  - Context: Keep upstream string classification at the Chrome adapter boundary, but convert recoverable page-target loss and unsupported capability cases into typed variants immediately.
  - Reuse_targets: `BrowserError`, page-target-loss classification helpers, `browser_error_detail`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: Page-target-loss and backend-unsupported conditions have typed details; existing display text remains useful for logs.
  - Validation: Run focused backend error tests and `cargo check --locked`.
  - Evidence: `cargo test --lib page_target` passed; `cargo test --lib backend_unsupported` passed; `cargo check --locked` passed.
  - Escalate if: `thiserror` display requirements force public message changes not covered by compatibility tests.

- [x] T002: Replace attach-degraded prefix decoding with typed payload mapping (owner: mayor) (scope: `src/browser/backend.rs`, `src/tools/core/mod.rs`, `tests/**`) (depends: T001) (started: 2026-04-24T10:48:43Z, completed: 2026-04-24T10:54:47Z)
  - Covers: R001, R002, R005, R006
  - Context: Existing public structured failure shape must remain compatible. The implementation should stop using encoded reason strings as the internal data path.
  - Reuse_targets: `attach_session_page_target_loss`, `attach_session_degraded_failure`, `tool_result_from_browser_error`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: validator
  - Verification_status: passed
  - DoD: Attach degraded failures are generated from typed details and tests assert structured fields rather than encoded reason prefixes.
  - Validation: Run `cargo test --lib attach_session`, `cargo test --lib structured_failure`, and `cargo test --lib mcp`.
  - Evidence: `cargo test --lib attach_session` passed; `cargo test --lib structured_failure` passed; `cargo test --lib mcp` passed.
  - Escalate if: Maintaining wire compatibility requires keeping a deprecated encoded field for one migration wave.

- [x] T003: Audit recovery/error tests for string-coupled assertions (owner: mayor) (scope: `tests/**`, `src/browser/backend.rs`, `src/tools/core/mod.rs`, `specs/typed-browser-error-taxonomy/tasks.md`) (depends: T001, T002) (started: 2026-04-24T10:48:43Z, completed: 2026-04-24T10:54:47Z)
  - Covers: R004, R005, R006, R007
  - Context: The closeout should remove fragile tests that only prove display strings.
  - Reuse_targets: existing attach-mode, MCP conversion, and structured failure tests
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - Verification_mode: required
  - Verification_status: passed
  - DoD: Tests assert typed details or structured payloads for migrated errors; remaining string-only families are documented as follow-up.
  - Validation: Run the full verification plan from `design.md`.
  - Evidence: Existing encoded-reason round-trip assertion was replaced with typed page-target-loss assertions; remaining upstream message matching is isolated in the Chrome backend classification helper.
  - Escalate if: A public client-facing field needs a breaking rename or removal.
