# Tasks - typed-browser-command-layer

Meta:
- Spec: `typed-browser-command-layer` - Typed commands for browser-side operations
- Depends on: `typed-browser-error-taxonomy`
- Global scope:
  - `src/browser/**`
  - `src/tools/actionability.rs`
  - `src/tools/browser_kernel.rs`
  - `src/tools/services/**`
  - `src/tools/click.rs`
  - `src/tools/input.rs`
  - `src/tools/hover.rs`
  - `src/tools/select.rs`
  - `tests/**`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add the internal browser command model and backend method (owner: mayor) (scope: `src/browser/**`, `tests/**`) (depends: spec:typed-browser-error-taxonomy) (started: 2026-04-24T11:09:40Z, completed: 2026-04-24T11:24:56Z)
  - Covers: R001, R002, R003, R004, R006
  - Verification_status: passed
  - Evidence:
    - `cargo test --lib commands`
    - `cargo test --lib fake_backend`
    - `cargo check --locked`
  - Notes: Added internal `BrowserCommand`/`BrowserCommandResult`, `SessionBackend::execute_command`, Chrome adaptation, fake command handling, and typed unsupported fallback while keeping raw `evaluate` intact.

- [x] T002: Migrate actionability and selector identity probes to commands (owner: mayor) (scope: `src/tools/actionability.rs`, `src/tools/services/interaction.rs`, `src/browser/**`, `tests/**`) (depends: T001) (started: 2026-04-24T11:09:40Z, completed: 2026-04-24T11:24:56Z)
  - Covers: R001, R002, R003, R005, R006, R007
  - Verification_status: passed
  - Evidence:
    - `cargo test --lib actionability`
    - `cargo test --lib interaction`
    - `cargo test --lib fake_backend`
  - Notes: Actionability and selector identity now call command DTOs; fake behavior branches on command data, and migrated rendered actionability scripts are no longer recognized by fake script matching.

- [x] T003: Migrate click/input/hover/select interaction actions to commands (owner: mayor) (scope: `src/tools/click.rs`, `src/tools/input.rs`, `src/tools/hover.rs`, `src/tools/select.rs`, `src/tools/browser_kernel.rs`, `src/browser/**`, `tests/**`) (depends: T002) (started: 2026-04-24T11:09:40Z, completed: 2026-04-24T11:24:56Z)
  - Covers: R001, R002, R003, R005, R006, R007
  - Verification_status: passed
  - Evidence:
    - `cargo test --lib click`
    - `cargo test --lib input`
    - `cargo test --lib hover`
    - `cargo test --lib select`
    - `cargo test --lib commands`
    - `cargo check --locked`
  - Notes: Click/input/hover/select preserve existing tool outputs and handoff handling while using typed interaction commands for browser execution.
