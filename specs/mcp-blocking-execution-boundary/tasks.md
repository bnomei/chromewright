# Tasks - mcp-blocking-execution-boundary

Meta:
- Spec: `mcp-blocking-execution-boundary` - Explicit MCP blocking execution boundary
- Depends on: none
- Global scope:
  - `Cargo.toml`
  - `src/mcp/**`
  - `src/bin/mcp_server.rs`
  - `tests/**`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- [ ] T001: Factor synchronous MCP tool execution into a reusable helper (owner: unassigned) (scope: `src/mcp/handler.rs`, `src/mcp/mod.rs`, `tests/**`) (depends: -)
  - Covers: R002, R003, R004, R005
  - Context: Preserve current conversion behavior. This task only extracts the existing synchronous body into a named helper that can be called inline or from a blocking worker.
  - Reuse_targets: `BrowserServer::session`, `ToolContext::new`, `ToolRegistry::execute`, `convert_result`, `mcp_internal_error`
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - Verification_mode: mayor
  - Verification_status: pending
  - DoD: `call_tool` behavior is unchanged through the helper; `list_tools` remains inline metadata mapping.
  - Validation: Run `cargo test --lib mcp`.
  - Escalate if: Extracting the helper requires changing MCP result conversion or tool registry APIs.

- [ ] T002: Use a Tokio blocking boundary when the server feature is enabled (owner: unassigned) (scope: `src/mcp/handler.rs`, `Cargo.toml`, `tests/**`) (depends: T001)
  - Covers: R001, R003, R004, R006, R007
  - Context: `mcp-server` already enables Tokio. Preserve no-Tokio `mcp-handler` compile behavior unless a narrow feature dependency change is unavoidable and documented.
  - Reuse_targets: `tokio::task::spawn_blocking`, `BrowserServer: Clone`, synchronous execution helper from T001
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: validator
  - Verification_status: pending
  - DoD: Under Tokio-enabled builds, browser/tool execution runs through an explicit blocking boundary and join failures map to MCP internal errors.
  - Validation: Run `cargo check --locked --features mcp-server`, `cargo test --lib mcp`, and focused join-failure coverage.
  - Escalate if: `rmcp::ServerHandler` lifetime constraints prevent moving request/session state into a `'static` blocking task without broad API changes.

- [ ] T003: Prove feature-gated adapter behavior stayed stable (owner: unassigned) (scope: `src/mcp/**`, `src/bin/mcp_server.rs`, `tests/**`, `specs/mcp-blocking-execution-boundary/tasks.md`) (depends: T001, T002)
  - Covers: R001, R002, R003, R005, R006, R007
  - Context: This closeout validates both compile surfaces and documents what cancellation remains unsolved.
  - Reuse_targets: no-default-feature checks, MCP handler tests, binary CLI parse tests
  - Autonomy: standard
  - Risk: low
  - Complexity: low
  - Verification_mode: required
  - Verification_status: pending
  - DoD: The ledger records validation commands, confirms metadata operations stay inline, and explicitly notes that deep browser cancellation remains future work.
  - Validation: Run the full verification plan from `design.md`.
  - Escalate if: The blocking boundary changes public MCP behavior, generated schemas, or server transport setup.

## Done

- (none)

