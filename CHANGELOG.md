# Changelog

## Unreleased

### Added

- Add repeatable `--url <URL>` startup seeding. Each safe URL opens in its own
  managed tab; the final seeded tab is active.

## 0.8.0 - 2026-07-19

### Added

- Add the default-enabled semantic terminal browser, with Vimari-style navigation,
  Vim search, configurable TOML bindings, bounded paste input, and a managed
  private headless Chrome session mode.
- Add the co-hosted loopback MCP companion, its complete `tui_*` tool family,
  and bounded active and revisioned semantic-document resources; standard
  stdio and `serve` sessions remain tools-only.
- Add TUI prose and structure projections, soft wrapping, themes and layout
  overrides, URL-history completion, fragment selection, form interaction, and
  external semantic-Markdown editing.

### Changed

- Enable the TUI in the default Cargo feature set while retaining isolated
  `mcp-server` and `tui` feature builds.
- Keep terminal rendering responsive during browser work and coordinate terminal
  and companion mutations through one transactional lifecycle.
- Upgrade to RMCP 2.2 and refresh the Rust 1.88-compatible dependency set,
  including `headless_chrome` 1.0.22.

### Fixed

- Publish truncated large-page semantic captures instead of failing navigation, and
  omit effectively hidden DOM nodes so live page filters update the rendered list.
- Settle navigation, reload, and form edits against the quiet main-frame revision
  before recapturing the semantic document.
- Recover safely from stale link targets, empty tab sets, failed page actions,
  and companion shutdown or transaction-abandonment races.
- Allow safe `about:` navigation, including `about:blank`, without
  `allow_unsafe`.
- Keep TUI logging off stderr by default; `CHROMEWRIGHT_LOG` now opts into a
  file target without corrupting the alternate screen.

## 0.7.1 - 2026-06-28

### Changed

- Keep public history helpers safe by default; unsafe history destinations still require an explicit per-tool `allow_unsafe` opt-in.
- Align synthetic text input with native typing semantics by dispatching `input` without an immediate synthetic `change`.

### Fixed

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
