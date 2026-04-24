# Requirements - mcp-blocking-execution-boundary

## Scope

Introduce an explicit execution boundary for blocking browser/tool work invoked through MCP transports.

This spec covers MCP handler execution mode, feature-gated compile behavior, blocking-pool dispatch for the server binary, and validation. It does not rewrite browser internals to async, replace `headless_chrome`, or change tool outputs.

## Requirements

- R001: When an MCP transport invokes a browser tool under the `mcp-server` feature, the system shall run blocking browser/tool execution behind an explicit boundary rather than inline inside a ready future.
- R002: When `list_tools` or server info is requested, the system shall keep those metadata operations lightweight and shall not route them through the blocking browser execution path.
- R003: When the crate is built with `mcp-handler` but without `mcp-server`, the system shall preserve the current feature-gated compile surface unless an explicit Cargo feature change is documented in this spec.
- R004: When blocking execution panics or the worker join fails, the MCP adapter shall return a structured internal MCP error rather than hanging the request.
- R005: When tool execution succeeds or returns a tool-local structured failure, the system shall preserve the existing `CallToolResult` conversion behavior.
- R006: When multiple HTTP MCP requests arrive, long-running browser work shall not occupy Tokio core worker threads directly.
- R007: If cancellation or per-operation timeout cannot be fully implemented without a larger browser actor, then this spec shall expose a narrow executor seam and leave cancellation as a documented follow-up rather than pretending it is solved.

