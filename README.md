# browser-use

[中文文档](README_CN.md)

A lightweight Rust library for browser automation via Chrome DevTools Protocol (CDP).

## ✨ Highlights

- **Zero Node.js dependency** - Pure Rust implementation directly controlling browsers via CDP
- **Lightweight & Fast** - No heavy runtime, minimal overhead
- **MCP Integration** - Built-in Model Context Protocol server for AI-driven automation
- **Simple API** - High-level document tools for navigation, interaction, and extraction

## Installation

```bash
cargo add browser-use
```

## Quick Start

```rust
use browser_use::browser::BrowserSession;

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
cargo run --features mcp-server --bin mcp-server

# Visible local browser over stdio
cargo run --features mcp-server --bin mcp-server -- --headed

# Connect to an existing Chrome DevTools WebSocket instead of launching Chrome
cargo run --features mcp-server --bin mcp-server -- \
  --ws-endpoint ws://127.0.0.1:9222/devtools/browser/<id>

# Expose streamable HTTP transport on localhost:3000/mcp
cargo run --features mcp-server --bin mcp-server -- --transport http
```

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

- Default high-level agent tools for snapshot, navigate, click, input, wait, tabs, and extraction
- DOM extraction with revision-scoped node references and iframe metadata
- CSS selector, numeric index, or `node_ref` targeting for document interactions
- Explicit operator opt-ins for raw JavaScript evaluation and filesystem-bound screenshots
- Thread-safe browser session management

## Tool Surfaces

The default `ToolRegistry` and MCP server expose the high-level document interaction contract.
Raw JavaScript evaluation and path-based screenshot capture are intentionally outside that default surface.

For direct Rust integrations, opt into those operator tools explicitly:

```rust
use browser_use::tools::ToolRegistry;

let mut registry = ToolRegistry::with_defaults();
registry.register_operator_tools();
```

## Requirements

- Rust 1.70+
- Chrome or Chromium installed

## Acknowledgments

This project was inspired by and references [agent-infra/mcp-server-browser](https://github.com/bytedance/UI-TARS-desktop/tree/main/packages/agent-infra/mcp-servers/browser).

## License

MIT
