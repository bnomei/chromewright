# Design - viewport-metrics-backend-api

## Goal

Make viewport metrics a first-class browser capability instead of a repeated JavaScript snippet.

## Architecture Alignment

This spec remains active and becomes the first narrow backend-capability extraction in the architecture program. It should land before `contract-domain-extraction` moves viewport DTOs and before `snapshot-projector-extraction` removes snapshot-scope policy from `src/tools/core/mod.rs`.

The page-metrics extraction formerly planned in `core-backend-seam-extraction` is intentionally absorbed here: if `ScreenshotPageMetrics` needs a focused home to expose typed viewport metrics cleanly, this spec owns that movement.

## Distilled Discovery

- `src/browser/backend.rs` already defines `ViewportMetrics`.
- Chrome viewport metrics are currently derived through `ScreenshotPageMetrics::evaluate(tab).viewport_metrics()` for `set_viewport` post-apply and post-reset results.
- `src/tools/core/mod.rs` has a separate `live_viewport_metrics(...)` helper that evaluates `window.innerWidth`, `window.innerHeight`, and `window.devicePixelRatio` directly, returning `None` on any failure.
- `src/browser/backend/fake.rs` has deterministic viewport state, but also recognizes viewport metric probes by checking whether script text contains `window.innerWidth`, `window.innerHeight`, and `window.devicePixelRatio`.
- Tests duplicate the same JavaScript probe in several places.

## API Shape

Add a typed backend method:

```rust
fn viewport_metrics(&self, tab_id: Option<&str>) -> Result<ViewportMetrics>;
```

Add a session wrapper:

```rust
pub(crate) fn viewport_metrics(&self, tab_id: Option<&str>) -> Result<ViewportMetrics>;
```

The method should be read-only and must not invalidate caches.

## Chrome Backend

Implementation should resolve the target tab using the same active-tab and specific-tab helpers used by screenshot and viewport emulation paths.

Implementation should call the existing page-metrics code:

```text
ScreenshotPageMetrics::evaluate(tab)?.viewport_metrics()
```

This preserves sanitization behavior and avoids a third browser-side viewport script.

## Fake Backend

Implementation should read from the existing fake state:

```text
current_viewport_metrics(state, tab_id)
```

After this exists, remove or narrow fake backend support for viewport metrics via script substring matching. Script matching may remain for unrelated browser evaluation tests, but viewport metrics should no longer depend on it.

## Snapshot Scope

Replace `live_viewport_metrics(context)` in `src/tools/core/mod.rs` with a call to `context.session.viewport_metrics(None)`.

Recommended behavior:

- For Chrome and fake backends, failures should propagate as tool errors because those backends support the method.
- If a future backend intentionally does not support metrics, it should return a distinct unsupported error and the caller can decide whether to omit optional `scope.viewport`.

Do not change the JSON shape of `SnapshotScope`. The field remains optional for schema compatibility.

## Validation Plan

Required checks:

```text
cargo test --lib viewport
cargo test --lib build_document_envelope_viewport_mode_scopes_snapshot_handles
cargo test --lib test_snapshot_schema_exposes_mode_and_scope_contract
cargo check --locked
```

Optional live check when Chrome is available:

```text
cargo test --test browser_tools_integration test_set_viewport_tool_emulates_breakpoint_and_snapshot_scope_reports_viewport -- --ignored
```

## Non-Goals

- Renaming public output fields
- Changing `DocumentMetadata`
- Adding viewport metrics to `tab_list`
- Rewriting all fake backend script dispatch
