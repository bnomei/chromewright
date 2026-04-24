# Requirements - contract-domain-extraction

## Scope

Extract pure, schema-facing browser-use contract types into a focused internal contract module while preserving the current one-crate package and public MCP behavior.

This spec covers DTO movement, compatibility re-exports, schema stability, and compile/test validation. It does not split the crate into a workspace, redesign the MCP tool surface, or move browser IO/runtime code.

## Requirements

- R001: When schema-facing DTOs move out of `src/tools/core/mod.rs` or `src/browser/backend.rs`, the system shall preserve the existing serialized MCP input and output shapes.
- R002: When tools, browser session code, or MCP adapters need document envelope, target, snapshot-scope, viewport, or tool-result DTOs, the system shall import those types from `crate::contract` or compatibility re-exports rather than from unrelated implementation hubs.
- R003: When a moved type is currently re-exported from `crate::tools`, `crate::browser`, or the crate root, the system shall preserve a compatibility re-export for the migration wave.
- R004: If a type depends on `headless_chrome`, `rmcp`, `clap`, `axum`, filesystem artifact IO, or live browser evaluation, then the type shall not be moved into the pure contract module in this spec.
- R005: When JSON schemas are generated after the move, the system shall produce the same public schemas except for explicitly approved changes from prerequisite viewport contract specs.
- R006: When target and cursor contract types move, stale-handle and selector-rebind behavior shall remain owned by target-resolution services rather than by passive DTOs.
- R007: When viewport request/result types move, runtime validation shall continue to reuse the existing browser validation logic and schema-visible constraints.
- R008: If the move reveals that a DTO cannot be extracted without behavior changes, then that DTO shall stay in place and the ledger shall record the blocked reason instead of widening scope.

