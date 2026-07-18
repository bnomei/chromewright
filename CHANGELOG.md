# Changelog

## Unreleased

### Added

- Add the default-enabled semantic terminal browser with Vimari navigation,
  Vim-style search, configurable TOML bindings, bounded paste input, managed
  headless Chrome sessions, and exact fail-closed id-less element interaction.
- Add the co-hosted loopback MCP companion with the complete `tui_*` tool
  family and bounded active/revisioned semantic resources.
- Add TUI soft word-wrap toggled with `zw` (`toggle_wrap`), off by default; wrap reflows content lines to the viewport and disables horizontal pan while active.
- Add terminal-native TUI color theme for headings, links, landmarks, controls, chrome lifecycle, and selection overlays.
- Move TUI selection to in-page fragment targets (`#id`) after capture, including named anchors and `#top`.
- Add prose (default) vs structure content projection toggled with `zs`; prose hides landmark/list/group chrome, flattens indent, disables collapse, and rebinds hidden container selection to the first visible descendant.

### Changed

- Drop the redundant `tui_query` companion alias; use `tui_render` for semantic markdown.
- Make the TUI inspect panel a compact CSS-selector-first developer view (identity, action fields, ref/rev) without Debug/`None` noise.
- Show the full DOM path (`main > form#x > input#y`) as the TUI inspect panel title instead of a static `inspect` label.
- Document the full default TUI keymap, action names, and TOML overlay syntax in `README.md`.
- Upgrade the MCP runtime to RMCP 2.2 and retain only the server, stdio, and streamable-HTTP transport features.
- Refresh Rust 1.88-compatible dependencies, including `headless_chrome` 1.0.22.
- Show TUI search in the footer like Vim: `/{buffer}` while typing and `/{pattern}  n/m` while a search remains active (not in the header URL bar).
- Show link-hint mode (`f` / `F` + typed keys) in the footer cmdline instead of the header.
- Replace header lifecycle words (`ready`/`load`/`err`) with single glyphs (`●`/`◐`/`✕`); detail stays in the footer.
- Multi-field TUI forms: Tab stashes field values; Enter writes all staged fields then submits (`requestSubmit`) and recaptures the resulting page like Chrome.
- Enter on a selected text input starts form edit mode (header shows `IN …`); type freely, then Enter again to submit.
- Render form controls inline with live values (staged Tab edits + active `IN` buffer with cursor); checkbox/radio toggle on Enter, select cycles options.

### Fixed

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
- Collapse TUI chrome to a single browser-like header bar (history, location, title, lifecycle glyph) using color only; omit wrap/structure descriptors from the header.
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
