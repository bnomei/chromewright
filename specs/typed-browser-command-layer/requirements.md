# Requirements - typed-browser-command-layer

## Scope

Introduce typed browser commands for shared JavaScript-backed operations so production Chrome execution and fake backend behavior depend on structured intent rather than script-text recognition.

This spec covers an internal command model, Chrome rendering/adaptation, fake backend command handling, and migration of actionability/interaction probes. It does not remove the raw `evaluate` operator tool.

## Requirements

- R001: When a tool invokes a shared browser-side operation, the system shall represent the operation as a typed command with typed request and result data.
- R002: When the Chrome backend executes a typed command, the backend shall render the existing JavaScript behavior from the command and decode the result through the existing structured decode path.
- R003: When the fake backend executes a migrated operation, it shall branch on the typed command and command payload rather than matching JavaScript substrings.
- R004: When the raw `evaluate` operator tool executes arbitrary JavaScript, the system shall continue to accept raw scripts and shall not route that public operator surface through command enums.
- R005: When actionability, selector identity, click, input, hover, or select behavior is migrated, public tool outputs and diagnostics shall remain compatible.
- R006: If a command migration would require rewriting browser-side JS semantics, then that operation shall remain on the current script path and be documented as follow-up.
- R007: Tests for migrated fake backend behavior shall assert command data and results, not rendered script text.

