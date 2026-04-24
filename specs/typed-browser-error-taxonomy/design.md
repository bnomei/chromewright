# Design - typed-browser-error-taxonomy

## Goal

Move recovery decisions out of display strings so browser recovery policy can evolve without fragile substring and prefix decoding throughout the codebase.

## Distilled Discovery

- `src/error.rs` centralizes `BrowserError`, but many variants carry plain `String` details.
- `src/browser/backend.rs` had encoded attach-session degraded reason prefixes and recoverable page-target-loss message matching.
- `src/tools/core/mod.rs` maps `BrowserError` to structured tool failures and has a dedicated attach-session degraded failure helper.
- `src/mcp/mod.rs` preserves structured tool failures as `CallToolResult` and maps internal errors separately.
- Current tests assert attach-degraded payloads and page-target-loss recovery behavior, but some paths still rely on reason text.

## Proposed Types

Add typed details near `BrowserError`:

```rust
pub(crate) struct PageTargetLostDetails {
    pub operation: String,
    pub detail: String,
    pub recoverable: bool,
    pub recovery_hint: Option<String>,
}

pub(crate) struct BackendUnsupportedDetails {
    pub capability: String,
    pub operation: String,
}
```

Add or refine variants:

```rust
BrowserError::PageTargetLost(PageTargetLostDetails)
BrowserError::BackendUnsupported(BackendUnsupportedDetails)
```

Keep `Display` useful for logs, but do not use `Display` as the data transport between backend, tools, and MCP conversion.

## Adapter Boundary

String matching against upstream `headless_chrome` text may remain only where the upstream error is first converted into a `BrowserError`. After conversion, downstream code should branch on variants and typed fields.

## Compatibility

Public structured failures should keep existing fields such as:

```text
code
kind
operation
detail
recovery_hint
```

The source of those fields changes from encoded strings to typed data. Any new field must be covered by schema or payload tests. The former encoded attach-degraded string carrier is removed rather than retained as a compatibility field because it was never part of the public structured payload.

## Validation Plan

Required checks:

```text
cargo test --lib attach_session
cargo test --lib page_target
cargo test --lib structured_failure
cargo test --lib mcp
cargo check --locked
```

## Non-Goals

- Rewriting all error variants
- Changing CLI error display
- Removing all upstream string classification immediately
- Changing public MCP error semantics
