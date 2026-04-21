# Requirements — performance-and-surface-hardening

## Scope

This spec covers the next hardening wave for the browser automation runtime:

- MCP server execution flow and browser-session sharing
- lightweight document metadata and revision polling
- post-action tool payload size and DOM refresh policy
- active-tab selection and tab activation bookkeeping
- markdown extraction reuse across repeated reads of the same revision
- high-level input clearing behavior
- default-safe navigation policy for high-level tools
- explicit acknowledgement and path constraints for operator tools
- regression tests and docs for the updated contract

This spec builds on `specs/agent-document-surface-hardening/` and keeps the revision-scoped
document model intact. It does not attempt evaluator-driven benchmarking or Chrome/CDP
dependency replacement in this wave.

## Requirements

- R001: When the MCP server runs browser tools, the system shall avoid binding all transport progress to a single Tokio current-thread runtime.
- R002: When a high-level tool only needs updated document identity after a mutation or navigation, the system shall return document metadata without reconstructing the full DOM snapshot.
- R003: When waiting for revision changes, the system shall poll a lightweight revision/metadata surface instead of rebuilding the full `DomTree` each cycle.
- R004: When high-level action tools report post-action state, the system shall default to a metadata-first envelope and exclude large snapshot/node payloads unless the caller explicitly requests them.
- R005: When resolving the active tab repeatedly, the system shall reuse a validated tab hint and only fall back to cross-tab probing when the hint is absent or stale.
- R006: When markdown is requested more than once for the same document revision, the system shall reuse cached extraction results for pagination and repeated reads.
- R007: If a high-level input action clears an element before typing, then the system shall clear the element with a bounded direct mutation path instead of a keypress loop proportional to text length.
- R008: If a high-level navigation request targets a non-web or otherwise unsafe scheme, then the system shall reject it by default unless the caller explicitly opts into unsafe navigation.
- R009: When operator tools execute raw JavaScript or write files, the system shall require explicit unsafe acknowledgement and shall constrain screenshot writes to safe relative output paths.
- R010: When this hardening wave changes runtime behavior or tool contracts, the repository shall enforce the new behavior with targeted unit and integration coverage plus updated user-facing documentation.
