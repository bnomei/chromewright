# Changelog

## Unreleased

### Added

- Add the default-enabled semantic terminal browser with Vimari navigation,
  Vim-style search, configurable TOML bindings, bounded paste input, managed
  headless Chrome sessions, and exact fail-closed id-less element interaction.
- Add the co-hosted loopback MCP companion with the complete `tui_*` tool
  family and bounded active/revisioned semantic resources.
- Add TUI soft word-wrap toggled with `zw` (`toggle_wrap`), on by default; wrap reflows content lines to the viewport and disables horizontal pan while active.
- Add terminal-native TUI color theme for headings, links, landmarks, controls, chrome lifecycle, and selection overlays.
- Move TUI selection to in-page fragment targets (`#id`) after capture, including named anchors and `#top`.
- Add prose (default) vs structure content projection toggled with `zs`; prose hides landmark/list/group chrome, flattens indent, disables collapse, and rebinds hidden container selection to the first visible descendant.

### Added

- URL bar Tab autocomplete from local TUI history (URLs successfully opened): ghost suffix while typing, Tab accepts/cycles, Shift-Tab cycles backward. Stored under `$XDG_DATA_HOME/chromewright/url_history` (not Chrome profile omnibox data).
- Configurable TUI content-pane padding via `[layout]` in `tui.toml` (`content_padding_x` / `content_padding_y`, default 1 col left/right and 0 row top/bottom). Header and footer stay full width; wrap/selection viewport uses the padded inner area.

### Changed

- Run terminal-owned blocking browser actions on a worker while the normal Ratatui loop remains the sole terminal writer, so Loading spinners and resize redraws continue during page work.
- Make companion synchronization consume one coherent shared-state snapshot, make terminal
  selection changes write through immediately, and centralize tool browser-effect classification
  in the registry, preventing torn lifecycle/document/selection reads.
