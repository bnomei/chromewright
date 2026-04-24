# Design - snapshot-projector-extraction

## Goal

Turn snapshot projection into a pure, testable policy module and leave `build_document_envelope` responsible only for orchestration.

## Distilled Discovery

- `src/tools/core/mod.rs` currently owns `build_document_envelope`, live viewport metrics reads, snapshot projection, delta projection, cache-entry conversion, scoped rendering, target resolution, tool result normalization, and registry composition.
- Projection functions are clustered around `SnapshotProjection`, `snapshot_projection`, `delta_snapshot_projection`, `snapshot_cache_entry_from_projection`, `delta_snapshot_text`, `delta_snapshot_nodes`, and scoped rendering helpers.
- `snapshot_cache_entry` currently lives in `src/browser/session/cache.rs` and should remain session storage, not projector-owned mutable state.
- `viewport-metrics-backend-api` should remove the live JS viewport probe before this spec moves projection policy.
- `contract-domain-extraction` should provide stable DTO homes for snapshot scope and document-envelope types before this spec changes imports.

## Proposed Module Shape

Preferred first-wave location:

```text
src/tools/core/snapshot_projection.rs
```

The module can later move closer to `contract` or `dom` if it stays pure. Keeping it under `tools/core` first minimizes churn while separating policy from the central core file.

Candidate API shape:

```rust
pub(crate) struct SnapshotProjectionInput<'a> {
    pub dom: &'a DomTree,
    pub mode: SnapshotMode,
    pub global_interactive_count: usize,
    pub base: Option<&'a SnapshotCacheEntry>,
}

pub(crate) struct SnapshotProjectionOutput {
    pub current: SnapshotProjection,
    pub base_used: Option<SnapshotProjection>,
}
```

Exact names may follow local style. The key rule is that the projector receives all inputs and returns values. It must not read from `BrowserSession`, mutate caches, or record operation metrics.

## Build Envelope Boundary

`build_document_envelope` remains the orchestration boundary:

1. obtain or reuse `DomTree`
2. read typed viewport metrics if needed
3. fetch compatible snapshot cache entry
4. call the projector
5. update cache and envelope metadata
6. record operation metrics

## Validation Plan

Required checks:

```text
cargo test --lib build_document_envelope_viewport_mode_scopes_snapshot_handles
cargo test --lib build_document_envelope_full_mode_preserves_exhaustive_snapshot_handles
cargo test --lib build_document_envelope_delta_mode_falls_back_to_viewport_without_base
cargo test --lib test_snapshot_schema_exposes_mode_and_scope_contract
cargo check --locked
```

Add deterministic unit tests for the extracted projector so future changes do not require live browser tests to prove policy.

## Non-Goals

- Redesigning `snapshot` output
- Changing snapshot cache invalidation
- Changing DOM extraction
- Moving target resolution
- Moving `ToolRegistry`

