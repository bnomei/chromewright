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
Vimari plus non-conflicting [md-tui](https://github.com/henriklovhaug/md-tui) aliases:
`f`/`F` (and `s`/`S`) hints for links and form controls (footer cmdline while active; one label per target), `j`/`k` or arrows for
selection, `h`/`l` horizontal pan, `u`/`d` or left/right arrows half-page view pan,
`Ctrl-u`/`Ctrl-d` selection by half page, `gg`/`G` document ends, `gi` first form field
(edit mode), `H`/`b`/`L` history, `r` reload, `w`/`q`/`x`/`t` tab actions, `o`/`O` URL
entry, `/` forward search, `n` next match, `N` previous match, Space collapse
(structure mode only), `zw` toggle soft word-wrap (on by default), `zs` toggle prose
vs structure projection (prose by default; Vim-style `z`-prefix view commands), `i`
inspection, `y` copy (link/image URL or block text), `Y` semantic-ref copy, Tab/Shift-Tab
focus (form fields stash typed values on Tab or Enter), Enter on a text field
only commits the value (does not send the form), Escape cancel (and dismiss a
failed page-action Error back to Ready while retaining the last good page), and
Ctrl-C quit. See the README default keymap table for the full action map.

Form controls render inline in the content pane. Text inputs show
`[input name: value]` (live staged/`IN` buffer with a `█` cursor while editing).
Checkboxes/radios show `☑`/`☐` and `●`/`○` and toggle on Enter. Selects show the
current option and cycle on Enter. **Sending a form requires Enter on an explicit
submit button** (`button`/`input type=submit`): staged fields are written, the
button is clicked, then the page settles and is recaptured. Forms without a
submit control are not supported.

Failed page actions (history, navigate, reload, tab changes, …) enter the
Error lifecycle, keep the last published page on screen, and block normal
keys until Escape dismisses the error. With `CHROMEWRIGHT_LOG` set, those
failures are also written at `error` level (for example
`tui page action 'history_back' failed: …`).

After a successful capture whose URL includes a fragment (`#section`), the TUI
moves selection to the matching component (`id` first, then named `<a name>`),
expands collapsed ancestors, and scrolls the target into view. Empty `#` and
unmatched `#top` jump to the document top. Missing targets keep the prior
selection and set status `fragment target not represented`.

Content uses a terminal-native ANSI-16 role palette: H1 light-blue, H2 green,
H3 magenta, H4 cyan, H5 yellow, H6 light-red; links blue; forms light-cyan;
hints yellow. Optional `[theme]` keys in `tui.toml` override roles (ANSI names,
`reset`, or `#rrggbb`). Selection is reverse+bold applied last. Soft wrap
(`zw`) is independent of theme.

**Prose vs structure** (`zs`): prose (default) hides landmark/list/group header
rows and uses fully flat indent so the pane reads closer to markdown. Structure
shows DOM-like chrome (`▾ [main]`, `▾ ol`, labeled groups) and depth indent.
Collapse (`Space`) is structure-only. Switching into prose rebinds selection to
the first visible descendant when the current ref has no prose line.

Search follows Vim semantics: a new `/pattern` starts after the current
selection and wraps at the end; `n` repeats forward, `N` repeats backward, and
submitting an empty `/` prompt repeats the previous pattern. The footer hosts the
search cmdline while typing (`/…`) and keeps `/{pattern}  n/m` while a search is
active (cleared when the pattern is empty / no prior search). Bracketed paste is
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

[theme]
# h2 = "yellow"
# form_control = "lightcyan"
```

Only the listed keymap actions and theme roles are replaced. Invalid or
conflicting entries prevent startup rather than silently changing terminal
behavior.

The TUI is part of the default binary but remains isolated behind the `tui`
Cargo feature. A server-only build can omit terminal code and dependencies with
`cargo build --no-default-features --features mcp-server`.

## Logging

Process logging is installed only after the CLI subcommand is known so server
stderr output cannot paint over the alternate-screen UI.

- Default: TUI logging is off (`LevelFilter::Off`). Startup `info!` lines and
  library `log` records are discarded.
- Optional file: set `CHROMEWRIGHT_LOG=/path/to/tui.log` to append env_logger
  output to that path (default filter `info`, overridable with `RUST_LOG`).
  If the file cannot be opened, logging stays off rather than writing to
  stderr.
- stdio MCP and `serve` are unchanged: env_logger still targets stderr.

Managed `--headless tui` Chrome already has its process stdout/stderr nulled
separately from this logger policy.

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

`tui_render`, `tui_refresh`, `tui_inspect`,
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
