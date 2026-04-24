# Design - headless-chrome-cdp-contract-pin

## Goal

Make the generated CDP protocol dependency explicit and reduce the blast radius of generated struct changes.

## Architecture Alignment

This spec remains active. It owns only the generated-CDP helper boundary and exact dependency pinning. The broader backend/module cleanup that used to be grouped with this work is now split across `viewport-metrics-backend-api`, `mcp-blocking-execution-boundary`, `typed-browser-error-taxonomy`, `typed-browser-command-layer`, and `snapshot-projector-extraction`.

That split keeps this spec low risk: it can land before or alongside later architecture work without changing public behavior.

## Distilled Discovery

- `Cargo.toml` currently declares `headless_chrome = "1.0.18"`, while `Cargo.lock` resolves `headless_chrome` to `1.0.21`.
- `src/browser/backend.rs` constructs generated CDP structs directly for `Page::CaptureScreenshot`, `Emulation::SetDeviceMetricsOverride`, `Emulation::SetTouchEmulationEnabled`, and `Emulation::ClearDeviceMetricsOverride`.
- `Emulation::SetDeviceMetricsOverride` construction currently includes generated fields such as `display_feature` and `device_posture`.
- Generated CDP structs are not a stable abstraction boundary for this crate; adding or removing generated fields can make exhaustive struct literals fail at compile time.
- The existing behavior is correct enough to preserve. This spec is contract hardening, not a runtime feature change.

## Architecture

### Dependency Contract

Change the dependency declaration from a broad compatible version to an exact version matching the lockfile:

```toml
headless_chrome = "=1.0.21"
```

This is intentionally conservative. The crate depends on generated protocol bindings, not only on semantic Rust APIs. Exact pinning makes future binding updates explicit review events.

### CDP Helper Boundary

Keep the public backend/session API unchanged. Add local helper functions near the Chrome backend implementation, or in a small `src/browser/backend/cdp.rs` module if the backend file is already being edited heavily.

Candidate helpers:

```text
capture_screenshot_method(request, clip) -> Page::CaptureScreenshot
set_device_metrics_override(emulation) -> Emulation::SetDeviceMetricsOverride
set_touch_emulation(enabled) -> Emulation::SetTouchEmulationEnabled
clear_device_metrics_override() -> Emulation::ClearDeviceMetricsOverride
```

These helpers should be thin and deterministic. They exist to localize generated CDP field shape, not to introduce new behavior.

### Validation

Use compile checks as the primary guard because generated-field drift is a compile-time failure mode. Runtime-focused tests then ensure the helper extraction did not change behavior.

Recommended validation:

```text
cargo check --locked
cargo test --lib viewport
cargo test --lib screenshot
cargo test --test browser_tools_integration test_set_viewport_tool_emulates_breakpoint_and_snapshot_scope_reports_viewport -- --ignored
```

The ignored browser integration test should run only when the environment has Chrome/CDP support.

## Non-Goals

- Upgrading `headless_chrome`
- Changing browser launch flags
- Changing screenshot output
- Changing `set_viewport` params or output
- Refactoring all of `src/browser/backend.rs`
