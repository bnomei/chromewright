# chromewright

[![Crates.io Version](https://img.shields.io/crates/v/chromewright)](https://crates.io/crates/chromewright)
[![Build Status](https://github.com/bnomei/browser-use-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/bnomei/browser-use-rs/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

Chromewright is a local-first browser automation MCP server built on Chrome DevTools Protocol (CDP). It can attach to an existing Chrome or Chromium session or launch its own browser, then expose a bounded, agent-oriented tool surface for navigation, reading, tab management, and interaction without a Node.js runtime.

It is built for the moment when an MCP client needs a real browser, but not an unbounded automation stack. Under the hood, Chromewright combines CDP session control, revision-scoped DOM extraction, cursor-based targeting, and consistent tool-result metadata. It is not a general-purpose end-to-end test runner. It is a browser control layer for AI agents.

## What To Use Chromewright For

Use Chromewright when you need browser-aware automation with a stable high-level surface instead of handwritten CDP calls.

- attaching to a running Chrome or Chromium session or launching a disposable browser
- exposing a real browser to MCP clients over streamable HTTP or stdio
- reading pages through `snapshot`, `inspect_node`, `get_markdown`, `extract`, and `read_links`
- driving bounded interactions through `navigate`, `click`, `input`, `select`, `hover`, `press_key`, `scroll`, `wait`, and the tab tools
- targeting follow-up actions with revision-scoped `cursor` or `node_ref` handles instead of relying only on fragile selectors

## Installation

### Cargo

Install the binary from crates.io:

```bash
cargo install chromewright
```

Building from source or installing with Cargo requires Rust 1.88 or newer.

### Homebrew

```bash
brew install bnomei/chromewright/chromewright
```

### GitHub Releases

Download a prebuilt archive or source package from GitHub Releases, extract it, and place `chromewright` on your `PATH`.

### From source

The published package and binary are named `chromewright`. The repository is currently hosted as `browser-use-rs`.

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

Use a dedicated profile when you do not want agent automation attached to your personal browsing session. If you prefer Chromewright to launch its own browser instead, skip this and pass any launch-mode flag in the next step. Launch mode is headed by default; add `--headless` only when you want a hidden browser.

### 2) Start Chromewright

```bash
cargo run --bin chromewright
```

This default mode attaches to Chrome on `127.0.0.1:9222` and serves MCP over stdio.

Other common startup modes:

```bash
# release build, same defaults
cargo run --release --bin chromewright

# serve streamable HTTP on 127.0.0.1:3000/mcp
cargo run --bin chromewright -- serve

# connect to a different DevTools endpoint
cargo run --bin chromewright -- \
  --ws-endpoint http://127.0.0.1:9333

# launch a new visible browser instead of attaching to an existing one
cargo run --bin chromewright -- \
  --user-data-dir /tmp/chromewright-profile

# launch a new visible browser and serve streamable HTTP
cargo run --bin chromewright -- \
  --user-data-dir /tmp/chromewright-profile serve

# launch headless instead of headed
cargo run --bin chromewright -- \
  --headless --user-data-dir /tmp/chromewright-profile
```

### 3) Add Chromewright to your MCP client

#### Codex

Recommended stdio configuration:

```toml
[mcp_servers.chromewright]
command = "/absolute/path/to/chromewright"
enabled = true
```

If you want a long-lived loopback HTTP service instead, start `chromewright serve` separately and point Codex at the running endpoint:

```toml
[mcp_servers.chromewright]
url = "http://127.0.0.1:3000/mcp"
enabled = true
```

If you need a non-default attach target, add `--ws-endpoint` explicitly. If you want Chromewright to launch its own browser from the client command, add a launch-mode flag such as `--user-data-dir /tmp/chromewright-profile`, and add `--headless` only when you do not want a visible browser.

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
- most high-level tools read and interact through CDP only; `screenshot` is the bounded exception and stores a managed PNG artifact for the caller
- `screenshot` is part of the default surface and uses `mode`, optional `tab_id`, optional `target`, and `region` instead of caller-chosen `path` or `confirm_unsafe`

## Use Cases

### Standard MCP browser automation

Once Chromewright is running, the normal workflow is:

1. Use `new_tab` or `tab_list` to establish an active tab. On a fresh session with no active tab, do not call `snapshot` first.
2. Use `snapshot` to get document metadata plus actionable nodes. `mode = "viewport"` is the default local reread, `mode = "delta"` reuses the prior session base when available, and `mode = "full"` keeps the exhaustive escape hatch. Inline `[index=...]` markers only appear for nodes that still expose a public follow-up handle in that returned scope.
3. Use `inspect_node` for targeted bounded reads, including selector-based inspection of non-actionable nodes such as headings, images, and overlays. Prefer `cursor` when one is available; stale cursors may selector-rebound, and a successful inspection may still legitimately return `cursor = null` with a selector-only `target`.
4. Use `screenshot` when you need a managed PNG artifact. `mode = "viewport"` is the default, `mode = "full_page"` captures the whole page, `mode = "element"` requires `target`, and `mode = "region"` requires `region`. `scale = "device"` preserves raw device pixels by default, while `scale = "css"` normalizes output dimensions to CSS pixels. Pass `tab_id` when the capture should target a specific tab without activating it first.
5. Use `click`, `input`, `select`, `hover`, `press_key`, `scroll`, `wait`, or the tab tools with `cursor` preferred for follow-up targeting inside a page and stable `tab_id` preferred for multi-tab flows.
6. Refresh `snapshot` after revision-changing actions. `cursor` and `node_ref` are revision-scoped, so rereads are the normal recovery path.

## Workflow Conventions

- Fresh sessions: use `new_tab` or `tab_list` before `snapshot` if you do not already have an active tab.
- Revision-scoped targets: `cursor` and `node_ref` belong to a specific document revision. After navigation or DOM-changing actions, rerun `snapshot`; stale `cursor` replay may selector-rebound, but treat rebound as a signal to reread before more precise chained work.
- Snapshot modes: default `viewport` is the fast local reread, `delta` reports the changed local surface when a compatible prior base exists and falls back to `viewport` when it does not, and `full` keeps the exhaustive page-wide tree for deep inspection or regression work.
- Viewport locality: `viewport` and `delta` now demote unchanged sticky/fixed header or footer chrome when stronger local anchors are present. If persistent chrome still wins because nothing stronger exists, `scope.locality_fallback_reason` explains that fallback.
- Snapshot inline handles: rendered `[index=...]` markers follow the same revision scope as the exposed actionable `nodes` and only advertise follow-up-capable nodes in that returned scope; use them as reread-local hints, not as durable cross-revision IDs.
- `target_status = same`: the tool still proved the same target, even if the post-action handle downgraded to selector-only because actionability disappeared.
- `target_status = rebound`: the tool recovered after a revision change; `target_after` may downgrade to selector-only when the same element still exists but no longer has a verified actionable handle, so reread with `snapshot` before more precise chained work.
- `target_status = detached`: the old target no longer exists, often after navigation; reacquire state from the new page before continuing.
- `target_status = unknown`: post-action identity stayed ambiguous, usually because multiple matches remained or the selector could not prove the same element.
- Attach-mode recovery: if a connected session returns `code = attach_page_target_lost`, use `tab_list` to confirm inventory, `switch_tab` to reacquire an active page target, and reconnect the session if DOM-backed tools still fail.
- Attach-mode safety: use a disposable browser profile for debugging and treat destructive tab tools as explicit actions, especially on connected sessions.

Chromewright also carries a few small but important contract details:

- DOM-targeted tools take one public `target` object: `{ "kind": "selector", "selector": "..." }` or `{ "kind": "cursor", "cursor": ... }`.
- Canonical target examples:
  - `inspect_node`: `{ "target": { "kind": "selector", "selector": "h1" } }`
  - `click`: `{ "target": { "kind": "cursor", "cursor": <snapshot cursor> } }`
- `screenshot` is part of the default surface and uses `mode` plus optional `scale` instead of legacy `full_page = true`; successful calls return managed artifact metadata including `artifact_uri`, `artifact_path`, `mime_type`, `byte_count`, image dimensions, CSS dimensions, DPR metadata, `revealed_from_offscreen`, and optional `clip`.
- `switch_tab` accepts stable `tab_id` only on the public MCP surface.
- Structured tool-local failures use one top-level family: `code`, `error`, optional `document`, optional `target`, optional `recovery`, and optional `details`.
- `extract` uses `code = element_not_found` for selector misses and reserves `code = invalid_extract_payload` for malformed extraction results.
- `read_links` returns both the raw `href` attribute and an absolute `resolved_url`.

## Default Tool Surface

The default Chromewright MCP server exposes 21 high-level tools:

- navigation: `navigate`, `go_back`, `go_forward`, `wait`
- interaction: `click`, `input`, `select`, `hover`, `press_key`, `scroll`
- tabs and lifecycle: `new_tab`, `tab_list`, `switch_tab`, `close_tab`, `close`
- reading and inspection: `snapshot`, `inspect_node`, `get_markdown`, `extract`, `read_links`
- managed artifacts: `screenshot`

The default surface intentionally excludes the raw-JavaScript operator tool `evaluate`.

High-level action tools return compact follow-up metadata by default. Use `snapshot` when you need the scoped YAML snapshot plus actionable-node list, with `viewport` as the default, `delta` for session-local changes, and `full` for exhaustive rereads. For targeted reads, use `snapshot` to choose a node and reuse its `cursor`, then call `inspect_node`; when you need to inspect a non-actionable DOM node such as a heading or image, `inspect_node` also accepts selector-based reads with an optional `cursor`, and stale cursor replay may selector-rebound before the final `target` settles. After revision-changing actions, rerun `snapshot` before more precise target reuse. Public DOM follow-up calls should use `target.kind = "cursor"` whenever a fresh cursor is available and fall back to `target.kind = "selector"` when only selector continuity remains.

Use `screenshot` when you need a bounded visual artifact from the browser. The public contract is `mode` plus optional `scale`, `tab_id`, `target`, and `region`; callers do not provide `path`, `full_page`, or `confirm_unsafe`. Successful results include `artifact_uri`, `artifact_path`, `mime_type`, `byte_count`, `width`, `height`, `css_width`, `css_height`, `device_pixel_ratio`, `pixel_scale`, `revealed_from_offscreen`, and optional `clip`.

Read-oriented tools are intentionally distinct: `get_markdown` is the broad reading tool, `extract` is for targeted text or HTML, and `read_links` is for link inventory and planning. For multi-tab work, prefer stable `tab_id` handles from `tab_list`, `new_tab`, `switch_tab`, and `close_tab` instead of relying only on tab indices.

## Operation Metrics

Finished tool results include `operation_metrics` metadata. Every finished result includes serialized output size, and measured hot paths add the relevant counters below:

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
- The default tool surface excludes raw JavaScript evaluation; `screenshot` remains part of the bounded default surface and returns managed artifact metadata.
- `screenshot` does not accept caller-chosen output paths or `confirm_unsafe`; use `mode = "full_page"` instead of a legacy `full_page = true` flag, and use `scale = "css"` only when you want CSS-pixel-normalized output instead of raw device pixels.
- `navigate` and `new_tab` reject unsafe schemes such as `data:` and `file:` unless the caller passes `allow_unsafe = true`.
- `cursor` and `node_ref` targets are revision-scoped. After a DOM-changing action, stale `cursor` replay may selector-rebound, but precise follow-up work should still be refreshed from a new `snapshot`.

## License

MIT
