# Design - typed-browser-command-layer

## Goal

Create a typed seam between tool services and browser-side JavaScript so tests and future backend work no longer depend on recognizing rendered script text.

## Distilled Discovery

- `SessionBackend` already has an internal abstraction point for browser operations, but most JS-backed operations still pass raw script strings through `evaluate`.
- `src/tools/browser_kernel.rs` centralizes script assembly for several interaction paths.
- `src/tools/actionability.rs` and `src/tools/services/interaction.rs` build reusable browser-side probes.
- `src/browser/backend/fake.rs` returns many canned results by checking `script.contains(...)` for viewport metrics, actionability, scroll, selector identity, click, input, hover, select, and inspection paths.
- The raw `evaluate` tool is operator-only and must keep accepting arbitrary JavaScript.

## Proposed Command Model

Add internal command types near the browser backend boundary:

```text
src/browser/commands.rs
```

First-wave command candidates:

```rust
BrowserCommand::ActionabilityProbe(ActionabilityProbeRequest)
BrowserCommand::SelectorIdentityProbe(SelectorIdentityProbeRequest)
BrowserCommand::Interaction(InteractionCommand)
```

The exact names can follow local style, but command requests and results should be plain serde-friendly Rust structs. The Chrome backend can keep rendering the existing JS through `browser_kernel` helpers. The fake backend handles command variants directly.

## Backend Trait Strategy

Add a narrow method to `SessionBackend`:

```rust
fn execute_command(&self, command: BrowserCommand) -> Result<BrowserCommandResult>;
```

The default implementation may return `BackendUnsupported` until a backend opts in. Do not remove `evaluate`; raw JS and unmigrated operations can keep using it.

## Migration Strategy

Migrate in this order:

1. actionability probe
2. selector identity probe used by interaction handoff
3. interaction actions for click/input/hover/select

Keep rendered JS equivalence tests around Chrome rendering. Move behavior tests to command payload/result assertions.

## Validation Plan

Required checks:

```text
cargo test --lib actionability
cargo test --lib interaction
cargo test --lib click
cargo test --lib input
cargo test --lib hover
cargo test --lib select
cargo check --locked
```

Optional live checks when Chrome is available:

```text
cargo test --test browser_tools_integration -- --ignored
```

## Non-Goals

- Removing or restricting the `evaluate` operator tool
- Rewriting browser-side JavaScript semantics
- Replacing `headless_chrome`
- Migrating every fake backend script branch in one task

