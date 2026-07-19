//! rmcp `ServerHandler` that dispatches registered browser tools on a shared session.
//!
//! Tools share one [`BrowserSession`] lifecycle. Companion builds
//! (`from_companion`) also expose the bounded TUI resource catalog; standard
//! stdio/`serve` servers stay tools-only even when the `tui` feature is compiled.

use crate::browser::{BrowserSession, ConnectionOptions};
use crate::mcp::{convert_result, mcp_internal_error};
use crate::tools::ToolDescriptor;
#[cfg(feature = "tui")]
use crate::tools::ToolEffect;
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

#[cfg(feature = "tui")]
use crate::tools::ToolResult as InternalToolResult;
#[cfg(feature = "tui")]
use crate::tui::{PageCoordinator, SharedTuiState};
#[cfg(feature = "tui")]
use rmcp::model::{
    ListResourceTemplatesResult, ListResourcesResult, ReadResourceRequestParams, ReadResourceResult,
};

/// Shared-session MCP server that dispatches registered browser tools via rmcp.
///
/// Implements `ServerHandler` for tool list/call. Tool-local failures map to
/// structured `CallToolResult` errors (preserving document, target, recovery);
/// infrastructure failures map to MCP internal errors. With the `tokio` feature,
/// tool work runs on a blocking pool so the async handler stays free.
///
/// Resource capability and the semantic catalog are available only on the
/// co-hosted TUI companion (`from_companion`). Standard stdio/serve servers
/// remain tools-only even when compiled with `feature = "tui"`.
#[derive(Clone)]
pub struct BrowserServer {
    session: Arc<BrowserSession>,
    #[cfg(feature = "tui")]
    coordinator: Option<Arc<PageCoordinator>>,
}

impl BrowserServer {
    /// Wrap an existing session so MCP tools share one browser lifecycle.
    pub fn from_session(session: BrowserSession) -> Self {
        Self {
            session: Arc::new(session),
            #[cfg(feature = "tui")]
            coordinator: None,
        }
    }

    /// Wrap an already-shared session (tools-only; no companion resource catalog).
    pub fn from_shared_session(session: Arc<BrowserSession>) -> Self {
        Self {
            session,
            #[cfg(feature = "tui")]
            coordinator: None,
        }
    }

    /// Co-hosted companion server over the TUI's shared session and coordination state.
    #[cfg(feature = "tui")]
    pub fn from_companion(coordinator: Arc<PageCoordinator>) -> Self {
        Self {
            session: coordinator.session().clone(),
            coordinator: Some(coordinator),
        }
    }

    /// Launch a browser with default options and expose it as an MCP server.
    pub fn new() -> Result<Self, String> {
        let session =
            BrowserSession::new().map_err(|e| format!("Failed to launch browser: {}", e))?;

        Ok(Self::from_session(session))
    }

    /// Launch with explicit options (headless, executable path, profile, port).
    pub fn with_options(options: crate::browser::LaunchOptions) -> Result<Self, String> {
        let session = BrowserSession::launch(options)
            .map_err(|e| format!("Failed to launch browser: {}", e))?;

        Ok(Self::from_session(session))
    }

    /// Attach to an existing DevTools / WebSocket endpoint as an MCP server.
    pub fn connect(options: ConnectionOptions) -> Result<Self, String> {
        let session = BrowserSession::connect(options)
            .map_err(|e| format!("Failed to connect browser session: {}", e))?;

        Ok(Self::from_session(session))
    }

    /// Borrow the shared browser session used for tool dispatch.
    pub(crate) fn session(&self) -> &BrowserSession {
        self.session.as_ref()
    }

    /// Advertise registered tools as rmcp descriptors (schemas + safety annotations).
    pub(crate) fn list_mcp_tools(&self) -> Vec<McpTool> {
        self.session()
            .tool_registry()
            .descriptors()
            .into_iter()
            .map(tool_descriptor_to_mcp)
            .collect()
    }

