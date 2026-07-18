# Semantic TUI

The terminal browser is enabled in the default build: `cargo run -- tui`.
It attaches to the same Chromewright browser session and renders semantic DOM
content only; it does not render pixels, CSS, or a browser layout.

## Managed headless browser

For the normal terminal-browser flow, launch a managed private headless
Chrome directly:

```bash
cargo run -- --headless tui
```

Chromewright creates a private runtime profile and records a narrow ownership
lease (PID, nonce, profile, and DevTools port). A later identical command
reconnects to the healthy Chromewright-owned browser when possible, or safely
replaces a stale one. It never terminates a browser unless that ownership is
proven.

Use a pinned managed port only when another local tool needs a stable endpoint:

```bash
cargo run -- --headless --debug-port 40001 tui
```

Request a fresh managed browser instead of reuse with:

```bash
cargo run -- --headless --browser-session restart tui
```

`--ws-endpoint http://127.0.0.1:9222 tui` is different: it is attach-only.
Chromewright neither restarts nor terminates that external browser. Do not use
`--user-data-dir` with managed `--headless tui`; its profile is deliberately
private to the managed session.

Keyboard bindings are deliberately not shown in the terminal. Navigation defaults follow
Vimari: `f`/`F` link hints, `j`/`k`/`h`/`l` scrolling, `u`/`d` half pages,
`gg`/`G` document ends, `gi` first form field, `H`/`L` history, `r` reload,
`w`/`q`/`x`/`t` tab actions, `o` URL entry, `/` forward search, `n` next
match, `N` previous match, Space collapse, `i`
inspection, `y` content copy, `Y` semantic-ref copy, Tab/Shift-Tab focus,
Enter confirm, Escape cancel, and Ctrl-C quit.

Search follows Vim semantics: a new `/pattern` starts after the current
selection and wraps at the end; `n` repeats forward, `N` repeats backward, and
submitting an empty `/` prompt repeats the previous pattern. Bracketed paste is
accepted only in URL, search, and form input modes and is bounded to 4096
characters.

Bindings are replaceable by action name. `--config PATH` takes precedence; if
omitted, Chromewright reads `$XDG_CONFIG_HOME/chromewright/tui.toml`, falling
back to `~/.config/chromewright/tui.toml`. A missing default file keeps the
built-ins; an explicitly requested file must parse successfully.

```toml
[keymap]
reload = "ctrl-r"
quit = "ctrl-q"
tab_prev = "shift-tab"
```

Only the listed actions are replaced. Invalid or conflicting bindings prevent
startup rather than silently changing terminal behavior.

The TUI is part of the default binary but remains isolated behind the `tui`
Cargo feature. A server-only build can omit terminal code and dependencies with
`cargo build --no-default-features --features mcp-server`.

## Co-hosted loopback MCP companion

`chromewright tui` co-hosts a streamable-HTTP MCP endpoint on loopback only:

```bash
cargo run -- tui --companion-port 0 --companion-path /mcp
```

Port `0` binds an ephemeral loopback port. The companion shares the TUI's one
in-process `BrowserSession`, semantic document, selection, agent attention,
and page lifecycle. Standard stdio `serve` mode remains separate and never
registers `tui_*` tools or semantic resources, even though the default binary
includes TUI support.

Page-mutating companion tools (`navigate`, `click`, `switch_tab`, …) and
`tui_refresh` acquire the same Loading lifecycle lock as terminal navigation:
Loading → settle → fresh semantic capture → atomic Ready, or Error while
retaining the last valid document. Concurrent mutations fail explicitly.

### `tui_*` tools

`tui_render`, `tui_refresh`, `tui_inspect`, `tui_query`,
`tui_selection_read`, `tui_selection_update`,
`tui_attention_read`, `tui_attention_set`, `tui_attention_clear`.

Human selection and agent attention are independent. Attention is set with an
exact `semantic_ref` (plus optional bounded message ≤ 512 characters) against
the active document/revision. Invalid, stale, wrong-document, unknown, or
evicted refs fail closed. Setting attention scrolls the TUI to reveal the
spotlight without changing human selection or interacting with Chrome.

### Semantic resources

Bounded, non-mutating catalog (companion only):

- `chromewright://active/semantic.md` — dynamic alias of the last complete capture
- `chromewright://page/{document_id}/{revision}/semantic.md` — immutable Markdown
- `…/outline.md`, `…/semantic.json`, `…/debug.json`, `…/component/{semantic_ref}.md`
- `chromewright://tui/selection.json`, `chromewright://tui/attention.json`

`offset`/`limit` pagination applies only to semantic Markdown URIs. Pagination
query parameters on outline, JSON, debug, component, selection, or attention
are rejected. JSON resources use bounded renderers that return valid JSON or
fail explicitly; they are never mid-object truncated. Unavailable revisions
never fall through to the active document.
