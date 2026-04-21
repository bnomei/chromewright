# Requirements — agent-document-surface-hardening

## Scope

This spec covers the agent-facing document model inside an already addressed browser tab/frame:

- document snapshots and element targeting
- post-action state reporting
- wait and settle semantics
- DOM cache ownership and staleness
- iframe-aware document assembly
- contract tests for the document interaction surface

This spec does not change tab identity or tab-selection policy yet. Stable tab IDs and multi-agent tab ownership remain follow-on work.

## Requirements

- R001: When producing a document snapshot for agent use, the system shall return a document-scoped revision token and node references that are valid for that revision.
- R002: If a high-level tool receives a node reference from an older or different document revision, then the system shall reject it with a structured stale-reference error instead of re-resolving it heuristically.
- R003: When exposing agent-targetable nodes, the system shall make actionable document elements the primary targeting surface instead of broad container-role indexing.
- R004: When a high-level tool changes document state, focus, form value, or navigation, the system shall return a uniform post-action document envelope that includes document metadata and the resulting revision.
- R005: While waiting for document changes, the system shall support explicit postconditions beyond CSS-selector existence, including navigation settle and node-state checks.
- R006: Where same-origin iframe content is accessible, the system shall preserve frame boundaries and include that content in the document model or report that the frame could not be assembled.
- R007: When direct Rust consumers reuse `ToolContext` across tool calls, the system shall centrally invalidate or version cached DOM state so stale snapshots are not returned after mutations.
- R008: When exposing agent-facing browser tools, the system shall keep raw JavaScript evaluation and filesystem-path screenshot capture outside the default high-level document interaction contract.
- R009: When the document interaction contract changes, the repository shall enforce it with browser-backed contract tests that cover snapshot generation, element targeting, mutation, and stale-reference handling.
