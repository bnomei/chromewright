# Requirements - typed-browser-error-taxonomy

## Scope

Replace fragile recovery/error string channels with typed browser error details for page target loss, attach-session degradation, and unsupported backend capabilities.

This spec covers internal error variants, conversion helpers, structured tool failure mapping, and regression tests. It does not redesign every `BrowserError` variant or change public tool output shapes unless explicitly covered by compatibility tests.

## Requirements

- R001: When the Chrome backend detects recoverable page target loss, the system shall represent it with typed details that include operation, source message, recoverability, and recovery hint data.
- R002: When attach mode degrades because a page target is lost, the system shall build structured tool failure payloads from typed error details rather than decoding a prefixed string reason.
- R003: When a backend does not support a capability, the system shall return a typed unsupported-capability error instead of ad hoc string text.
- R004: When upstream `headless_chrome` errors must still be classified by message text, the system shall isolate that string matching at the Chrome adapter boundary.
- R005: When MCP result conversion handles typed browser errors, existing public structured failure fields shall remain compatible unless a requirement explicitly approves a new field.
- R006: When tests assert recovery behavior, they shall assert typed fields or structured payload fields rather than matching opaque display strings.
- R007: If migrating a broad error family would require unrelated behavior changes, then the task shall leave that family in string form and record it as follow-up.

