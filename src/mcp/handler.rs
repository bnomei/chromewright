//! ServerHandler implementation for BrowserSession

use crate::browser::{BrowserSession, ConnectionOptions};
use log::debug;
use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    tool_handler,
};
use std::sync::Arc;

/// MCP Server wrapper for BrowserSession
///
/// This struct holds a browser session and provides thread-safe access
/// for MCP tool execution.
#[derive(Clone)]
pub struct BrowserServer {
    session: Arc<BrowserSession>,
    tool_router: ToolRouter<Self>,
}

impl BrowserServer {
    fn from_session(session: BrowserSession) -> Self {
        Self {
            session: Arc::new(session),
            tool_router: Self::tool_router(),
        }
    }

    /// Create a new browser server with default launch options
    pub fn new() -> Result<Self, String> {
        let session =
            BrowserSession::new().map_err(|e| format!("Failed to launch browser: {}", e))?;

        Ok(Self::from_session(session))
    }

    /// Create a new browser server with custom launch options
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
}

impl Default for BrowserServer {
    fn default() -> Self {
        Self::new().expect("Failed to create default browser server")
    }
}

impl Drop for BrowserServer {
    fn drop(&mut self) {
        debug!("BrowserServer dropped");
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BrowserServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Browser-use MCP Server")
    }
}
