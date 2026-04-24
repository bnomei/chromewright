# Requirements - viewport-output-contract-alignment

## Scope

Align public action output naming so incompatible viewport-related payloads are not exposed under the same top-level field name without a documented migration path.

This spec covers `set_viewport` and `scroll` success outputs, schema tests, and docs. It does not remove existing fields in the same change unless the user explicitly approves a breaking contract change.

## Requirements

- R001: When a public tool output exposes viewport size and device pixel ratio, the system shall expose that object under a canonical `viewport_metrics_after` field.
- R002: When a public tool output exposes scroll position and scroll-boundary state, the system shall expose that object under a canonical `scroll_after` field.
- R003: While preserving compatibility, existing `viewport_after` fields may remain as aliases, but the schema and docs shall identify the canonical field for each payload shape.
- R004: If a breaking cleanup removes legacy aliases, then that removal shall happen in a separate explicitly approved spec or task.
- R005: When MCP output schemas are generated, `set_viewport` and `scroll` shall advertise their canonical fields and shall not force clients to infer incompatible shapes from the same `viewport_after` name.
- R006: Runtime behavior for viewport emulation, scrolling, document metadata, and target continuity shall remain unchanged.

