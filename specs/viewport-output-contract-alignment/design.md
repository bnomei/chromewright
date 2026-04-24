# Design - viewport-output-contract-alignment

## Goal

Reduce client ambiguity caused by `viewport_after` meaning different shapes in different tool outputs.

## Architecture Alignment

This spec remains active as a contract-stabilization prerequisite. It should land before `contract-domain-extraction` moves shared output DTOs, so the moved types expose canonical names rather than carrying avoidable alias ambiguity as their primary shape.

## Distilled Discovery

- `set_viewport` currently returns `viewport_after` with `width`, `height`, and `device_pixel_ratio`.
- `scroll` currently returns `viewport_after` with `scroll_y`, `is_at_top`, and `is_at_bottom`.
- Both shapes are valid in per-tool schemas, but shared clients often normalize top-level output fields across tools.
- The existing `set-viewport` spec intentionally reused `viewport_after` from `scroll`; this follow-up spec corrects that contract ambiguity without requiring an immediate breaking removal.
- `tool-output-contract-normalization` already covers broader output consistency, so this spec should stay narrow and either depend on or feed into that work.

## Canonical Output Names

Use shape-specific names:

```text
set_viewport.viewport_metrics_after:
  width: f64
  height: f64
  device_pixel_ratio: f64

scroll.scroll_after:
  scroll_y: i64
  is_at_top: bool
  is_at_bottom: bool
```

Compatibility aliases may remain for one migration window:

```text
set_viewport.viewport_after == set_viewport.viewport_metrics_after
scroll.viewport_after == scroll.scroll_after
```

The implementation should avoid maintaining two independent values. Prefer serializing aliases from the same data object or building output from one canonical value.

## Schema and Docs

MCP output schemas should include canonical fields. If compatibility aliases remain, descriptions or tests should make clear which field is canonical.

README should tell agents to read:

- `set_viewport.viewport_metrics_after` for width, height, and DPR
- `snapshot.scope.viewport` for the current reread viewport
- `scroll.scroll_after` for scroll position state

## Migration Policy

This spec is non-breaking by default. Removing `viewport_after` aliases is out of scope unless the user explicitly chooses a breaking cleanup.

If aliases remain, regression tests should assert both:

- canonical fields exist
- alias fields match canonical values exactly

## Validation Plan

Required checks:

```text
cargo test --lib set_viewport
cargo test --lib scroll
cargo test --lib test_set_viewport_schema_is_exported_via_mcp
cargo test --test browser_tools_integration test_set_viewport_tool_emulates_breakpoint_and_snapshot_scope_reports_viewport -- --ignored
```

The ignored browser integration check should be run only when Chrome/CDP support is available.

## Non-Goals

- Removing aliases without explicit approval
- Adding viewport metrics to `DocumentMetadata`
- Changing `snapshot.scope.viewport`
- Changing scroll behavior
