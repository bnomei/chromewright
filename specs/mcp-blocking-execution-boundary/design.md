# Design - mcp-blocking-execution-boundary

## Goal

Make the sync browser runtime honest at the MCP boundary. The first win is not end-to-end async; it is to stop executing blocking browser operations directly inside the async-shaped handler path used by the server.

## Distilled Discovery

- `Cargo.toml` has `mcp-handler` without `tokio`, while `mcp-server` adds `tokio`, `axum`, and MCP transports.
- `src/mcp/handler.rs` implements `ServerHandler::call_tool` by returning `future::ready` around synchronous registry execution.
- Tool execution can call `headless_chrome`, evaluate browser JavaScript, capture screenshots, poll with `std::thread::sleep`, and perform filesystem artifact IO.
- `list_tools` only maps registered descriptors into MCP schemas and does not need the blocking execution path.
- Existing `convert_result` and `mcp_internal_error` functions already define the correct MCP result conversion behavior.

## Proposed Execution Shape

Add a narrow helper around synchronous tool execution:

```text
BrowserServer::execute_tool_sync(request) -> Result<CallToolResult, McpError>
```

Then add feature-gated async dispatch:

```text
#[cfg(feature = "tokio")]
call_tool -> async move {
    let server = self.clone();
    tokio::task::spawn_blocking(move || server.execute_tool_sync(request)).await
}

#[cfg(not(feature = "tokio"))]
call_tool -> future::ready(self.execute_tool_sync(request))
```

The exact implementation can use a small `ToolExecutor` type if that keeps tests cleaner, but the important boundary is:

- MCP metadata stays inline
- browser/tool execution crosses a named blocking boundary when Tokio is available
- join failures become MCP internal errors
- successful and structured-failure tool results continue through `convert_result`

## Cancellation And Timeout Policy

This spec should not claim full cancellation. `headless_chrome` calls and page-side waits are synchronous today. The executor boundary should make future cancellation/actor work possible by isolating where tool execution is scheduled.

If a later design needs preemptive cancellation, it should get its own spec or loop after this boundary lands.

## Validation Plan

Required checks:

```text
cargo check --locked --no-default-features
cargo check --locked --no-default-features --features mcp-handler
cargo check --locked --features mcp-server
cargo test --lib mcp
cargo test --bin chromewright
```

Recommended focused tests:

- `call_tool` still converts normal tool success and structured tool failure exactly as before.
- A simulated blocking-executor join failure returns MCP internal error.
- `list_tools` does not require a blocking executor.

## Non-Goals

- Making `SessionBackend` async
- Replacing `std::thread::sleep` loops inside browser code
- Adding a browser actor queue
- Changing tool result schemas
- Changing stdio or HTTP protocol behavior

