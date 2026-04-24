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

- [ ] T001: Add the internal browser command model and backend method (owner: unassigned) (scope: `src/browser/**`, `tests/**`) (depends: spec:typed-browser-error-taxonomy)
  - Covers: R001, R002, R003, R004, R006
  - Context: Keep `evaluate` intact. The new command path should be additive and default unsupported until implemented by Chrome/fake backends.
  - Reuse_targets: `SessionBackend`, `ScriptEvaluation`, `decode_browser_json_value`, `BrowserError::BackendUnsupported`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: mayor
  - Verification_status: pending
  - DoD: `BrowserCommand` and result types exist; `SessionBackend` exposes an internal command execution method; unsupported behavior is typed.
  - Validation: Run backend unit tests and `cargo check --locked`.
  - Escalate if: Adding the method forces public API exposure or requires removing raw script evaluation.

- [ ] T002: Migrate actionability and selector identity probes to commands (owner: unassigned) (scope: `src/tools/actionability.rs`, `src/tools/services/interaction.rs`, `src/browser/**`, `tests/**`) (depends: T001)
  - Covers: R001, R002, R003, R005, R006, R007
  - Context: Preserve probe diagnostics and polling behavior. Chrome may render the same JS; fake must branch on typed command data for these probes.
  - Reuse_targets: `ActionabilityProbe`, `wait_for_actionability`, selector identity probe helper, `browser_kernel` rendering helpers
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: validator
  - Verification_status: pending
  - DoD: Actionability and selector identity behavior no longer depend on fake script substring matching.
  - Validation: Run `cargo test --lib actionability`, `cargo test --lib interaction`, and focused fake-backend tests.
  - Escalate if: Probe result compatibility requires changing public target-status or actionability diagnostics.

- [ ] T003: Migrate click/input/hover/select interaction actions to commands (owner: unassigned) (scope: `src/tools/click.rs`, `src/tools/input.rs`, `src/tools/hover.rs`, `src/tools/select.rs`, `src/tools/browser_kernel.rs`, `src/browser/**`, `tests/**`) (depends: T002)
  - Covers: R001, R002, R003, R005, R006, R007
  - Context: Preserve current tool output and handoff behavior. Do not rewrite the browser-side interaction implementation unless needed to pass through typed config/result structs.
  - Reuse_targets: existing interaction JS configs, `build_interaction_handoff`, `browser_kernel` template renderers, fake backend interaction state
  - Autonomy: standard
  - Risk: high
  - Complexity: high
  - Verification_mode: required
  - Verification_status: pending
  - DoD: Click/input/hover/select production behavior is unchanged and fake behavior is command-driven for migrated actions.
  - Validation: Run focused tool tests plus `cargo check --locked`; run ignored browser integration interaction tests when Chrome is available.
  - Escalate if: The migration starts changing user-visible actionability, focus, or target-rebind semantics.

## Done

- (none)

