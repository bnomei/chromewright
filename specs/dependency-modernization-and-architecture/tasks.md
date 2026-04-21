# Tasks — dependency-modernization-and-architecture

Meta:
- Spec: dependency-modernization-and-architecture — staged dependency modernization and architecture cleanup
- Depends on: none
- Global scope:
  - `Cargo.toml`
  - `Cargo.lock`
  - `src/bin/mcp_server.rs`
  - `src/browser/`
  - `src/mcp/`
  - `src/tools/`
  - `tests/`
  - `README.md`
  - `specs/dependency-modernization-and-architecture/`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- [ ] T006: Perform focused structural cleanup in the tool layer (owner: unassigned) (scope: `src/browser/`, `src/tools/`, `tests/`) (depends: T003)
  - Covers: R002, R008
  - Context: The tool layer repeats selector/index validation and target resolution across several tools. `input` can also reuse a cached DOM after mutation when index-based lookup was used, which can return stale snapshots. Limit this wave to seams that directly improve correctness and future upgrade cost.
  - Reuse_targets: `ToolContext`, `DomTree::get_selector(...)`, duplicated validation blocks in `click`, `hover`, `input`, and `select`
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: mayor
  - Verification_status: pending
  - DoD: duplicated target resolution is consolidated, DOM invalidation rules are explicit, and tests cover the changed behavior.
  - Validation: `cargo test --locked`, `cargo check --all-features --all-targets --locked`
  - Escalate if: the cleanup grows into a broad redesign instead of a bounded correctness/maintainability pass.
  - Notes: Close with one atomic commit focused on tool-layer cleanup.

## Done

- [x] T002: Make the CLI contract honest (owner: mayor) (scope: `src/bin/mcp_server.rs`, `src/browser/`, `README.md`, `tests/`) (depends: T001)
  - Covers: R002, R007
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: every retained CLI flag is honored end-to-end or removed from the interface and docs.
  - Validation: `cargo test --locked --all-features`, `cargo check --all-features --all-targets --locked`
  - Notes: Wired the CLI to either launch a local browser or connect to an existing DevTools WebSocket, removed unsupported flags and SSE mode from the binary surface, and updated the public docs/examples to match the supported contract.

- [x] T003: Refresh compatible dependencies inside current major lines (owner: mayor) (scope: `Cargo.toml`, `Cargo.lock`) (depends: T001, T002)
  - Covers: R002, R003
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: compatible dependency updates land cleanly and the baseline remains green.
  - Validation: `cargo update`, `cargo test --locked --all-features`, `cargo check --all-features --all-targets --locked`
  - Notes: Refreshed the lockfile to current same-major releases including `headless_chrome 1.0.21`, `axum 0.8.9`, `tokio 1.52.1`, `clap 4.6.1`, `env_logger 0.11.10`, `schemars 1.2.1`, `indexmap 2.14.0`, `serde_json 1.0.149`, and `thiserror 2.0.18`. `Cargo.toml` did not need edits because the existing semver ranges already admitted these updates.

- [x] T004: Upgrade `rmcp` to `1.5.x` and resolve transport fallout (owner: mayor) (scope: `Cargo.toml`, `Cargo.lock`, `src/mcp/`, `src/bin/mcp_server.rs`, `README.md`) (depends: T003)
  - Covers: R002, R003, R005
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: repo builds and tests on `rmcp` `1.5.x`; transport surface is intentional and documented.
  - Validation: `cargo update -p rmcp --precise 1.5.0`, `cargo check --all-features --all-targets --locked`, `cargo test --locked --all-features`
  - Notes: Migrated to `rmcp 1.5.0`, removed the leftover `transport-sse-server` feature flag from the manifest, dropped the now-unused direct `tokio-util` dependency, and updated the MCP server glue to the 1.5 router and `ServerInfo::new(...).with_instructions(...)` conventions. The T002 CLI cleanup meant no further user-facing transport changes were needed here.

- [x] T005: Preserve structured MCP outputs (owner: mayor) (scope: `src/mcp/`, `src/tools/`, `tests/`) (depends: T004)
  - Covers: R002, R006
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: tool results remain readable to humans but are not text-only by construction.
  - Validation: `cargo test --locked --all-features`, `cargo check --all-features --all-targets --locked`
  - Notes: Changed the MCP adapter to emit `structured_content` for successful JSON payloads, `structured_error` payloads for tool-level failures, and `_meta` for preserved tool metadata. Added focused adapter tests to lock in structured success, structured error, and text-only success behavior.

- [x] T001: Stabilize the browser-launch baseline (owner: mayor) (scope: `src/browser/`, `tests/`, `Cargo.toml`) (depends: -)
  - Covers: R001, R002, R004
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: launch configuration supports deterministic debugging-port behavior, and the relevant ignored browser-launch tests are either green or precisely environment-gated.
  - Validation: `cargo test --locked`, `cargo test --locked --lib -- --ignored`
  - Notes: Added repo-owned DevTools port selection instead of inheriting the dependency's `8000..9000` scan, and made ignored browser-launch tests skip only on recognized environment launch failures.

- [x] P000: Capture baseline evidence and write the staged execution spec (owner: mayor) (scope: `specs/dependency-modernization-and-architecture/`) (depends: -)
  - Covers: R001, R002, R003
  - Verification_mode: mayor
  - Verification_status: passed
  - DoD: baseline commands were run, upgrade constraints were documented, and the repo contains requirements, design, and task artifacts for the next work waves.
  - Validation: `cargo test --locked`, `cargo test --locked -- --ignored`, `cargo check --all-features --all-targets --locked`
  - Notes: This planning step should be committed before implementation begins.