- Enter on a text field (and Tab-away / blur) writes the value into the live DOM and same-document patches so live-search/filter UIs work without a submit button. Typing stays local until complete; submit buttons still send full multi-field forms.
- Form control selection uses reverse only on the field text (no full-width pad or extra edit spaces that left a long bar / empty non-bg cell).
- TUI content theme uses a clearer ANSI-16 role ladder (H1–H6, blue links, light-cyan forms, yellow hints, muted gray images) inspired by md-tui’s role idea, not a 1:1 color match; optional `[theme]` keys in `tui.toml` override individual roles.
- Underline only the URL inside markdown-style link lines (`[label](url)`), not the whole link row.
- TUI `f`/`F` hints cover form controls as well as links (text inputs open edit mode); multi-line/wrapped targets show a single label on the first row only.
- Color form controls light cyan so they stay distinct from yellow `f`/`F` hint labels.
- Default TUI soft word-wrap to on (`zw` still toggles; off disables wrap and restores horizontal pan).
- Show the active tab ordinal in the TUI header as `2/5` immediately left of the history arrows.
- Align TUI muscle memory with [md-tui](https://github.com/henriklovhaug/md-tui) where safe: `s`/`S` link hints (aliases of `f`/`F`), arrow keys for selection and half-page pan, `b` history back; keep browser-first `f`/`h`/`l`/`t`/`o`. Keymap actions may bind multiple sequences; TOML overlay still replaces the whole action.
- Drop the redundant `tui_query` companion alias; use `tui_render` for semantic markdown.
- Make the TUI inspect panel a compact CSS-selector-first developer view (identity, action fields, ref/rev) without Debug/`None` noise.
- Show the full DOM path (`main > form#x > input#y`) as the TUI inspect panel title instead of a static `inspect` label.
- Document the full default TUI keymap, action names, and TOML overlay syntax in `README.md`.
- Upgrade the MCP runtime to RMCP 2.2 and retain only the server, stdio, and streamable-HTTP transport features.
- Refresh Rust 1.88-compatible dependencies, including `headless_chrome` 1.0.22.
- Show TUI search in the footer like Vim: `/{buffer}` while typing and `/{pattern}  n/m` while a search remains active (not in the header URL bar).
- Show link-hint mode (`f` / `F` + typed keys) in the footer cmdline instead of the header.
- Replace header lifecycle words (`ready`/`load`/`err`) with single glyphs (`●`/`◐`/`✕`); detail stays in the footer.
- Multi-field TUI forms: Tab/Enter on a text field stages values only; **Enter on a submit button** writes staged fields, clicks submit, and recaptures (forms without a submit control are unsupported).
- Enter on a selected text input starts form edit mode (header shows `IN …`); Enter again commits the field without sending the form.
- Render form controls inline with live values (staged Tab edits + active `IN` buffer with cursor); checkbox/radio toggle on Enter, select cycles options.

### Added

- TUI `e` (`edit_external`) opens the current page’s semantic markdown in `$VISUAL` / `$EDITOR` / `vi` (Nereid-style suspend; read-only for now).
- Loading header glyph spins through `◐◓◑◒` every 250 ms (background paint while CDP work blocks; full redraw while companion Loading).
- Amp-style right-edge content scrollbar: solid block track/thumb (no line glyphs), outside the padded/capped markdown column.

### Changed

- TUI browser tabs: `[` previous / `]` next; `q` quits (Ctrl-c still works).
- TUI content defaults to a centered `content_max_width = 100` column (`[layout]` in `tui.toml`; `0` disables). Press `w` (`toggle_full_width`) to switch full width ↔ capped.

### Fixed

- Make terminal and companion page-action completion ticket-transactional, preserve typed
  `tui_refresh` failures, gate companion claims atomically with shutdown, and clear shared
  state successfully when a companion closes the final tab.
- Large-page semantic capture (e.g. duckduckgo.com) no longer fails navigation with `resource-semantic_capture`; truncated trees are published with status `capture truncated (page exceeded semantic bounds)`.
- Esc in Normal mode clears sticky TUI `/search` footer (`/{query}  n/m`); Esc while typing `/…` still only cancels the prompt and keeps the prior pattern for `n`/`N`.
- Semantic capture skips effectively hidden nodes (`display:none`, `visibility:hidden`, `[hidden]`, `aria-hidden`) so client-side filters like Holmes on builtwithkirby.com update the markdown list after search, not only the input value/URL.
- TUI settle after navigate/reload/field apply waits for navigation completion and a quiet main-frame revision so the semantic body matches the updated URL (was capturing stale DOM while the address bar already moved).
- Form edit (`IN`) caret uses a reverse-filled ASCII space instead of U+2588 full block, and the hardware cursor stays hidden after each draw, so the value field no longer shows a hollow empty cell next to `]`.
- Same-document button activates (e.g. clipboard copy on docs sites) recapture DOM updates (label flips) while pinning the topmost visible content line so the markdown view does not jump; selection is rebound without `ensure_visible`. Same-page `#fragment` links still jump to the target (detected from link `href` / post-click URL).
- When no tabs are open, submitting a URL (`o` then Enter) opens a new tab at that address instead of failing to navigate a missing page; `o` remains available in Error for empty-session recovery.
- Make TUI `y` copy the resolved URL for links and images instead of the full rendered element.
- Allow `about:blank` (and other `about:` URLs) in navigation validation so TUI `t` / `new_tab` can open a blank tab without `allow_unsafe`.
- Make agent attention visible in prose mode: paint the whole subtree, scroll to the first visible descendant, status feedback, and a magenta background spotlight (fg-only was invisible on cyan headings).

- Dismiss TUI Error lifecycle with Escape so a failed history/navigation action no longer leaves the terminal browser unresponsive; keep the retained page and log the failure when `CHROMEWRIGHT_LOG` is set.
- Fall back to navigating a link's captured href when the live DOM click locator is stale/missing, and retry post-capture metadata matching a few times on dynamic pages.
- Clear the TUI when the last browser tab is closed and keep `t` (new tab) available from Error so an empty session can recover.
- Fold `data:` / base64 image (and link) URLs in TUI content to short placeholders like `base64,…`.
- Exit link-hint mode after a successful current-tab follow (`f`); new-tab hints (`F`) still chain until Esc.
- Anchor the inspect panel under the selected block and refresh it as selection moves until Esc.
- Add prose-only heading spacing (2 lines above h1/h2, 1 above h3+, 2 after h1 unless h2 follows); spacer rows are non-selectable and omitted in structure mode.
- Collapse TUI chrome to a single browser-like header bar (tab ordinal, history, location, title, lifecycle glyph) using color only; omit wrap/structure descriptors from the header.
- Add `Ctrl-d` / `Ctrl-u` to move selection by half a page (`d`/`u` remain view-only pan).
- Add `O` (`edit_url`) to open the location bar prefilled with the current URL for editing; `o` still starts empty.

## 0.7.1 - 2026-06-28

### Changed

- Keep public history helpers safe by default; unsafe history destinations still require an explicit per-tool `allow_unsafe` opt-in.
- Align synthetic text input with native typing semantics by dispatching `input` without an immediate synthetic `change`.

### Fixed

- Keep `chromewright tui` free of stderr log spill by deferring logger install until the transport is known: TUI defaults to a quiet logger, optional `CHROMEWRIGHT_LOG` file target, while stdio and HTTP `serve` keep stderr logging.
- Reveal and capture same-origin iframe screenshot targets using frame-aware viewport geometry.
- Invalidate scroll snapshot cache state before parsing scroll results so malformed payloads cannot preserve stale delta bases.
- Quote YAML `~` scalar values so snapshot text round-trips as a string instead of YAML null.
- Preserve missing-value `evaluate` descriptions in test coverage.

## 0.7.0 - 2026-06-28

### Added

- Return richer `evaluate` metadata so callers can distinguish missing CDP values from real JSON `null` results.
- Add shared target-resolution metadata and viewport result contracts across tool outputs.

### Changed

- Apply the unsafe-scheme gate to `go_back` and `go_forward`, reverting rejected history moves unless `allow_unsafe` is set on that request.
- Preserve explicit CSS selector targets during actionability polling, interaction execution, inspection, waits, and element screenshot reveal, while retaining index reconciliation for cursor/index/node_ref targets.
- Key markdown and snapshot caches on live document identity details to avoid stale reads after URL, scroll, or viewport changes.

### Fixed

- Avoid stale delta snapshots after scroll operations.
- Return `set_viewport` results for the targeted tab instead of the previously active tab.
- Reveal offscreen screenshot targets through the shared browser kernel so target indexes and iframe scopes stay consistent.
- Prevent automatic attach-session recovery from replaying non-idempotent page mutations.

## 0.6.0 - 2026-06-17

### Added

- Expose operator tools in the normal production MCP surface.
- Add CI quality gates for rustfmt, clippy, MSRV, cargo check, tests, and package verification.
- Add a local browser smoke workflow via `scripts/browser-smoke.sh`.

### Changed

- Set practical screenshot viewport caps and document the large-canvas override.
- Clarify that operation metrics output byte counts are optional.
- Update repository identity references to `bnomei/chromewright`.

### Fixed

- Percent-encode screenshot artifact file URLs.
- Stabilize browser smoke tab workflow assertions against runtime tab IDs.

## 0.5.1 - 2026-06-10

### Fixed

- Advertise `wait` tool parameters as a flat object schema without top-level schema composition, while preserving strict runtime validation.
- Ensure registered tool input schemas avoid top-level `oneOf`, `anyOf`, `allOf`, `enum`, and `not` keywords for broader client compatibility.

## 0.5.0 - 2026-06-01

### Security

- Fail closed for stale cursor mutations while preserving read-only `inspect_node` selector rebinding.
- Add bounded wait, screenshot, DOM extraction, reader, markdown, and inspection resource limits.
- Harden screenshot artifact storage with private per-session directories and exclusive artifact file creation.

### Changed

- Add `inspect_node.truncated_fields` metadata when compact fields are shortened by inspection output limits.
