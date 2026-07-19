# Chromewright Agent Guide

Chromewright is a bounded, local-first browser-control MCP server over CDP. Its supported surface is the `chromewright` binary and MCP contract; the public Rust API is not stable.

## Workflow

- Edit the owning layer: CLI (`src/bin`), session/CDP (`src/browser`), shared contracts (`src/contract`), tools (`src/tools`), MCP (`src/mcp`), or TUI/semantic companion (`src/tui`, `src/semantic`). Read nearby tests first.
- Make the smallest behaviorally complete change. Reuse shared contracts, target resolution, browser-kernel logic, services, and `ToolContext` metrics.
- Add focused public-behavior tests, including negative/stale paths. Prefer `FakeSessionBackend` when Chrome is unnecessary.
- Update `README.md` or tracked focused docs when user behavior changes, and `CHANGELOG.md` → `Unreleased` for release-visible work.
- Preserve unrelated work. Do not force-add ignored docs, specs, or tool state unless explicitly required.

## Contracts and safety

- Treat tool names, schemas, serialized fields, defaults, errors, and safety behavior as compatibility-sensitive. Registered input schemas must avoid top-level `oneOf`, `anyOf`, `allOf`, `enum`, and `not`.
- Prefer bounded typed tools over raw JavaScript. Enforce limits at the owning boundary and preserve explicit truncation signals.
- Keep runtime confirmation and URL gates; MCP annotations are hints only. `evaluate` requires `confirm_unsafe = true` on every call.
- Launch sessions own their process, but supplied profiles may persist. Attach sessions may expose existing tabs, cookies, and authenticated state; destructive access to unmanaged tabs requires opt-in.
- Treat cursors, node refs, and semantic refs as revision-scoped. Mutations and revisioned resources fail closed on stale targets; never fall through to the active document.
- Invalidate DOM/snapshot caches at established mutation and navigation seams. Preserve markdown cache keying by document ID, revision, and URL.
- Standard stdio/`serve` sessions are tools-only; `tui_*` tools and semantic resources are companion-only. Serialize companion mutations through its shared lifecycle and retain the last valid document on failure.
- Never replay non-idempotent actions during recovery or expose caller-chosen screenshot paths.

## Rust and verification

- Support Rust 1.88 and edition 2024. Keep default `mcp-server` + `tui` and both isolated feature builds coherent.
- Run the narrowest relevant test first, then applicable jobs from `.github/workflows/ci.yml`. Use `scripts/browser-smoke.sh` only when browser-level confidence is needed; never turn product failures into skips.
- Do not weaken gates or limits, add sleeps instead of readiness polling, panic on caller/browser input, duplicate public contract types, or change versions, dependencies, generated/vendor files, or packaging unless required.
