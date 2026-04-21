# Design — dependency-modernization-and-architecture

## Why This Is A Spec

This work is bounded enough for a spec rather than a loop:

- the validation gates are concrete
- the risky dependency change is concentrated in `rmcp`
- the structural cleanup targets are already visible in the repository
- the user explicitly wants stepwise execution with commit boundaries

## Baseline Evidence

Baseline commands run on April 21, 2026:

- `cargo test --locked`
  - Passed.
  - Result: 57 unit tests passed, 3 doctests passed, 34 integration/browser tests remained ignored.
- `cargo test --locked -- --ignored`
  - Failed.
  - Result: 6 ignored library browser tests failed during browser launch.
  - Failure signature: `LaunchFailed("There are no available ports between 8000 and 9000 for debugging")`.
- `cargo check --all-features --all-targets --locked`
  - Passed.
  - Result: the `mcp-server` feature set and binary compile successfully in the current dependency set.

## Current Architecture Map

### Browser core

- `src/browser/config.rs`
  - User-facing launch/connect configuration.
- `src/browser/session.rs`
  - Owns the `headless_chrome::Browser`, creates tabs, resolves the "active" tab, and dispatches tools.

### DOM extraction

- `src/dom/extract_dom.js`
  - Runs in the page and builds the ARIA-oriented snapshot data.
- `src/dom/tree.rs`
  - Parses the extracted JSON, stores selector maps, and provides indexing helpers.

### Tool layer

- `src/tools/mod.rs`
  - Defines `Tool`, `ToolContext`, `ToolRegistry`, and `ToolResult`.
- `src/tools/*.rs`
  - One file per tool, with repeated selector-vs-index validation patterns across several files.

### MCP layer

- `src/mcp/handler.rs`
  - Wraps a `BrowserSession` inside a mutex-backed server object.
- `src/mcp/mod.rs`
  - Registers tools and converts internal tool results into RMCP responses.

### CLI server

- `src/bin/mcp_server.rs`
  - Parses CLI flags, launches the browser-backed server, and exposes stdio, SSE, and streamable HTTP transports.

## Repository Findings That Affect The Plan

### 1. Browser launch reliability is currently the first blocker

Evidence:

- Browser tests fail with `There are no available ports between 8000 and 9000 for debugging`.
- The upstream `headless_chrome` launch options already support an explicit `port`.
- `browser-use-rs` does not currently expose a launch port in `LaunchOptions`, so the repo inherits the dependency default.

Implication:

- Do not start with the `rmcp` upgrade.
- First make the browser launch path deterministic enough that ignored tests can become a useful signal.

### 2. The CLI advertises flags it does not actually honor

Evidence:

- `src/bin/mcp_server.rs` parses `--executable-path`, `--cdp-endpoint`, `--ws-endpoint`, `--user-data-dir`, and `--log-file`.
- The constructed `LaunchOptions` currently uses only `headless`.
- `BrowserSession::connect(...)` exists in the library, but the binary never routes into it.

Implication:

- The CLI contract is unreliable today.
- Fixing or trimming this surface should happen before or alongside major transport work.

### 3. The current MCP bridge throws away structure

Evidence:

- `src/mcp/mod.rs` serializes internal tool data with `serde_json::to_string_pretty(...)` and wraps it in `Content::text(...)`.
- `ToolResult` is already structured JSON internally.

Implication:

- Even before the `rmcp` upgrade, there is a clear architectural seam for improvement.
- After the `rmcp` upgrade, the repo should move toward structured output rather than text-only payloads.

### 4. `rmcp` is the main disruptive dependency upgrade

Evidence:

- Current repo usage: `rmcp = "0.8"` plus `transport-sse-server`, `transport-streamable-http-server`, and macro-based tool routing.
- Current CLI implements stdio, SSE, and streamable HTTP server modes.
- Upstream `rmcp` `1.5.0` changelog states SSE transport support was removed in the `1.x` line.
- Current `rmcp` `1.5.0` examples use `ServerInfo::new(...)` and updated tool-handler patterns.

Implication:

- `rmcp` is not a "bump and reformat imports" change.
- The repo must either:
  - drop SSE mode,
  - keep SSE behind a repo-owned compatibility layer, or
  - postpone the `rmcp` upgrade until that product decision is made.

Recommended default:

- Keep `stdio` and streamable HTTP.
- Remove built-in SSE mode during the `rmcp` 1.x migration unless the user explicitly wants SSE retained.

### 5. Structural cleanup can be focused instead of broad

Evidence:

