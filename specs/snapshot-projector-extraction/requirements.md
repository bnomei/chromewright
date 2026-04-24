# Requirements - snapshot-projector-extraction

## Scope

Extract snapshot projection, scoped rendering, and delta policy from `src/tools/core/mod.rs` into a pure projector module.

This spec covers internal module movement, pure projector APIs, fixture tests, and unchanged snapshot/tool behavior. It does not redesign the public snapshot payload.

## Requirements

- R001: When snapshot projection code moves out of `src/tools/core/mod.rs`, the system shall preserve current `snapshot` viewport, full, and delta semantics.
- R002: When the projector computes a projection, it shall depend only on explicit inputs such as `DomTree`, snapshot mode, prior cache entry data, viewport metrics, and global interactive counts, not on `ToolContext` or `BrowserSession`.
- R003: When `build_document_envelope` needs snapshot data, it shall delegate projection policy to the projector and keep browser reads, cache lookup, and metrics recording outside the projector.
- R004: When delta mode has no compatible base, the system shall preserve the existing fallback behavior.
- R005: When scoped snapshot rendering preserves persistent chrome or local viewport nodes, the system shall preserve current handle and node ordering behavior.
- R006: When projector tests run, they shall cover viewport, full, delta, cache-entry conversion, and scoped rendering using deterministic fixtures.
- R007: The change shall preserve MCP schemas, serialized tool outputs, and snapshot cache invalidation behavior.

