# chromewright

A lightweight Rust library for browser automation via Chrome DevTools Protocol (CDP).

## ✨ Highlights

- **Zero Node.js dependency** - Pure Rust implementation directly controlling browsers via CDP
- **Lightweight & Fast** - No heavy runtime, minimal overhead
- **MCP Integration** - Built-in Model Context Protocol server for AI-driven automation
- **Simple API** - High-level document tools for navigation, interaction, and extraction

## Installation

### Binary From Crates.io

```bash
cargo install chromewright
```

### Library Dependency

If you only want the embeddable Rust library surface and not the standalone server binary dependencies:

```bash
cargo add chromewright --no-default-features
```

Enable `mcp-handler` explicitly if you need the in-process MCP module without the standalone CLI:

```toml
chromewright = { version = "0.2.3", default-features = false, features = ["mcp-handler"] }
```

### From Source

```bash
cargo install --path .
```

## Quick Start

```rust
use chromewright::browser::BrowserSession;

// Launch browser and navigate
let session = BrowserSession::launch(Default::default())?;
session.navigate("https://example.com")?;

// Extract DOM with revision-scoped document metadata
let dom = session.extract_dom()?;
println!("Current revision: {}", dom.document.revision);
```

## MCP Server

Run the built-in MCP server for AI-driven automation:

```bash
# Headless local browser over stdio
cargo run --bin chromewright

# Visible local browser over stdio
cargo run --bin chromewright -- --headed

# Connect to an existing Chrome DevTools WebSocket instead of launching Chrome
cargo run --bin chromewright -- \
  --ws-endpoint ws://127.0.0.1:9222/devtools/browser/<id>

# Expose streamable HTTP transport on localhost:3000/mcp
cargo run --bin chromewright -- --transport http
```

Recommended macOS launch command for a visible dedicated Chrome session that this repo can reconnect to reliably:

```bash
open -na "Google Chrome" --args \
  --remote-debugging-port=9222 \
  --user-data-dir="$HOME/.chromewright-agent-profile"
```

Use this when you want a headed browser without attaching to your personal Chrome profile.
After Chrome is running, point the MCP server at the DevTools browser WebSocket exposed on port `9222`.

Supported browser-mode flags:

- `--headed`
- `--executable-path <PATH>`
- `--user-data-dir <DIR>`
- `--debug-port <PORT>`
- `--ws-endpoint <URL>` for remote browser connections

Supported transports:

- `stdio` (default)
- `http` via streamable HTTP on `--port` and `--http-path`

## Features

- Default high-level agent tools for snapshot, inspect_node, navigate, click, input, wait, tabs, and extraction
- DOM extraction with revision-scoped node references and iframe metadata
- Cursor-first targeted inspection via `snapshot` and `inspect_node`, without expanding into a wide getter family
- Metadata-first post-action envelopes for high-level tools, with the full snapshot surface kept on the `snapshot` tool
- Cursor, CSS selector, numeric index, or `node_ref` targeting for document interactions, with `cursor` preferred for follow-up calls
- Explicit operator opt-ins for raw JavaScript evaluation and filesystem-bound screenshots
- Thread-safe browser session management

## Tool Surfaces

The default `ToolRegistry` and MCP server expose the high-level document interaction contract.
Raw JavaScript evaluation and path-based screenshot capture are intentionally outside that default surface.
High-level action tools now return updated document metadata by default; call `snapshot` when you need
the full YAML snapshot plus actionable-node list.
For targeted DOM reads, use `snapshot` to choose a node and reuse its `cursor`, then call
`inspect_node` as the default targeted inspection tool; keep `evaluate` as the explicit operator
escape hatch when the bounded inspection surface is insufficient. During the migration, targetable
inspection and interaction tools still accept `selector`, `index`, and `node_ref`, but `cursor`
is the preferred follow-up handle.
Non-targeted `scroll` and `press_key` calls also return compact follow-up state for the next tool
decision: fresh `document` metadata plus `viewport_after` or `focus_after` when those hints are
available.
See [docs/tool-description-index.md](docs/tool-description-index.md) for the concise tool-description
index and wording rules.

For direct Rust integrations, opt into those operator tools explicitly:

```rust
use chromewright::tools::ToolRegistry;

let mut registry = ToolRegistry::with_defaults();
registry.register_operator_tools();
```

Once operator tools are registered, `evaluate` and `screenshot` still require `confirm_unsafe = true`
per call. High-level `navigate` and `new_tab` also reject unsafe schemes such as `data:` and `file:`
unless the caller passes `allow_unsafe = true`.

## Requirements

- Rust 1.85+
- Chrome or Chromium installed

## Release Plan

The repository is set up to support both crates.io distribution and GitHub release archives for the `chromewright` binary.
The fastest way to reserve the crate name is to publish the package to crates.io first, then attach prebuilt archives from tagged GitHub releases.

## Acknowledgments

This project was inspired by and references [agent-infra/mcp-server-browser](https://github.com/bytedance/UI-TARS-desktop/tree/main/packages/agent-infra/mcp-servers/browser).

## License

MIT