- Selector/index resolution is duplicated across `click`, `hover`, `input`, and `select`.
- `ToolContext` caches DOM state.
- `input` can read the DOM to resolve an index and then reuse that cached DOM after typing, which makes the returned snapshot stale for index-based calls.
- `switch_tab` notes that there is no proper session-level tab switch API and works directly on the active browser tabs.

Implication:

- The cleanup wave should target upgrade friction, not rewrite the whole project.
- The most valuable cleanup is:
  - shared target resolution
  - explicit DOM invalidation/refresh rules
  - more explicit active-tab ownership

## Dependency Matrix

Version snapshot confirmed on April 21, 2026 from current lockfile plus crates.io lookups.

| Crate | Current manifest / resolved | Latest published | Recommended wave | Notes |
| --- | --- | --- | --- | --- |
| `rmcp` | `0.8` / `0.8.5` | `1.5.0` | dedicated major wave | Breaks SSE transport assumptions. |
| `headless_chrome` | `1.0.18` / `1.0.18` | `1.0.21` | low-risk same-major wave | Also relevant to launch-port reliability work. |
| `axum` | `0.8` / `0.8.6` | `0.8.9` | low-risk compatible refresh | Mostly lockfile-level if feature remains. |
| `tokio` | `1` / `1.48.0` | `1.52.1` | low-risk compatible refresh | No major migration expected. |
| `tokio-util` | `0.7` / `0.7.17` | `0.7.18` | low-risk compatible refresh | Pair with tokio refresh. |
| `clap` | `4.5` / `4.5.51` | `4.6.1` | low-risk compatible refresh | Useful once CLI behavior is corrected. |
| `env_logger` | `0.11` / `0.11.8` | `0.11.10` | low-risk compatible refresh | Minimal risk. |
| `schemars` | `1.1` / `1.1.0` | `1.2.1` | low-risk compatible refresh | Worth pairing with RMCP structured-output work. |
| `indexmap` | `2.0` / `2.12.0` | `2.14.0` | low-risk compatible refresh | Lockfile-level or manifest relaxation only. |
| `html2md` | `0.2` / `0.2.15` | `0.2.15` | no-op | Already current. |
| `serde` | `1.0` / `1.0.228` | `1.0.228` | no-op | Already current. |
| `serde_json` | `1.0` / `1.0.145` | `1.0.149` | low-risk compatible refresh | Lockfile-level refresh. |
| `thiserror` | `2.0` / `2.0.17` | `2.0.18` | low-risk compatible refresh | Lockfile-level refresh. |
| `anyhow` | `1.0` / `1.0.100` | `1.0.102` | low-risk compatible refresh | Lockfile-level refresh. |
| `async-trait` | `0.1` / `0.1.89` | `0.1.89` | no-op | Already current. |
| `log` | `0.4` / `0.4.28` | `0.4.29` | low-risk compatible refresh | Lockfile-level refresh. |

## Execution Strategy

### Wave 1: make the baseline trustworthy

- Fix browser launch/test determinism.
- Fix or trim misleading CLI flags.
- Keep dependency versions mostly unchanged.

Reason:

- Without this wave, later breakage will be hard to attribute to either the environment or the upgrade.

### Wave 2: refresh compatible dependencies

- Update lockfile and same-major direct dependencies that do not require architectural decisions.
- Re-run the full baseline.

Reason:

- This buys easier compiler feedback and smaller diffs before the `rmcp` migration.

### Wave 3: migrate `rmcp`

- Update features and API usage to `rmcp` `1.5.0`.
- Remove or replace SSE support.
- Update MCP response handling toward structured output.

Reason:

- This is the only direct major dependency upgrade currently visible in the repo.

### Wave 4: focused structural cleanup

- Consolidate target resolution.
- Make DOM invalidation explicit.
- Reduce brittle active-tab heuristics where possible.

Reason:

- These changes improve correctness and make future upgrades cheaper, but they should follow the dependency waves so that their benefit is measured against a modernized baseline.

## Commit Policy

Every execution task in `tasks.md` closes with:

1. code changes limited to that task's scope
2. validation commands recorded in the task
3. one atomic git commit before the next task begins

## External References

- crates.io `rmcp`: https://crates.io/crates/rmcp
- crates.io `headless_chrome`: https://crates.io/crates/headless_chrome
- crates.io `axum`: https://crates.io/crates/axum
- crates.io `tokio`: https://crates.io/crates/tokio
- RMCP migration guide: https://github.com/modelcontextprotocol/rust-sdk/discussions/716
- RMCP repository: https://github.com/modelcontextprotocol/rust-sdk
