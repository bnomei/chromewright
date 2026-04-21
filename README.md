# browser-use

[中文文档](README_CN.md)

A lightweight Rust library for browser automation via Chrome DevTools Protocol (CDP).

## ✨ Highlights

- **Zero Node.js dependency** - Pure Rust implementation directly controlling browsers via CDP
- **Lightweight & Fast** - No heavy runtime, minimal overhead
- **MCP Integration** - Built-in Model Context Protocol server for AI-driven automation
- **Simple API** - Easy-to-use tools for common browser operations

## Installation

```bash
cargo add browser-use
```

## Quick Start

```rust
use browser_use::browser::BrowserSession;

// Launch browser and navigate
let session = BrowserSession::launch(Default::default())?;
session.navigate("https://example.com", None)?;

// Extract DOM with indexed interactive elements
let dom = session.extract_dom()?;
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

- Navigate, click, input, screenshot, extract content
- DOM extraction with indexed interactive elements
- CSS selector or numeric index-based element targeting
- Thread-safe browser session management

## Requirements

- Rust 1.70+
- Chrome or Chromium installed

## Acknowledgments

This project was inspired by and references [agent-infra/mcp-server-browser](https://github.com/bytedance/UI-TARS-desktop/tree/main/packages/agent-infra/mcp-servers/browser).

## License

MIT