    /// Run one tool call on the shared session and map the outcome to `CallToolResult`.
    ///
    /// Tool-local failures stay structured success/error content; only registry or
    /// conversion infrastructure issues become MCP internal errors.
    pub(crate) fn execute_tool_sync(
        &self,
        request: CallToolRequestParams,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "tui")]
        if let Some(coordinator) = &self.coordinator
            && self.session().tool_registry().effect(request.name.as_ref())
                == ToolEffect::BrowserMutation
        {
            // Companion page mutators own the same Loading → capture → Ready|Error
            // lifecycle as terminal actions and tui_refresh.
            return execute_companion_page_mutation(coordinator, request);
        }

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

    #[cfg(feature = "tui")]
    fn companion_shared(&self) -> Option<&SharedTuiState> {
        self.coordinator.as_ref().map(|c| c.shared())
    }
}

/// Run a companion page mutation under the shared Loading lifecycle, then
/// settle/capture/publish Ready or Error with the last valid document retained.
#[cfg(feature = "tui")]
fn execute_companion_page_mutation(
    coordinator: &PageCoordinator,
    request: CallToolRequestParams,
) -> Result<CallToolResult, McpError> {
    let action = request.name.to_string();
    let ticket = match coordinator.begin(&action) {
        Ok(ticket) => ticket,
        Err(error) => return convert_result(InternalToolResult::failure(error.to_string())),
    };
    let session = coordinator.session().as_ref();

    let mut context = crate::tools::ToolContext::new(session);
    let params = request
        .arguments
        .map(serde_json::Value::Object)
        .unwrap_or_else(|| serde_json::json!({}));

    match session
        .tool_registry()
        .execute(request.name.as_ref(), params, &mut context)
    {
        Ok(result) if result.success => {
            let page = match coordinator.finalize_browser_mutation(ticket, &action) {
                Ok(page) => page,
                Err(error) => {
                    return convert_result(InternalToolResult::failure(error.to_string()));
                }
            };
            if action == "tui_refresh" {
                return convert_result(InternalToolResult::success_with(
                    crate::tools::tui::TuiResult {
                        available: true,
                        data: Some(crate::tools::tui::TuiData::Refresh {
                            document_id: page.document_id,
                            revision: page.revision,
                            url: page.url,
                            title: page.title,
                        }),
                        error: None,
                    },
                ));
            }
            convert_result(result)
        }
        Ok(result) => {
            let message = result
                .error
                .clone()
                .unwrap_or_else(|| format!("{action} failed"));
            let _ = coordinator.fail(ticket, &action, message);
            convert_result(result)
        }
        Err(error) => {
            let _ = coordinator.fail(ticket, &action, error.to_string());
            Err(mcp_internal_error(error))
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

    #[cfg(feature = "tui")]
    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        future::ready(Ok(match self.companion_shared() {
            Some(shared) => crate::mcp::resources::list_resources(shared),
            None => ListResourcesResult::default(),
        }))
    }

    #[cfg(feature = "tui")]
    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourceTemplatesResult, McpError>> + Send + '_
    {
        future::ready(Ok(match self.companion_shared() {
            Some(_) => crate::mcp::resources::resource_templates(),
            None => ListResourceTemplatesResult::default(),
        }))
    }

    #[cfg(feature = "tui")]
    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ReadResourceResult, McpError>> + Send + '_ {
        future::ready(match self.companion_shared() {
            Some(shared) => {
                crate::mcp::resources::read_resource(shared, &request.uri).map_err(|error| {
                    match error {
                        crate::mcp::resources::ResourceError::MalformedUri => {
                            McpError::invalid_params(error.to_string(), None)
                        }
                        crate::mcp::resources::ResourceError::NotFound
                        | crate::mcp::resources::ResourceError::Coordination(_)
                        | crate::mcp::resources::ResourceError::Render(_) => {
                            McpError::resource_not_found(error.to_string(), None)
                        }
                    }
                })
            }
            None => Err(McpError::method_not_found::<
                rmcp::model::ReadResourceRequestMethod,
            >()),
        })
    }

    fn get_info(&self) -> ServerInfo {
        self.server_info()
    }
}

