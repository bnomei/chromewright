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

- [ ] T002: Make the CLI contract honest (owner: unassigned) (scope: `src/bin/mcp_server.rs`, `src/browser/`, `README.md`, `tests/`) (depends: T001)
  - Covers: R002, R007
  - Context: The binary currently parses `--executable-path`, `--cdp-endpoint`, `--ws-endpoint`, `--user-data-dir`, and `--log-file`, but only `headless` is applied. The public surface must either be wired through or reduced.
  - Reuse_targets: `BrowserSession::connect(...)`, `LaunchOptions`, existing CLI parsing
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: mayor
  - Verification_status: pending
  - DoD: every retained CLI flag is honored end-to-end or removed from the interface and docs.
  - Validation: `cargo check --all-features --all-targets --locked`, `cargo test --locked`
  - Escalate if: retaining both remote-connect and local-launch modes creates an unclear user contract that needs a product decision.
  - Notes: Close with one atomic commit before any dependency refreshes.

- [ ] T003: Refresh compatible dependencies inside current major lines (owner: unassigned) (scope: `Cargo.toml`, `Cargo.lock`) (depends: T001, T002)
  - Covers: R002, R003
  - Context: Most direct dependencies are already on current major versions. The low-risk wave is to refresh lockfile-compatible crates and any same-major manifest ranges before attempting the `rmcp` major jump.
  - Reuse_targets: current dependency ranges already present in `Cargo.toml`
  - Autonomy: standard
  - Risk: low
  - Complexity: medium
  - Bundle_with: T006
  - Verification_mode: mayor
  - Verification_status: pending
  - DoD: compatible dependency updates land cleanly and the baseline remains green.
  - Validation: `cargo test --locked`, `cargo check --all-features --all-targets --locked`
  - Escalate if: a supposedly compatible update changes public behavior or forces a Rust/MSRV decision.
  - Notes: Close with one atomic commit dedicated to low-risk dependency refreshes.

- [ ] T004: Upgrade `rmcp` to `1.5.x` and resolve transport fallout (owner: unassigned) (scope: `Cargo.toml`, `Cargo.lock`, `src/mcp/`, `src/bin/mcp_server.rs`, `README.md`) (depends: T003)
  - Covers: R002, R003, R005
  - Context: Upstream `rmcp` `1.x` changed server patterns and removed built-in SSE transport support. This repo currently depends on `transport-sse-server` and uses the `0.8` API shape. The recommended default is to keep stdio and streamable HTTP and remove SSE mode during the migration unless the user explicitly wants SSE preserved.
  - Reuse_targets: upstream `rmcp` `1.x` examples using `ServerInfo::new(...)`, existing tool-router layout in `src/mcp/`
  - Read_allowlist: local cargo registry `rmcp-1.5.0`, official migration guide
  - Autonomy: strict
  - Risk: high
  - Complexity: high
  - Verification_mode: mayor
  - Verification_status: pending
  - DoD: repo builds and tests on `rmcp` `1.5.x`; transport surface is intentional and documented.
  - Validation: `cargo check --all-features --all-targets --locked`, `cargo test --locked`
  - Escalate if: the user wants to preserve SSE transport instead of removing it as part of the migration.
  - Notes: Close with one atomic commit dedicated to the RMCP migration.

- [ ] T005: Preserve structured MCP outputs (owner: unassigned) (scope: `src/mcp/`, `src/tools/`, `tests/`) (depends: T004)
  - Covers: R002, R006
  - Context: The current MCP adapter converts internal JSON results into prettified text, which throws away structure that clients could consume directly. After the `rmcp` upgrade, move this seam to structured outputs with text fallback only when necessary.
  - Reuse_targets: internal `ToolResult`, RMCP structured result helpers, JSON wrappers where appropriate
  - Autonomy: standard
  - Risk: medium
  - Complexity: medium
  - Verification_mode: mayor
  - Verification_status: pending
  - DoD: tool results remain readable to humans but are not text-only by construction.
  - Validation: `cargo test --locked`, targeted MCP result conversion tests if added
  - Escalate if: an MCP client compatibility requirement forces a dual-format response policy.
  - Notes: Close with one atomic commit after validation is green.

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
