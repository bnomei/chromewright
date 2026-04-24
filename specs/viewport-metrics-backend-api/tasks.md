# Tasks - viewport-metrics-backend-api

Meta:
- Spec: `viewport-metrics-backend-api` - Typed viewport metrics backend capability
- Depends on: `set-viewport`
- Global scope:
  - `src/browser/**`
  - `src/tools/core/mod.rs`
  - `src/tools/set_viewport.rs`
  - `tests/**`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add typed viewport metrics methods to backend and session (owner: mayor) (scope: `src/browser/**`) (depends: -) (started: 2026-04-24T10:43:28Z, completed: 2026-04-24T10:47:33Z)
  - Covers: R001, R002, R003, R005, R006, R007
  - Context: Chrome already measures viewport metrics via `ScreenshotPageMetrics::evaluate`; fake backend already stores deterministic per-tab viewport state.
  - Reuse_targets: `SessionBackend`, `BrowserSession`, `ScreenshotPageMetrics::viewport_metrics`, `ChromeSessionBackend::measure_viewport_metrics`, `FakeSessionBackend::current_viewport_metrics`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: mayor
  - DoD: `SessionBackend` and `BrowserSession` expose a typed viewport metrics read for active or specific tabs; Chrome and fake backend implementations preserve current measured values.
  - Validation: Run `cargo test --lib viewport`.
  - Evidence: `cargo test --lib viewport` passed; `cargo check --locked` passed.
  - Escalate if: Supporting `tab_id` requires changing active-tab semantics, adding cache invalidation to a read-only method, or moving unrelated screenshot/CDP helpers beyond the page-metrics cluster.

- [x] T002: Route snapshot scope through the typed metrics method (owner: mayor) (scope: `src/tools/core/mod.rs`) (depends: T001) (started: 2026-04-24T10:43:28Z, completed: 2026-04-24T10:47:33Z)
  - Covers: R001, R004, R007
  - Context: `live_viewport_metrics` currently embeds a separate JavaScript probe and returns `None` for any error. The replacement should make supported-backend failures visible.
  - Reuse_targets: `build_document_envelope`, `SnapshotScope`, `ToolContext::record_browser_evaluation`
  - Autonomy: standard
  - Risk: medium
  - Complexity: low
  - Verification_mode: mayor
  - DoD: Snapshot scope metrics are populated via `BrowserSession::viewport_metrics`; duplicate viewport JS in `src/tools/core/mod.rs` is removed.
  - Validation: Run `cargo test --lib build_document_envelope_viewport_mode_scopes_snapshot_handles`.
  - Evidence: `cargo test --lib build_document_envelope_viewport_mode_scopes_snapshot_handles` passed; `cargo test --lib test_snapshot_schema_exposes_mode_and_scope_contract` passed.
  - Escalate if: Preserving optional `scope.viewport` requires a new explicit unsupported-backend error type.

- [x] T003: Remove viewport-metric dependence on fake script string matching (owner: mayor) (scope: `src/browser/backend/fake.rs`, `tests/**`) (depends: T001, T002) (started: 2026-04-24T10:43:28Z, completed: 2026-04-24T10:47:33Z)
  - Covers: R003, R005, R007
  - Context: Tests should verify typed fake backend metrics directly. Do not rewrite unrelated script-dispatch branches in the fake backend.
  - Reuse_targets: `scripted_viewport_metrics`, `scripted_result_with_url`, existing viewport tests in `src/browser/session.rs` and `src/tools/set_viewport.rs`
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - Verification_mode: validator
  - DoD: Viewport metrics tests no longer rely on fake JavaScript substring recognition; unrelated fake evaluation behavior remains intact.
  - Validation: Run `cargo test --lib viewport` and `cargo test --lib set_viewport`.
  - Evidence: `cargo test --lib viewport` passed; `cargo test --lib set_viewport` passed; added fake-backend regression for typed-only viewport metrics.
  - Escalate if: Existing tests still require JavaScript evaluation of viewport globals as part of the public `evaluate` operator contract.
