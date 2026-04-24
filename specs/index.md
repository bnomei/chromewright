# Specs Index

- `strict-tool-inputs-and-defaults`
  - Status: active
  - Depends on: none
  - Goal: replace permissive public tool inputs with strict schema-expressed contracts and explicit defaults.

- `tool-output-contract-normalization`
  - Status: active
  - Depends on: `strict-tool-inputs-and-defaults`
  - Goal: normalize public success and structured error payloads across the non-snapshot tool surface.

- `browser-screenshot-surface`
  - Status: planned
  - Depends on: none
  - Goal: replace the path-based operator screenshot with one managed high-level screenshot tool for viewport, full-page, element, and region capture.

- `mcp-structured-target-contract`
  - Status: planned
  - Depends on: `strict-tool-inputs-and-defaults`
  - Goal: keep the live MCP `target` contract object-typed and canonical across schema, descriptions, and docs.

- `screenshot-debug-capture-polish`
  - Status: planned
  - Depends on: `browser-screenshot-surface`
  - Goal: make screenshot pixel-density semantics explicit and prevent misleading offscreen element captures in production debugging work.

- `snapshot-viewport-locality-tuning`
  - Status: planned
  - Depends on: none
  - Goal: bias viewport and delta snapshots toward the local component work area instead of persistent sticky chrome.

- `set-viewport`
  - Status: active
  - Depends on: none
  - Goal: add a runtime `set_viewport` tool backed by CDP emulation and expose current viewport metrics on the canonical reread surface.

- `headless-chrome-cdp-contract-pin`
  - Status: planned
  - Depends on: none
  - Goal: pin the generated CDP dependency contract and isolate generated protocol struct construction without broad backend refactoring.

- `viewport-metrics-backend-api`
  - Status: planned
  - Depends on: `set-viewport`
  - Goal: make live viewport metrics a typed backend/session capability instead of repeated JavaScript probes.

- `set-viewport-schema-constraints`
  - Status: planned
  - Depends on: `set-viewport`
  - Goal: advertise `set_viewport` numeric and reset-only constraints in the MCP schema.

- `viewport-output-contract-alignment`
  - Status: planned
  - Depends on: `set-viewport`, `tool-output-contract-normalization`
  - Goal: add canonical shape-specific output names for viewport metrics and scroll state while preserving compatibility aliases before shared output DTOs move.

- `contract-domain-extraction`
  - Status: planned
  - Depends on: none
  - Goal: move pure schema-facing browser-use DTOs into a focused `src/contract` module while preserving compatibility re-exports and MCP schemas; viewport DTO movement waits on the viewport contract specs.

- `mcp-blocking-execution-boundary`
  - Status: planned
  - Depends on: none
  - Goal: make blocking browser/tool work cross an explicit MCP execution boundary under the server runtime without making browser internals async.

- `typed-browser-error-taxonomy`
  - Status: planned
  - Depends on: none
  - Goal: replace stringly page-target-loss, attach-degraded, and unsupported-backend channels with typed error details and compatible structured failures.

- `typed-browser-command-layer`
  - Status: planned
  - Depends on: `typed-browser-error-taxonomy`
  - Goal: introduce typed commands for shared JavaScript-backed operations so Chrome and fake backends handle structured intent instead of rendered script text.

- `snapshot-projector-extraction`
  - Status: planned
  - Depends on: `contract-domain-extraction`, `viewport-metrics-backend-api`
  - Goal: extract snapshot projection, scoped rendering, and delta policy into a pure projector module with deterministic fixture coverage.

Superseded:
- `core-backend-seam-extraction`
  - Status: dropped
  - Replaced by: `headless-chrome-cdp-contract-pin`, `viewport-metrics-backend-api`, `snapshot-projector-extraction`, `mcp-blocking-execution-boundary`, `typed-browser-command-layer`
  - Reason: the original spec mixed CDP helpers, page metrics, snapshot projection, and broad backend cleanup; the new split gives workers narrower write scopes and clearer validation.

Shared program constraint:
- broad `snapshot` payload redesign is out of scope unless a dedicated snapshot spec explicitly takes it on. Public contracts introduced here must stay valid while locality tuning lands.
