# chromewright

[![Crates.io Version](https://img.shields.io/crates/v/chromewright)](https://crates.io/crates/chromewright)
[![Build Status](https://github.com/bnomei/browser-use-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/bnomei/browser-use-rs/actions/workflows/ci.yml)
[![Docs.rs](https://img.shields.io/docsrs/chromewright)](https://docs.rs/chromewright)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

Chromewright is a local-first browser automation MCP server and Rust library built on Chrome DevTools Protocol (CDP). It can attach to an existing Chrome or Chromium session or launch its own browser, then expose a bounded, agent-oriented tool surface for navigation, reading, tab management, and interaction without a Node.js runtime.

It is built for the moment when an MCP client needs a real browser, but not an unbounded automation stack. Under the hood, Chromewright combines CDP session control, revision-scoped DOM extraction, cursor-based targeting, and consistent tool-result metadata. It is not a general-purpose end-to-end test runner. It is a browser control layer for AI agents and Rust applications.

## What To Use Chromewright For

Use Chromewright when you need browser-aware automation with a stable high-level surface instead of handwritten CDP calls.

- attaching to a running Chrome or Chromium session or launching a disposable browser from Rust
- exposing a real browser to MCP clients over streamable HTTP or stdio
- reading pages through `snapshot`, `inspect_node`, `get_markdown`, `extract`, and `read_links`
- driving bounded interactions through `navigate`, `click`, `input`, `select`, `hover`, `press_key`, `scroll`, `wait`, and the tab tools
- targeting follow-up actions with revision-scoped `cursor` or `node_ref` handles instead of relying only on fragile selectors
- embedding the same tool surface directly inside a Rust process with `ToolRegistry`

## Installation

### Cargo

Published crate:

```bash
cargo install chromewright
```

Library-only dependency without the standalone server binary surface:

```bash
cargo add chromewright --no-default-features
```

If you want the in-process MCP server module without the CLI transport dependencies:

```toml
chromewright = { version = "0.2.3", default-features = false, features = ["mcp-handler"] }
```

### GitHub Releases

Download a prebuilt archive or source package from GitHub Releases, extract it, and place `chromewright` on your `PATH`.

### From source

The published crate and binary are named `chromewright`. The repository is currently hosted as `browser-use-rs`.

```bash
git clone https://github.com/bnomei/browser-use-rs.git
cd browser-use-rs
cargo install --path .
```

If you only want a local release build:

```bash
cargo build --release
```

## Quickstart

### 1) Prepare Chrome or Chromium

The default attach mode expects a browser exposing DevTools on `http://127.0.0.1:9222`.

Recommended macOS launch command for a dedicated visible Chrome profile:

```bash
open -na "Google Chrome" --args \
  --remote-debugging-port=9222 \
  --user-data-dir="$HOME/.chromewright-agent-profile"
```

Use a dedicated profile when you do not want agent automation attached to your personal browsing session. If you prefer Chromewright to launch its own browser instead, skip this and use `--headed` in the next step.

### 2) Start the recommended Chromewright service

```bash
cargo run --bin chromewright
```

This default mode attaches to Chrome on `127.0.0.1:9222` and serves streamable HTTP on `127.0.0.1:3000/mcp`.

Other common startup modes:

```bash
# release build, same defaults
cargo run --release --bin chromewright

# stdio transport instead of HTTP
cargo run --bin chromewright -- \
  --transport stdio

# connect to a different DevTools endpoint
cargo run --bin chromewright -- \
  --ws-endpoint http://127.0.0.1:9333

# launch a new visible browser instead of attaching to an existing one
cargo run --bin chromewright -- --headed
```

The default MCP endpoint is:

`http://127.0.0.1:3000/mcp`

### 3) Add Chromewright to your MCP client

Point your client at the loopback HTTP endpoint of the running Chromewright service:

`http://127.0.0.1:3000/mcp`

#### Codex

```toml
[mcp_servers.chromewright]
url = "http://127.0.0.1:3000/mcp"
enabled = true
```

Equivalent stdio configuration:

```toml
[mcp_servers.chromewright]
command = "/absolute/path/to/chromewright"
args = ["--transport", "stdio"]
enabled = true
```

If you need a non-default attach target, add `--ws-endpoint` explicitly in either mode.

#### Other JSON-configured clients

```json
{
  "mcpServers": {
    "chromewright": {
      "transport": "streamable_http",
      "url": "http://127.0.0.1:3000/mcp"
    }
  }
}
```

The exact file name and field names vary by client. The important part is that the client connects to a running Chromewright service at that URL.

## How Chromewright Uses Your Browser

- attach mode connects to an existing Chrome or Chromium session and can see the tabs, cookies, and authenticated state already present in that profile
- launch mode starts a dedicated browser session and tracks the tabs created under that session
- in attach mode, `close` defaults to session-managed cleanup and `close_tab` requires `confirm_destructive = true` before closing an unmanaged active tab
- the normal high-level tool surface reads and interacts through CDP only; it does not write to local files during ordinary MCP use
- filesystem-bound screenshots are excluded from the default surface and remain explicit operator actions

## Use Cases

### Standard MCP browser automation

Once Chromewright is running, the normal workflow is:

1. Use `new_tab` or `tab_list` to establish an active tab. On a fresh session with no active tab, do not call `snapshot` first.
2. Use `snapshot` to get document metadata plus actionable nodes. Its inline `[index=...]` markers are revision-scoped numeric handles that mirror the exposed `nodes` list; they are not a separate durable handle family.
3. Use `inspect_node` for targeted bounded reads, including selector-based inspection of non-actionable nodes such as headings, images, and overlays. Prefer `cursor` when one is available; otherwise a successful inspection may legitimately return `cursor = null`.
4. Use `click`, `input`, `select`, `hover`, `press_key`, `scroll`, `wait`, or the tab tools with `cursor` preferred for follow-up targeting inside a page and stable `tab_id` preferred for multi-tab flows.
5. Refresh `snapshot` after revision-changing actions. `cursor` and `node_ref` are revision-scoped, so rereads are the normal recovery path.

### Direct Rust integration

Chromewright exposes the same high-level tool contract to Rust callers through `BrowserSession`, `ToolContext`, and `ToolRegistry`, so MCP usage and in-process usage share the same mental model and result envelope.

## Workflow Conventions

- Fresh sessions: use `new_tab` or `tab_list` before `snapshot` if you do not already have an active tab.
- Revision-scoped targets: `cursor` and `node_ref` belong to a specific document revision. After navigation or DOM-changing actions, rerun `snapshot` or fall back to selector-based recovery before precise follow-up work.
- Snapshot inline handles: rendered `[index=...]` markers follow the same revision scope as the exposed actionable `nodes`; use them as reread-local hints, not as durable cross-revision IDs.
- `target_status = same`: the tool still resolved the same target, so direct follow-up is usually safe.
- `target_status = rebound`: the tool recovered after a revision change; reread with `snapshot` before more precise chained work.
- `target_status = detached`: the old target no longer exists, often after navigation; reacquire state from the new page before continuing.
- Attach-mode safety: use a disposable browser profile for debugging and treat destructive tab tools as explicit actions, especially on connected sessions.

Chromewright also carries a few small but important contract details:

- `input` accepts `value` as a backward-compatible alias, while `text` remains the canonical field.
- `extract` uses `code = element_not_found` for selector misses and reserves `code = invalid_extract_payload` for malformed extraction results.
- `read_links` returns both the raw `href` attribute and an absolute `resolved_url`.

Companion references:

- [Tool Handoff Contract](docs/tool-handoff-contract.md)
- [Tool Description Index](docs/tool-description-index.md)

## Default Tool Surface

The default `ToolRegistry` and MCP server expose the same 20 high-level tools:

- navigation: `navigate`, `go_back`, `go_forward`, `wait`
- interaction: `click`, `input`, `select`, `hover`, `press_key`, `scroll`
- tabs and lifecycle: `new_tab`, `tab_list`, `switch_tab`, `close_tab`, `close`
- reading and inspection: `snapshot`, `inspect_node`, `get_markdown`, `extract`, `read_links`

The default surface intentionally excludes the operator tools `evaluate` and `screenshot`. For direct Rust integrations, opt into those explicitly:

```rust,no_run
use chromewright::tools::ToolRegistry;

let mut registry = ToolRegistry::with_defaults();
registry.register_operator_tools();
```

High-level action tools return compact follow-up metadata by default. Use `snapshot` when you need the full YAML snapshot plus actionable-node list. For targeted reads, use `snapshot` to choose a node and reuse its `cursor`, then call `inspect_node`; when you need to inspect a non-actionable DOM node such as a heading or image, `inspect_node` also accepts selector-based reads with an optional `cursor`. After revision-changing actions, rerun `snapshot` before more precise target reuse. Targetable tools still accept `selector`, `index`, and `node_ref`, but `cursor` is the preferred follow-up handle.

Read-oriented tools are intentionally distinct: `get_markdown` is the broad reading tool, `extract` is for targeted text or HTML, and `read_links` is for link inventory and planning. For multi-tab work, prefer stable `tab_id` handles from `tab_list`, `new_tab`, `switch_tab`, and `close_tab` instead of relying only on tab indices.

## Library Usage

### Basic browser session

```rust,no_run
use chromewright::{BrowserSession, ConnectionOptions};

fn main() -> chromewright::Result<()> {
    let session = BrowserSession::connect(ConnectionOptions::new("http://127.0.0.1:9222"))?;
    session.navigate("https://example.com")?;

    let dom = session.extract_dom()?;
    println!("Document revision: {}", dom.document.revision);

    Ok(())
}
```

### Executing tools directly in Rust

```rust,no_run
use chromewright::{BrowserSession, ConnectionOptions};
use serde_json::json;

fn main() -> chromewright::Result<()> {
    let session = BrowserSession::connect(ConnectionOptions::new("http://127.0.0.1:9222"))?;

    session.execute_tool("navigate", json!({ "url": "https://example.com" }))?;
    let snapshot = session.execute_tool("snapshot", json!({}))?;

    println!("snapshot ok: {}", snapshot.success);
    Ok(())
}
```

Prefer the crate-root reexports for session setup and browser lifecycle:

```rust,no_run
use chromewright::{BrowserSession, ConnectionOptions, LaunchOptions};
```

The file modules behind `browser::config`, `browser::session`, and the internal tool helpers are implementation details. `src/tools/mod.rs` remains the stable tools facade, with shared primitives under `src/tools/core/` and shared multi-step workflows under `src/tools/services/`.

## Operation Metrics

Finished tool results flow through `ToolContext::finish()`, which attaches `operation_metrics` metadata on `ToolResult.metadata` in Rust and forwards the same metadata through MCP tool metadata. Every finished result includes serialized output size, and measured hot paths add the relevant counters below:

- browser evaluation count
- poll iterations
- DOM extraction count and extraction time
- snapshot render time
- handoff rebuild count and time
- serialized output size

The lightweight validation surface for these metrics is in the normal test suite:

```bash
cargo test --locked --all-features operation_metrics
```

## Safety And Boundaries

- Chromewright drives a real Chrome or Chromium instance through CDP. In attach mode, it sees the tabs, cookies, and authenticated state of the browser profile you give it.
- Use a dedicated browser profile for agent work when you do not want automation attached to your personal session.
- The default tool surface excludes raw JavaScript evaluation and filesystem-bound screenshots.
- Once operator tools are registered, `evaluate` and `screenshot` still require `confirm_unsafe = true` per call.
- `navigate` and `new_tab` reject unsafe schemes such as `data:` and `file:` unless the caller passes `allow_unsafe = true`.
- `cursor` and `node_ref` targets are revision-scoped. After a DOM-changing action, stale targets fail cleanly and should be refreshed from a new `snapshot`.

## License

MIT
