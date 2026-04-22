//! ServerHandler implementation for BrowserSession

use crate::browser::{BrowserSession, ConnectionOptions};
use crate::mcp::{convert_result, mcp_internal_error};
use crate::tools::ToolDescriptor;
use log::debug;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerInfo, Tool as McpTool,
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
}

fn tool_descriptor_to_mcp(descriptor: ToolDescriptor) -> McpTool {
    let ToolDescriptor {
        name,
        description,
        parameters_schema,
        output_schema,
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
        future::ready({
            let mut context = crate::tools::ToolContext::new(self.session());
            let params = request
                .arguments
                .map(serde_json::Value::Object)
                .unwrap_or_else(|| serde_json::json!({}));

            match self.session().tool_registry().execute(
                request.name.as_ref(),
                params,
                &mut context,
            ) {
                Ok(result) => convert_result(result),
                Err(error) => Err(mcp_internal_error(error)),
            }
        })
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
    use super::server_info;

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
}
