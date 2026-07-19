# chromewright

[![Crates.io Version](https://img.shields.io/crates/v/chromewright)](https://crates.io/crates/chromewright)
[![Build Status](https://github.com/bnomei/chromewright/actions/workflows/ci.yml/badge.svg)](https://github.com/bnomei/chromewright/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

Chromewright is a local-first browser automation MCP server built on Chrome DevTools Protocol (CDP). It exposes a real Chrome or Chromium browser to MCP clients over stdio or loopback streamable HTTP, with high-level tools for navigation, page reading, tab management, screenshots, viewport emulation, and bounded interaction.

Use Chromewright when an agent needs browser state from a real browser without embedding a Node.js automation stack or writing raw CDP calls. Chromewright is not an end-to-end test runner; it is a browser control layer for AI agents and MCP clients.

## When to use Chromewright

- Attach an MCP client to an existing Chrome or Chromium profile on a DevTools endpoint.
- Launch a dedicated local browser session for agent work.
- Read pages through snapshots, markdown extraction, targeted inspection, and link inventory.
- Drive bounded interactions such as click, input, select, hover, key press, scroll, and wait.
- Capture managed PNG screenshots without letting callers choose arbitrary output paths.
- Reuse revision-scoped `cursor` handles from snapshots instead of relying only on CSS selectors.

## Installation

Chromewright requires Rust 1.88 or newer when you install or build it with Cargo.

Install from crates.io:

```bash
cargo install chromewright
```

Install with Homebrew:

```bash
brew install bnomei/chromewright/chromewright
```

Install from source:

```bash
git clone https://github.com/bnomei/chromewright.git
cd chromewright
cargo install --path .
```

You can also download prebuilt archives from GitHub Releases and place the `chromewright` binary on your `PATH`.

Verify the binary is available:

```bash
chromewright --version
```

Expected output:

```txt
chromewright <version>
```

## Quickstart

This path starts a visible Chrome profile with DevTools enabled, then serves Chromewright over loopback HTTP at `http://127.0.0.1:3000/mcp`.

### Prerequisites

- `chromewright` on your `PATH`
- Chrome, Chromium, or another CDP-compatible browser
- An MCP client that supports streamable HTTP or stdio servers

### 1. Start a dedicated browser profile

On macOS, run:

```bash
open -na "Google Chrome" --args \
  --remote-debugging-port=9222 \
  --user-data-dir="$HOME/.chromewright-agent-profile"
```

Use a dedicated profile when you do not want agent automation attached to your personal browser session. The default Chromewright attach mode expects DevTools at `http://127.0.0.1:9222`.

### 2. Start Chromewright

Run the streamable HTTP server:

```bash
chromewright serve
```

Expected log line:

```txt
Ready to accept MCP connections at http://127.0.0.1:3000/mcp
```

### 3. Connect an MCP client

For JSON-configured clients that support streamable HTTP:

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

For Codex over stdio, let the client start the server:

```toml
[mcp_servers.chromewright]
command = "/absolute/path/to/chromewright"
enabled = true
```

For Codex against the long-lived HTTP server from step 2:

```toml
[mcp_servers.chromewright]
url = "http://127.0.0.1:3000/mcp"
enabled = true
```

### 4. Verify with the client

Use your MCP client to call `tab_list`. A connected session should return at least one tab with a stable `tab_id`. If no active tab is useful, call `new_tab` before calling `snapshot`.

## Browser modes

Chromewright has two browser modes:

| Mode | How to start | What it does |
| --- | --- | --- |
| Attach | Run `chromewright` or `chromewright serve` with no launch flags. | Connects to `http://127.0.0.1:9222` by default. |
| Attach to another endpoint | Pass `--ws-endpoint <URL>`. | Connects to a browser WebSocket URL or a DevTools HTTP origin such as `http://127.0.0.1:9333`. |
| Launch | Pass any launch flag such as `--user-data-dir`, `--headless`, `--executable-path`, or `--debug-port`. | Starts a local browser session. Launch mode is headed unless you pass `--headless`. |

Examples:

```bash
# Default: attach to http://127.0.0.1:9222 and serve MCP over stdio.
chromewright

# Serve streamable HTTP on the default loopback endpoint.
chromewright serve

# Serve streamable HTTP on a custom port and path.
chromewright serve --port 3333 --http-path /browser

# Attach to a different DevTools endpoint.
chromewright --ws-endpoint http://127.0.0.1:9333

# Launch a visible browser with a dedicated profile.
chromewright --user-data-dir /tmp/chromewright-profile

# Launch a headless browser and serve streamable HTTP.
chromewright --headless --user-data-dir /tmp/chromewright-profile serve
```

## CLI reference

| Option or command | Default | Description |
| --- | --- | --- |
| `chromewright` | stdio transport | Starts the MCP server over stdio. |
| `chromewright serve` | `127.0.0.1:3000/mcp` | Starts the MCP server over loopback streamable HTTP. |
| `serve --port <PORT>`, `serve -p <PORT>` | `3000` | Sets the HTTP port. |
| `serve --http-path <PATH>` | `/mcp` | Sets the HTTP endpoint path. |
| `--ws-endpoint <URL>` | `http://127.0.0.1:9222` when no launch flags are present | Connects to an existing browser WebSocket URL or DevTools HTTP origin. This conflicts with launch flags. |
| `--headless` | `false` | Launches a new browser in headless mode. |
| `--executable-path <PATH>` | auto-detected by the browser backend | Uses a specific browser executable in launch mode. |
| `--user-data-dir <DIR>` | backend default | Uses a persistent browser profile directory in launch mode. |
| `--debug-port <PORT>` | auto-selected | Uses a specific DevTools port for a locally launched browser. |
| `chromewright tui` | (feature `tui`, default-on) | Starts the semantic terminal browser against the same browser session. Always co-hosts a loopback MCP companion. |
| `tui --config <PATH>` | `$XDG_CONFIG_HOME/chromewright/tui.toml` or `~/.config/chromewright/tui.toml` | TOML keymap overlay. An explicit path must exist and parse; a missing default file keeps built-in bindings. |
| `tui --companion-port <PORT>` | `0` (ephemeral) | Loopback port for the co-hosted streamable-HTTP MCP companion. |
| `tui --companion-path <PATH>` | `/mcp` | HTTP path for the co-hosted companion. |
| `--browser-session <reuse\|restart>` | `reuse` with `--headless tui` | Reuse or replace Chromewright's owned managed headless browser. Only valid with `--headless tui`. |

Source: [src/bin/mcp_server.rs](src/bin/mcp_server.rs).

## Terminal browser (TUI)

The default build includes a semantic terminal browser. It attaches to the same Chromewright browser session and renders semantic DOM content only (not pixels, CSS, or browser layout).

```bash
# Attach to an existing DevTools endpoint (default http://127.0.0.1:9222).
chromewright tui

# Managed private headless Chrome for the normal terminal-browser flow.
chromewright --headless tui
```

The header is a single browser-like bar: tab ordinal (`2/5`) left of the history arrows, then location/title, with a lifecycle glyph on the right. Keyboard bindings are not shown in the terminal chrome. Defaults are Vimari-compatible. Multi-key sequences such as `gg` and `gi` wait for the full chord; an unbound prefix is rejected rather than re-firing the last key.

### Default keymap

Browser-first (Vimari) defaults, with [md-tui](https://github.com/henriklovhaug/md-tui)-style aliases where they do not collide. Overlaying an action replaces **all** of its sequences (primary + aliases).

| Key | Action name | Behavior |
| --- | --- | --- |
| `f` / `s` | `link_hints_follow` | Enter link-hint mode; follow the chosen link in the current tab, then return to Normal. (`s` = md-tui select-link.) |
| `F` / `S` | `link_hints_new_tab` | Enter link-hint mode; open the chosen link in a new tab (hint mode stays open for chaining until Esc). (`S` = md-tui select-link alt.) |
| `j` / `↓` | `scroll_down` | Scroll or move selection down one block. |
| `k` / `↑` | `scroll_up` | Scroll or move selection up one block. |
| `h` | `scroll_left` | Horizontal scroll left when content overflows. (Not md-tui half-page; browser tables/code need pan.) |
| `l` | `scroll_right` | Horizontal scroll right when content overflows. |
| `u` / `→` | `half_page_up` | Pan the view half a page up (selection unchanged). |
| `d` / `←` | `half_page_down` | Pan the view half a page down (selection unchanged). |
| `Ctrl-u` | `page_select_up` | Move selection up by about half a page. |
| `Ctrl-d` | `page_select_down` | Move selection down by about half a page. |
| `gg` | `go_top` | Jump to the document top. (Single `g` is not bound so `gi` stays available.) |
| `G` | `go_bottom` | Jump to the document bottom. |
| `gi` | `focus_first_input` | Focus the first form control and start editing. |
| `H` / `b` | `history_back` | Browser history back. (`b` = md-tui back.) |
| `L` | `history_forward` | Browser history forward. |
| `r` | `reload` | Reload the active page. |
| `w` | `next_tab` | Switch to the next tab. |
| `q` | `prev_tab` | Switch to the previous tab. (Not quit; use `Ctrl-c`.) |
| `x` | `close_tab` | Close the current tab. |
| `t` | `new_tab` | Open a new tab. |
| `o` | `open_url` | Open the URL entry prompt with an empty buffer. |
| `O` | `edit_url` | Open the URL entry prompt prefilled with the current address (edit from the end). |
| `/` | `search` | Start forward search by exact semantic content. (md-tui also binds `f` to search; we keep `f` for link hints.) |
| `n` | `search_next` | Repeat the last search forward. |
| `N` | `search_previous` | Repeat the last search backward. |
| `Space` | `collapse` | Collapse or expand the selected block. |
| `zw` | `toggle_wrap` | Toggle soft word-wrap (on by default). When on, long lines wrap to the viewport and `h`/`l` horizontal pan is disabled. |
| `zs` | `toggle_structure` | Toggle prose (default) vs structure projection. Prose hides landmark/list/group chrome and flattens indent; structure shows DOM-like containers. Collapse (`Space`) only works in structure mode. |
| `i` | `inspect` | Open a compact inspect panel under the selected block; title is the full DOM path (`main > … > tag#id`), body has action fields and ref/rev; follows selection until Esc. |
| `y` | `copy_block` | Copy selection: link/image URL (resolved), otherwise rendered block text (OSC 52). |
| `Y` | `copy_ref` | Copy the opaque `semantic_ref` (OSC 52). |
| `Tab` | `tab_next` | Next focusable control; stashes the current form field value (multi-field). |
| `Shift-Tab` | `tab_prev` | Previous focusable control; stashes the current form field value. |
| `Enter` | `confirm` | Text input: start/finish editing (stages value; does not send). Checkbox/radio: toggle. Select: cycle options. **Submit button only**: write staged fields + click + recapture. Link/other: activate. |
| `Esc` | `escape` | Leave prompt, hint, or inspect mode. After a failed page action, dismiss Error back to Ready (retained page stays) so keys work again. |
| `Ctrl-c` | `quit` | Quit the TUI. |

Link hints use deterministic two-key labels from the alphabet `asdfgqwertzxcvb` (for example `aa`, `as`), assigned only to viewport-visible links.

After navigation or a link follow settles, a URL fragment such as `#section` moves the TUI selection to the matching component (`id`, then named anchor), expands collapsed ancestors, and scrolls it into view. Unmatched fragments keep the prior selection.

The content pane uses a terminal-native ANSI color theme (headings, links, landmarks, controls) with reverse-video selection applied last. Colors inherit the terminal light/dark theme.

Default reading mode is **prose** (markdown-like): no `▾ [main]` / `ol` / group chrome, fully flat lines. Press `zs` for **structure** (DOM-like outline). Wrap (`zw`) and structure (`zs`) are not shown in the header bar; toggle feedback appears in the status line.

Search follows Vim semantics: a new `/pattern` starts after the current selection and wraps at the end; `n` repeats forward, `N` repeats backward, and submitting an empty `/` prompt repeats the previous pattern. The footer shows the cmdline while typing (`/…`) and keeps `/{pattern}  n/m` while a search is active; link hints (`f` / `F`) also use the footer (`f as`) rather than the header. Bracketed paste is accepted only in URL, search, and form input modes and is bounded to 4096 characters.

### Custom keymap

Bindings are replaceable by action name. `--config PATH` takes precedence; if omitted, Chromewright reads `$XDG_CONFIG_HOME/chromewright/tui.toml`, falling back to `~/.config/chromewright/tui.toml`. A missing default file keeps the built-ins; an explicitly requested file must parse successfully.

```toml
# Only list actions you want to rebind. Unknown actions or conflicting
# sequences abort startup rather than partially applying the overlay.
[keymap]
reload = "ctrl-r"
quit = "ctrl-q"
tab_prev = "shift-tab"
```

Binding specs accept single keys (`r`, `space`, `esc`, `enter`, `tab`), multi-key letter sequences (`gg`, `gi`), and chords with `-`, `+`, or space separators (`ctrl-c`, `C-c`, `shift-tab`). Supported named keys include `esc`, `enter`, `tab`, `backtab` / `shift-tab`, `backspace`, arrow keys, `home`, `end`, `pageup` / `pgup`, `pagedown` / `pgdn`, `space`, and function keys such as `f1`.

More detail on managed headless sessions, logging, and the co-hosted `tui_*` companion lives in [docs/tui.md](docs/tui.md). Source of truth for defaults: [src/tui/keymap.rs](src/tui/keymap.rs) and [src/tui/action.rs](src/tui/action.rs).

## Tool workflow

A typical agent workflow is:

1. Call `tab_list` or `new_tab` to establish an active tab.
2. Call `snapshot` to read the current page and collect actionable nodes.
3. Prefer a fresh `cursor` from `snapshot` or `inspect_node` when targeting follow-up actions.
4. Use `inspect_node`, `get_markdown`, `extract`, or `read_links` for more focused reads.
5. Use `click`, `input`, `select`, `hover`, `press_key`, `scroll`, `wait`, or tab tools for bounded interaction.
6. Call `snapshot` again after navigation, DOM-changing actions, viewport changes, or ambiguous target recovery.

`snapshot` supports these modes:

| Mode | Use it when |
| --- | --- |
| `viewport` | You want the default local reread of the current visible scope. |
| `delta` | You want the changed local surface when a compatible prior snapshot base exists. |
| `full` | You need an exhaustive page-wide read. |

DOM-targeted tools accept a public `target` object:

```json
{
  "target": {
    "kind": "selector",
    "selector": "h1"
  }
}
```

or:

```json
{
  "target": {
    "kind": "cursor",
    "cursor": "<cursor from snapshot or inspect_node>"
  }
}
```

Selector strings are still accepted for compatibility by tools that use the public target type, but the object form is the canonical contract.

## Production MCP tool surface

Production MCP sessions register the default high-level tools plus the guarded operator tool `evaluate`.

| Category | Tools |
| --- | --- |
| Navigation | `navigate`, `go_back`, `go_forward`, `wait` |
| Interaction and viewport | `click`, `input`, `select`, `hover`, `press_key`, `scroll`, `set_viewport` |
| Tabs and lifecycle | `new_tab`, `tab_list`, `switch_tab`, `close_tab`, `close` |
| Reading and inspection | `snapshot`, `inspect_node`, `get_markdown`, `extract`, `read_links` |
| Managed artifacts | `screenshot` |
| Operator diagnostics | `evaluate` |

`evaluate` executes JavaScript in the active page and requires `confirm_unsafe = true` on each call. It is available for diagnostics and escape-hatch inspection when bounded tools cannot answer a page-specific question.

Source: [src/tools/core/mod.rs](src/tools/core/mod.rs) and [src/browser/session.rs](src/browser/session.rs).

## Screenshots and viewport emulation

Use `screenshot` when a caller needs a managed PNG artifact. The tool accepts:

- `mode`: `viewport`, `full_page`, `element`, or `region`
- `scale`: `device` or `css`
- optional `tab_id`
- optional `target` for `element` captures
- optional `region` for `region` captures

Successful calls return managed artifact metadata, including `artifact_uri`, `artifact_path`, `mime_type`, `byte_count`, image dimensions, CSS dimensions, device pixel ratio, pixel scale, `revealed_from_offscreen`, and optional clip data. Callers do not provide output paths.

Use `set_viewport` to emulate responsive breakpoints through CDP. Successful calls return `viewport_metrics_after`; later `snapshot` calls expose the live metrics under `scope.viewport`.

## Safety boundaries

- Attach mode can see the tabs, cookies, and authenticated state in the browser profile you connect to.
- Use a dedicated browser profile for agent work when you do not want automation attached to a personal browser session.
- `evaluate` requires `confirm_unsafe = true` because it runs arbitrary JavaScript in the active page.
- `navigate` and `new_tab` reject unsafe schemes such as `data:` and `file:` unless that request passes `allow_unsafe = true`.
- `go_back` and `go_forward` apply the same unsafe-scheme gate and revert rejected history moves.
- `close_tab` requires `confirm_destructive = true` before closing an unmanaged active tab in a connected session.
- `close` requires `confirm_destructive = true` before expanding connected-session cleanup from managed tabs to all tabs.
- `cursor` and `node_ref` targets are revision-scoped. After navigation or DOM-changing actions, refresh with `snapshot`.

## Operation metrics

Finished tool results include `operation_metrics` metadata when a tool records non-zero metrics. `operation_metrics.output_bytes` is optional; it appears only when a tool path measures the exact serialized output size.

Measured paths may include:

- browser evaluation count
- poll iterations
- DOM extraction count and extraction time
- last DOM node count
- snapshot render time
- handoff rebuild count and time
- exact serialized output size when measured

Run the focused operation metrics tests:

```bash
cargo test --locked --all-features operation_metrics
```

## Local development

Build from source:

```bash
cargo build
```

Run the normal test suite:

```bash
cargo test
```

Run the browser smoke suite from the repository root:

```bash
scripts/browser-smoke.sh
```

The smoke script runs:

```bash
cargo test --test browser_smoke -- --nocapture
```

Browser smoke checks launch a local browser and are intended for maintainer workstations. CI covers formatting, clippy, MSRV, cargo check, tests, and packaging without requiring a live browser attach target.

## Source anchors

- Package metadata and Rust version: [Cargo.toml](Cargo.toml)
- CLI flags and transports: [src/bin/mcp_server.rs](src/bin/mcp_server.rs)
- Tool registry: [src/tools/core/mod.rs](src/tools/core/mod.rs)
- MCP handler: [src/mcp/handler.rs](src/mcp/handler.rs)
- Public target contract: [src/contract/target.rs](src/contract/target.rs)
- Screenshot contract: [src/tools/screenshot.rs](src/tools/screenshot.rs)
- TUI overview (managed headless, companion, resources): [docs/tui.md](docs/tui.md)
- TUI default keymap and actions: [src/tui/keymap.rs](src/tui/keymap.rs), [src/tui/action.rs](src/tui/action.rs)
- Browser smoke script: [scripts/browser-smoke.sh](scripts/browser-smoke.sh)

## License

MIT
