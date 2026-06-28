# Changelog

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
