//! ServerHandler implementation for BrowserSession

use crate::browser::{BrowserSession, ConnectionOptions};
use crate::mcp::{convert_result, mcp_internal_error};
use crate::tools::ToolDescriptor;
use log::debug;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerInfo, Tool as McpTool, ToolAnnotations as McpToolAnnotations,
    },
    service::RequestContext,
};
use std::future;
use std::sync::Arc;

/// MCP Server wrapper for BrowserSession
///
/// This struct holds a browser session and provides thread-safe access
/// for MCP tool execution.
#[derive(Clone)]
pub struct BrowserServer {
    session: Arc<BrowserSession>,
}

impl BrowserServer {
    /// Create a server from a preconfigured browser session.
    pub fn from_session(session: BrowserSession) -> Self {
        Self {
            session: Arc::new(session),
        }
    }

    /// Create a new browser server with default launch options.
    pub fn new() -> Result<Self, String> {
        let session =
            BrowserSession::new().map_err(|e| format!("Failed to launch browser: {}", e))?;

        Ok(Self::from_session(session))
    }

    /// Create a new browser server with custom launch options.
    pub fn with_options(options: crate::browser::LaunchOptions) -> Result<Self, String> {
        let session = BrowserSession::launch(options)
            .map_err(|e| format!("Failed to launch browser: {}", e))?;

        Ok(Self::from_session(session))
    }

    /// Create a browser server by connecting to an existing WebSocket endpoint.
    pub fn connect(options: ConnectionOptions) -> Result<Self, String> {
        let session = BrowserSession::connect(options)
            .map_err(|e| format!("Failed to connect browser session: {}", e))?;

        Ok(Self::from_session(session))
    }

    /// Get a reference to the shared browser session.
    pub(crate) fn session(&self) -> &BrowserSession {
        self.session.as_ref()
    }

    pub(crate) fn list_mcp_tools(&self) -> Vec<McpTool> {
        self.session()
            .tool_registry()
            .descriptors()
            .into_iter()
            .map(tool_descriptor_to_mcp)
            .collect()
    }

    pub(crate) fn execute_tool_sync(
        &self,
        request: CallToolRequestParams,
    ) -> Result<CallToolResult, McpError> {
        let mut context = crate::tools::ToolContext::new(self.session());
        let params = request
            .arguments
            .map(serde_json::Value::Object)
            .unwrap_or_else(|| serde_json::json!({}));

        match self
            .session()
            .tool_registry()
            .execute(request.name.as_ref(), params, &mut context)
        {
            Ok(result) => convert_result(result),
            Err(error) => Err(mcp_internal_error(error)),
        }
    }
}

#[cfg(feature = "tokio")]
fn join_blocking_tool_result(
    result: std::result::Result<Result<CallToolResult, McpError>, tokio::task::JoinError>,
) -> Result<CallToolResult, McpError> {
    match result {
        Ok(result) => result,
        Err(error) => Err(mcp_internal_error(error)),
    }
}

fn tool_descriptor_to_mcp(descriptor: ToolDescriptor) -> McpTool {
    let ToolDescriptor {
        name,
        description,
        parameters_schema,
        output_schema,
        annotations,
    } = descriptor;

    let input_schema = match parameters_schema {
        serde_json::Value::Object(object) => object,
        _ => serde_json::Map::new(),
    };
    let output_schema = match output_schema {
        serde_json::Value::Object(object) => Some(Arc::new(object)),
        _ => None,
    };

    let mut tool = McpTool::new(name, description, Arc::new(input_schema));
    tool.output_schema = output_schema;
    tool.annotations = Some(McpToolAnnotations::from_raw(
        None,
        Some(annotations.read_only_hint),
        Some(annotations.destructive_hint),
        Some(annotations.idempotent_hint),
        Some(annotations.open_world_hint),
    ));
    tool
}

impl Drop for BrowserServer {
    fn drop(&mut self) {
        debug!("BrowserServer dropped");
    }
}

impl ServerHandler for BrowserServer {
    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        #[cfg(feature = "tokio")]
        {
            let server = self.clone();
            async move {
                join_blocking_tool_result(
                    tokio::task::spawn_blocking(move || server.execute_tool_sync(request)).await,
                )
            }
        }

        #[cfg(not(feature = "tokio"))]
        {
            future::ready(self.execute_tool_sync(request))
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        future::ready(Ok(ListToolsResult::with_all_items(self.list_mcp_tools())))
    }

    fn get_info(&self) -> ServerInfo {
        server_info()
    }
}

fn server_info() -> ServerInfo {
    ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        .with_instructions("chromewright MCP server")
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "tokio")]
    use super::join_blocking_tool_result;
    use super::{BrowserServer, server_info};
    use crate::browser::BrowserSession;
    use crate::browser::backend::FakeSessionBackend;
    use rmcp::model::CallToolRequestParams;
    #[cfg(feature = "tokio")]
    use serde_json::json;

    fn call_tool_request(
        name: &'static str,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> CallToolRequestParams {
        let request = CallToolRequestParams::new(name);
        if let Some(arguments) = arguments {
            request.with_arguments(arguments)
        } else {
            request
        }
    }

    #[test]
    fn test_server_info_enables_tools_and_instructions() {
        let info = server_info();

        assert!(
            info.instructions
                .as_deref()
                .unwrap_or_default()
                .contains("chromewright MCP server")
        );
        assert!(info.capabilities.tools.is_some());
    }

    #[test]
    fn execute_tool_sync_converts_success_results() {
        let server = BrowserServer::from_session(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        ));
        let result = server
            .execute_tool_sync(call_tool_request("tab_list", None))
            .expect("tab_list should execute");

        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|content| content.get("count"))
                .and_then(|count| count.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn execute_tool_sync_preserves_tool_local_failures() {
        let server = BrowserServer::from_session(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        ));
        let result = server
            .execute_tool_sync(call_tool_request("missing_tool", None))
            .expect("tool-local failures should convert to CallToolResult");

        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|content| content.get("code"))
                .and_then(|code| code.as_str()),
            Some("tool_error")
        );
    }

    #[test]
    fn list_mcp_tools_uses_metadata_without_tool_execution() {
        let server = BrowserServer::from_session(BrowserSession::with_test_backend(
            FakeSessionBackend::new(),
        ));
        let tools = server.list_mcp_tools();

        assert!(tools.iter().any(|tool| tool.name.as_ref() == "snapshot"));
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn blocking_join_failure_maps_to_internal_mcp_error() {
        let joined = tokio::task::spawn_blocking(|| {
            panic!("simulated blocking executor panic");
            #[allow(unreachable_code)]
            Ok(rmcp::model::CallToolResult::structured(json!({})))
        })
        .await;

        let error = join_blocking_tool_result(joined)
            .expect_err("blocking executor panic should map to MCP error");
        assert!(
            error
                .to_string()
                .contains("simulated blocking executor panic")
        );
    }
}
