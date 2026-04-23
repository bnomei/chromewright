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
- the normal high-level tool surface reads and interacts through CDP only; it does not write to local files during ordinary MCP use
- filesystem-bound screenshots are excluded from the default surface and remain explicit operator actions

## Use Cases

### Standard MCP browser automation

Once Chromewright is running, the normal workflow is:

1. Use `new_tab` or `tab_list` to establish an active tab. On a fresh session with no active tab, do not call `snapshot` first.
2. Use `snapshot` to get document metadata plus actionable nodes. Its inline `[index=...]` markers only appear for nodes that still expose a public follow-up handle in that revision; they mirror the exposed `nodes` list rather than forming a separate durable handle family.
3. Use `inspect_node` for targeted bounded reads, including selector-based inspection of non-actionable nodes such as headings, images, and overlays. Prefer `cursor` when one is available; otherwise a successful inspection may legitimately return `cursor = null` and a selector-only `target`.
4. Use `click`, `input`, `select`, `hover`, `press_key`, `scroll`, `wait`, or the tab tools with `cursor` preferred for follow-up targeting inside a page and stable `tab_id` preferred for multi-tab flows.
5. Refresh `snapshot` after revision-changing actions. `cursor` and `node_ref` are revision-scoped, so rereads are the normal recovery path.

## Workflow Conventions

- Fresh sessions: use `new_tab` or `tab_list` before `snapshot` if you do not already have an active tab.
- Revision-scoped targets: `cursor` and `node_ref` belong to a specific document revision. After navigation or DOM-changing actions, rerun `snapshot` or fall back to selector-based recovery before precise follow-up work.
- Snapshot inline handles: rendered `[index=...]` markers follow the same revision scope as the exposed actionable `nodes` and only advertise follow-up-capable nodes; use them as reread-local hints, not as durable cross-revision IDs.
- `target_status = same`: the tool still proved the same target, even if the post-action handle downgraded to selector-only because actionability disappeared.
- `target_status = rebound`: the tool recovered after a revision change; `target_after` may downgrade to selector-only when the same element still exists but no longer has a verified actionable handle, so reread with `snapshot` before more precise chained work.
- `target_status = detached`: the old target no longer exists, often after navigation; reacquire state from the new page before continuing.
- `target_status = unknown`: post-action identity stayed ambiguous, usually because multiple matches remained or the selector could not prove the same element.
- Attach-mode recovery: if a connected session returns `code = attach_page_target_lost`, use `tab_list` to confirm inventory, `switch_tab` to reacquire an active page target, and reconnect the session if DOM-backed tools still fail.
- Attach-mode safety: use a disposable browser profile for debugging and treat destructive tab tools as explicit actions, especially on connected sessions.

Chromewright also carries a few small but important contract details:

- `input` accepts `value` as a backward-compatible alias, while `text` remains the canonical field.
- `extract` uses `code = element_not_found` for selector misses and reserves `code = invalid_extract_payload` for malformed extraction results.
- `read_links` returns both the raw `href` attribute and an absolute `resolved_url`.

Companion references:

- [Tool Handoff Contract](docs/tool-handoff-contract.md)
- [Tool Description Index](docs/tool-description-index.md)

## Default Tool Surface

The default Chromewright MCP server exposes the same 20 high-level tools:

- navigation: `navigate`, `go_back`, `go_forward`, `wait`
- interaction: `click`, `input`, `select`, `hover`, `press_key`, `scroll`
- tabs and lifecycle: `new_tab`, `tab_list`, `switch_tab`, `close_tab`, `close`
- reading and inspection: `snapshot`, `inspect_node`, `get_markdown`, `extract`, `read_links`

The default surface intentionally excludes the operator tools `evaluate` and `screenshot`.

High-level action tools return compact follow-up metadata by default. Use `snapshot` when you need the full YAML snapshot plus actionable-node list. For targeted reads, use `snapshot` to choose a node and reuse its `cursor`, then call `inspect_node`; when you need to inspect a non-actionable DOM node such as a heading or image, `inspect_node` also accepts selector-based reads with an optional `cursor`, and successful reads reconcile around the probed element even when the final `target` is selector-only. After revision-changing actions, rerun `snapshot` before more precise target reuse. Targetable tools still accept `selector`, `index`, and `node_ref`, but `cursor` is the preferred follow-up handle.

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
- The default tool surface excludes raw JavaScript evaluation and filesystem-bound screenshots.
- Once operator tools are registered, `evaluate` and `screenshot` still require `confirm_unsafe = true` per call.
- `navigate` and `new_tab` reject unsafe schemes such as `data:` and `file:` unless the caller passes `allow_unsafe = true`.
- `cursor` and `node_ref` targets are revision-scoped. After a DOM-changing action, stale targets fail cleanly and should be refreshed from a new `snapshot`.

## License

MIT
