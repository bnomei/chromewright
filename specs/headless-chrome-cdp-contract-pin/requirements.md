# Requirements - headless-chrome-cdp-contract-pin

## Scope

Stabilize the `headless_chrome` CDP binding contract used by the browser backend so generated protocol struct drift does not break builds unexpectedly.

This spec covers dependency metadata, CDP request construction helpers, and focused compile/test validation. It does not upgrade Chrome, change browser behavior, or redesign backend module boundaries.

## Requirements

- R001: When the browser backend constructs generated CDP request structs, the dependency manifest shall resolve `headless_chrome` to the exact generated binding version the source code was written against.
- R002: When `Cargo.lock` resolves `headless_chrome`, the manifest and lockfile shall agree on the intended exact version rather than relying on a broad compatible range.
- R003: When viewport emulation or screenshot code constructs CDP request structs, the system shall route construction through small local helper functions or types so future generated-field drift is isolated to one place.
- R004: If a future dependency update changes generated CDP request fields, then the update shall fail at the helper boundary and shall not require scattered call-site edits.
- R005: When the dependency contract is changed, the system shall pass locked compilation and focused browser-backend tests that exercise screenshot and viewport CDP paths.
- R006: The spec shall not introduce behavior changes to screenshot capture, viewport emulation, tab targeting, or MCP tool schemas.

