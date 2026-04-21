# Tasks — performance-and-surface-hardening

Meta:
- Spec: performance-and-surface-hardening — reduce avoidable DOM/runtime work and harden the unsafe tool surface
- Depends on: `specs/agent-document-surface-hardening/`
- Global scope:
  - `src/browser/`
  - `src/dom/`
  - `src/mcp/`
  - `src/tools/`
  - `src/bin/`
  - `src/lib.rs`
  - `README.md`
  - `tests/`
  - `docs/perf.md`
  - `specs/performance-and-surface-hardening/`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T007: Harden MCP/runtime execution so transport is not tied to `current_thread` progress (owner: mayor) (scope: `src/mcp/`, `src/bin/mcp_server.rs`, `src/browser/session.rs`, `tests/`) (depends: T001, T002)
  - Covers: R001, R010
  - Verification_mode: required
  - Verification_status: passed
  - DoD: the server no longer depends on a current-thread runtime for all transport progress, and the chosen session-sharing model is documented in code/comments if lock narrowing remains constrained by dependencies.
  - Validation: `cargo check --all-features --all-targets --locked`, `cargo test --locked --all-features`
  - Notes: Switched the MCP server to Tokio `multi_thread` and removed the outer `Arc<Mutex<BrowserSession>>` wrapper in favor of a shared immutable session with interior caches.

- [x] T006: Require explicit unsafe acknowledgement for operator tools and constrain screenshot paths (owner: mayor) (scope: `src/tools/evaluate.rs`, `src/tools/screenshot.rs`, `src/tools/mod.rs`, `README.md`, `src/lib.rs`, `tests/`) (depends: -)
  - Covers: R009, R010
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: operator tools reject calls without explicit unsafe acknowledgement; screenshot output paths are validated as safe relative paths; docs match the new contract.
  - Validation: `cargo check --all-features --all-targets --locked`, `cargo test --locked --all-features`
  - Notes: Added `confirm_unsafe` gates to operator params, introduced safe relative screenshot-path validation, and updated the README/crate docs to advertise the new contract.

- [x] T005: Enforce default-safe navigation with explicit unsafe opt-in (owner: mayor) (scope: `src/tools/utils.rs`, `src/tools/navigate.rs`, `src/tools/new_tab.rs`, `src/mcp/`, `tests/navigation_integration.rs`, `tests/tab_management_integration.rs`, `README.md`) (depends: -)
  - Covers: R008, R010
  - Verification_mode: required
  - Verification_status: passed
  - DoD: unsafe schemes are blocked by default on high-level navigation tools; explicit opt-in exists for trusted/test usage; docs and tests are updated.
  - Validation: `cargo check --all-features --all-targets --locked`, `cargo test --locked --all-features`
  - Notes: Added `allow_unsafe` to high-level navigation params, kept low-level `BrowserSession::navigate` unchanged, and updated the `data:`-backed tab-management tests to opt in explicitly.

- [x] T004: Replace input clear key loops with direct bounded clearing (owner: mayor) (scope: `src/tools/input.rs`, `tests/browser_tools_integration.rs`) (depends: T001)
  - Covers: R007
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: input clearing no longer depends on repeated keypress loops and browser-backed tests still pass.
  - Validation: `cargo check --all-features --all-targets --locked`, `cargo test --locked --all-features`
  - Notes: Input clearing now uses a direct DOM mutation helper plus `input`/`change` dispatch instead of sending a backspace loop.

- [x] T003: Cache markdown extraction per document revision (owner: mayor) (scope: `src/browser/session.rs`, `src/tools/markdown.rs`, `tests/markdown_integration.rs`) (depends: T001)
  - Covers: R006
  - Verification_mode: required
  - Verification_status: passed
  - DoD: repeated markdown reads for the same revision hit a cache and only re-extract on revision change.
  - Validation: `cargo check --all-features --all-targets --locked`, `cargo test --locked --all-features`
  - Notes: Added a revision-scoped markdown cache on `BrowserSession` and reused it for pagination and repeat reads of the same document revision.

- [x] T002: Cache and validate the active-tab hint across tab operations (owner: mayor) (scope: `src/browser/session.rs`, `src/tools/new_tab.rs`, `src/tools/switch_tab.rs`, `src/tools/close_tab.rs`, `src/tools/tab_list.rs`, `tests/tab_management_integration.rs`) (depends: -)
  - Covers: R005
  - Verification_mode: required
  - Verification_status: passed
  - DoD: active-tab lookup is cache-backed, tab activation paths keep the cache coherent, and tab-management tests cover the behavior.
  - Validation: `cargo check --all-features --all-targets --locked`, `cargo test --locked --all-features`
  - Notes: `BrowserSession` now keeps a validated active-tab hint, uses it when present, and refreshes it on launch/new-tab/activation paths.

- [x] T001: Add lightweight document metadata/revision reads and use them in minimal envelopes (owner: mayor) (scope: `src/browser/`, `src/dom/`, `src/tools/mod.rs`, `src/tools/wait.rs`, `src/tools/click.rs`, `src/tools/input.rs`, `src/tools/select.rs`, `src/tools/hover.rs`, `src/tools/navigate.rs`, `src/tools/go_back.rs`, `src/tools/go_forward.rs`, `src/tools/snapshot.rs`, `tests/`) (depends: -)
  - Covers: R002, R003, R004
  - Verification_mode: required
  - Verification_status: passed
  - DoD: a lightweight metadata/revision path exists; high-level actions no longer eagerly rebuild full snapshots; `wait(revision_changed)` polls metadata instead of full DOM extraction.
  - Validation: `cargo check --all-features --all-targets --locked`, `cargo test --locked --all-features`
  - Notes: Added a lightweight document-metadata script, aligned revision-token generation between metadata and full snapshots, switched action tools to metadata-first envelopes, and kept the `snapshot` tool as the explicit full-document surface.

- [x] P000: Capture repository evidence and write the performance/surface hardening spec (owner: mayor) (scope: `specs/performance-and-surface-hardening/`, `docs/perf.md`) (depends: -)
  - Covers: R001, R002, R003, R004, R005, R006, R007, R008, R009, R010
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: the repository contains aligned requirements, design, and task artifacts grounded in Frigg-backed discovery and the existing perf review.
  - Validation: Frigg discovery across `src/browser/`, `src/dom/`, `src/mcp/`, `src/tools/`, `tests/`, and existing specs
  - Notes: This wave intentionally treats benchmark proof as follow-on validation rather than a reason to block bounded code hardening.
