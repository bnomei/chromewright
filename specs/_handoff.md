# Program handoff

Last updated: 2026-04-24T10:54:47Z

## Current focus
- Implement all open active/planned spec tasks end to end, committing after each successful spec.
- Execution mode: adaptive (cap: 2, bundle depth: 1; currently using strict-by-spec commits for overlapping code scopes)

## Reservations (in progress scopes)
- (none)

## In progress tasks
- (none)

## Pending verification
- (none)

## Blockers
- (none)

## Next ready tasks
- `typed-browser-error-taxonomy/T001` after current browser scope is free.
- `mcp-blocking-execution-boundary/T001` after current MCP-adjacent exploration returns.
- `contract-domain-extraction/T001` after viewport metrics lands.

## Completed this cycle
- `headless-chrome-cdp-contract-pin` completed and ready for commit.
- `set-viewport-schema-constraints` completed and ready for commit.
- `viewport-output-contract-alignment` completed and ready for commit.
- `headless-chrome-cdp-contract-pin` committed as `9264c41`/`c690e1b` across plan + implementation.
- `set-viewport-schema-constraints` committed as `68e2d57`.
- `viewport-output-contract-alignment` committed as `99132c6`.
- `viewport-metrics-backend-api` completed and ready for commit.
  - Evidence: `cargo test --lib viewport`; `cargo test --lib set_viewport`; `cargo test --lib build_document_envelope_viewport_mode_scopes_snapshot_handles`; `cargo test --lib test_snapshot_schema_exposes_mode_and_scope_contract`; `cargo check --locked`.
- `viewport-metrics-backend-api` committed as `20994e4`.
- `typed-browser-error-taxonomy` completed and ready for commit.
  - Evidence: `cargo test --lib page_target`; `cargo test --lib structured_failure`; `cargo test --lib backend_unsupported`; `cargo test --lib attach_session`; `cargo test --lib mcp`; `cargo check --locked`.

## Notes
- `snapshot` redesign remains out of scope; only `snapshot.scope.viewport` is in wave.
- Prefer CDP emulation over OS window resizing in attach mode.
- `specs/*` is gitignored in this repo; orchestration state is durable locally but not tracked unless ignore rules change.
- Recovered lease history for `set-viewport/T001`: `worker:019dbeae-afb9-7e01-bf6e-c115e3b5649b` left a partial `src/browser/backend.rs` edit without validation; mayor preserved the sound pieces, added the missing CDP max-bound validation, and completed the browser/session layer.
- `worker:019dbeb7-c9b1-73e3-9db4-817ac7394930` was closed after an audit-first recovery lease stalled without reaching validation.
- Final implementation surfaces:
  - browser/session viewport apply/reset primitives under `src/browser/`
  - public `set_viewport` tool and MCP/schema exposure under `src/tools/` and `src/mcp/`
  - narrow `snapshot.scope.viewport` runtime/schema addition without widening unrelated outputs
  - README default-surface docs updated from 21 to 22 tools
- Validation completed locally:
  - `cargo test --lib viewport`
  - `cargo test --lib set_viewport`
  - `cargo test --lib build_document_envelope_viewport_mode_scopes_snapshot_handles`
  - `cargo test --lib test_snapshot_schema_exposes_mode_and_scope_contract`
  - `cargo test --test browser_tools_integration test_set_viewport_tool_emulates_breakpoint_and_snapshot_scope_reports_viewport -- --ignored`
  - `cargo test --test dom_integration test_snapshot_tool_exposes_document_metadata_and_node_refs -- --ignored`
