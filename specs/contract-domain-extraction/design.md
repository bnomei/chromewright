# Design - contract-domain-extraction

## Goal

Create a pure contract home for the stable browser-use concepts before deeper runtime and tool refactors depend on those concepts.

## Distilled Discovery

- `src/lib.rs` says the stable product surface is the binary and MCP tool contract, while Rust modules are implementation details.
- `src/tools/core/mod.rs` currently owns pure DTOs and active behavior together: document envelopes, public targets, snapshot scope, target resolution, tool results, registry, and snapshot projection.
- `src/browser/backend.rs` currently owns viewport DTOs next to Chrome/CDP implementation details.
- `src/dom/tree.rs` owns pure handle-like types such as `NodeRef` and `Cursor`, but it also owns browser extraction adapters that evaluate scripts against `headless_chrome::Tab`.
- Existing schema and MCP tests are the right guardrail for proving no public contract drift.

## Proposed Module Shape

Add a pure module tree:

```text
src/contract/mod.rs
src/contract/document.rs
src/contract/target.rs
src/contract/viewport.rs
src/contract/tool_result.rs
```

Candidate first-wave moves:

- `DocumentEnvelope`, `DocumentResult`, `DocumentActionResult`, `TargetedActionResult`, `SnapshotScope`, and `SnapshotMode`
- `PublicTarget` plus tagged/compat target DTOs, without moving target-resolution behavior
- `TargetStatus` because `TargetedActionResult` serializes it
- `ViewportMetrics`, `ViewportOrientation`, `ViewportEmulation`, `ViewportEmulationRequest`, `ViewportResetRequest`, and `ViewportOperationResult` after the viewport specs stabilize schema and output names. Runtime validation and CDP conversion helpers stay in the browser backend.
- `ToolResult` if the move does not pull in `ToolContext`, `Tool`, `DynTool`, or `ToolRegistry`

Do not move in this spec:

- `ToolContext`
- `ToolRegistry`, `Tool`, or `DynTool`
- `SessionBackend`
- `ChromeSessionBackend`
- `DomTree::from_tab` or `DocumentMetadata::from_tab`
- snapshot projection functions
- MCP conversion code

## Compatibility

The crate can keep compatibility re-exports in the old modules for one migration wave:

```text
crate::tools::DocumentEnvelope
crate::tools::PublicTarget
crate::browser::ViewportMetrics
crate::ViewportMetrics
```

The new implementation code should prefer `crate::contract::*` imports. External-facing docs can continue to describe the MCP contract rather than internal module paths.

## Sequencing

Recommended prerequisites before moving viewport DTOs:

1. `set-viewport-schema-constraints`
2. `viewport-output-contract-alignment`
3. `viewport-metrics-backend-api`

After those land, this spec can move DTOs without preserving known schema gaps.

## Verification Plan

Required checks:

```text
cargo check --locked --no-default-features
cargo check --locked --no-default-features --features mcp-handler
cargo check --locked
cargo test --lib test_set_viewport_schema_is_exported_via_mcp
cargo test --lib test_snapshot_schema_exposes_mode_and_scope_contract
cargo test --lib tool_registry
```

## Non-Goals

- Splitting into multiple crates
- Introducing a public Rust embedding API
- Changing MCP tool names, params, or outputs
- Moving behavior-heavy services into `contract`
- Removing compatibility re-exports in the same wave