fn server_info() -> ServerInfo {
    ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        .with_instructions("chromewright MCP server")
}

impl BrowserServer {
    fn server_info(&self) -> ServerInfo {
        #[cfg(feature = "tui")]
        if self.coordinator.is_some() {
            return ServerInfo::new(
                ServerCapabilities::builder()
                    .enable_tools()
                    .enable_resources()
                    .build(),
            )
            .with_instructions(
                "chromewright TUI companion MCP server (shared session, tui_* tools, semantic resources)",
            );
        }

        server_info()
    }
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
        assert!(
            info.capabilities.resources.is_none(),
            "standard stdio/serve servers must not advertise resources"
        );
    }

    #[cfg(feature = "tui")]
    #[test]
    fn tui_companion_server_info_enables_resources_while_stdio_does_not() {
        use crate::tui::SharedTuiState;
        use rmcp::ServerHandler;
        use std::sync::Arc;

        let session = Arc::new(BrowserSession::with_test_backend(FakeSessionBackend::new()));
        let shared = SharedTuiState::new();
        let companion = BrowserServer::from_companion(Arc::new(crate::tui::PageCoordinator::new(
            session.clone(),
            shared,
        )));
        assert!(companion.get_info().capabilities.resources.is_some());

        let stdio = BrowserServer::from_shared_session(session);
        assert!(stdio.get_info().capabilities.resources.is_none());
    }

    #[cfg(feature = "tui")]
    #[test]
    fn tui_companion_rejects_page_actions_while_loading() {
        use crate::tui::SharedTuiState;
        use std::sync::Arc;

        let session = Arc::new(BrowserSession::with_test_backend(FakeSessionBackend::new()));
        let shared = SharedTuiState::new();
        shared.activate_runtime();
        shared.begin_page_action("navigate").expect("claim loading");
        let server = BrowserServer::from_companion(Arc::new(crate::tui::PageCoordinator::new(
            session, shared,
        )));
        let result = server
            .execute_tool_sync(call_tool_request("navigate", None))
            .expect("tool-local rejection");
        assert_eq!(result.is_error, Some(true));
        let message = result
            .structured_content
            .as_ref()
            .and_then(|content| content.get("error"))
            .and_then(|error| error.as_str())
            .unwrap_or_default();
        assert!(
            message.contains("already in progress"),
            "unexpected rejection message: {message}"
        );
    }

    #[cfg(feature = "tui")]
    #[test]
    fn tui_companion_page_mutation_publishes_loading_to_ready_atomically() {
        use crate::tui::{Lifecycle, SharedTuiState};
        use serde_json::json;
        use std::sync::Arc;

        let session = Arc::new(BrowserSession::with_test_backend(FakeSessionBackend::new()));
        let shared = SharedTuiState::new();
        shared.activate_runtime();
        // Seed a known document so resources can observe retention across the mutation.
        let before = session.extract_semantic_document().expect("seed capture");
        shared.publish(before);
        let prior_revision = shared.active().unwrap().document.revision.clone();

        let server = BrowserServer::from_companion(Arc::new(crate::tui::PageCoordinator::new(
            session,
            shared.clone(),
        )));
        let mut args = serde_json::Map::new();
        args.insert("url".into(), json!("https://example.test/next"));
        let result = server
            .execute_tool_sync(call_tool_request("navigate", Some(args)))
            .expect("navigate");
        assert_eq!(result.is_error, Some(false));
        assert!(shared.lifecycle().is_ready());
        let after = shared.active().expect("published after navigate");
        assert_ne!(after.document.revision, prior_revision);
        assert_eq!(after.document.url, "https://example.test/next");
        // Concurrent mutation while ready is fine; while loading is rejected above.
        assert!(matches!(shared.lifecycle(), Lifecycle::Ready));
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
