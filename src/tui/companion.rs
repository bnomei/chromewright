//! Loopback MCP companion co-hosted with the interactive terminal browser.
//!
//! Binds streamable HTTP only on localhost, nests under a configured absolute path,
//! and hands every session a [`BrowserServer`] built with the same
//! [`BrowserSession`] + [`SharedTuiState`] the TUI owns. Standard stdio MCP remains
//! a separate process boundary and never registers `tui_*` tools or semantic
//! resources through this path.

use crate::tui::SharedTuiState;
use crate::{BrowserServer, BrowserSession};
use axum::Router;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;

/// Running loopback MCP companion: HTTP task handle plus the bound address/path.
///
/// Drop is not enough — call [`Companion::stop`] so the listener is aborted
/// before the terminal restores raw mode.
pub struct Companion {
    task: tokio::task::JoinHandle<()>,
    #[cfg_attr(not(test), allow(dead_code))]
    address: SocketAddr,
    #[cfg_attr(not(test), allow(dead_code))]
    path: String,
}

fn loopback_address(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

/// Reject relative paths and URL-shaped values so the nest route cannot escape
/// the intended companion endpoint.
fn validate_http_path(path: &str) -> Result<String, String> {
    if path.is_empty() || !path.starts_with('/') {
        return Err(format!(
            "companion HTTP path must be an absolute path starting with '/', got {path:?}"
        ));
    }
    if path.contains("://") || path.contains(' ') {
        return Err(format!("companion HTTP path is invalid: {path:?}"));
    }
    Ok(path.to_owned())
}

/// Start the companion on loopback, sharing the live TUI session and coordination state.
///
/// `port` may be `0` for an ephemeral bind. Refuses non-loopback addresses even
/// if the OS would otherwise allow them. The returned [`Companion`] owns the
/// serve task until [`Companion::stop`].
pub fn start(
    session: Arc<BrowserSession>,
    shared: SharedTuiState,
    path: String,
    port: u16,
) -> Result<Companion, String> {
    let path = validate_http_path(&path)?;
    let listener = TcpListener::bind(loopback_address(port))
        .map_err(|e| format!("failed to bind TUI MCP companion on loopback: {e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;
    if !address.ip().is_loopback() {
        return Err(format!(
            "TUI MCP companion refused non-loopback bind address {address}"
        ));
    }
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let listener = tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?;
    let service = StreamableHttpService::new(
        move || {
            Ok(BrowserServer::from_companion(
                session.clone(),
                shared.clone(),
            ))
        },
        LocalSessionManager::default().into(),
        Default::default(),
    );
    let router = Router::new().nest_service(&path, service);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Ok(Companion {
        task,
        address,
        path,
    })
}

impl Companion {
    /// Bound loopback socket (resolved after ephemeral-port allocation).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Absolute HTTP nest path the streamable MCP service is mounted under.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Abort the serve task so the loopback listener becomes unreachable.
    pub fn stop(self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::backend::FakeSessionBackend;
    use crate::dom::DocumentMetadata;
    use crate::semantic::SemanticDocument;
    use crate::tools::tui::{NAMES, TuiTool};
    use crate::tui::SharedTuiState;
    use rmcp::ServerHandler;
    use std::sync::Arc;

    fn active_companion_session() -> (Arc<BrowserSession>, SharedTuiState) {
        let shared = SharedTuiState::unbound();
        let mut session = Arc::new(BrowserSession::with_test_backend(FakeSessionBackend::new()));
        let session_mut = Arc::get_mut(&mut session).expect("unique test session");
        for name in NAMES {
            session_mut
                .tool_registry_mut()
                .register(TuiTool::with_shared(name, shared.clone()));
        }
        shared
            .bind_session(session.clone())
            .expect("bind shared session");
        shared.activate_runtime();
        (session, shared)
    }

    fn probe_status(url: String) -> u16 {
        match ureq::get(&url).call() {
            Ok(response) => response.status().as_u16(),
            Err(ureq::Error::StatusCode(status)) => status,
            Err(error) => panic!("HTTP probe failed for {url}: {error}"),
        }
    }

    fn response_json(mut response: ureq::http::Response<ureq::Body>) -> serde_json::Value {
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("<missing>")
            .to_string();
        let body = response
            .body_mut()
            .read_to_string()
            .expect("read MCP response");
        let json = body
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim))
            .find(|data| !data.is_empty())
            .unwrap_or(body.trim());
        serde_json::from_str(json).unwrap_or_else(|error| {
            panic!(
                "valid MCP JSON response (status={status}, content-type={content_type}, body={body:?}): {error}"
            )
        })
    }

    /// Sandboxed CI environments can prohibit all localhost binds. Keep the
    /// integration tests real where sockets are available, but skip only that
    /// explicit capability denial rather than treating it as a product defect.
    fn loopback_bind_available() -> bool {
        match TcpListener::bind(loopback_address(0)) {
            Ok(listener) => {
                drop(listener);
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipping loopback companion test: localhost binds are denied");
                false
            }
            Err(error) => panic!("unexpected loopback bind failure: {error}"),
        }
    }

    #[tokio::test]
    async fn companion_binds_configured_loopback_path_and_stops_cleanly() {
        if !loopback_bind_available() {
            return;
        }
        let (session, shared) = active_companion_session();
        let companion = start(session, shared.clone(), "/tui-mcp".into(), 0).expect("start");
        let address = companion.address();
        assert!(address.ip().is_loopback());
        assert_eq!(address.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_ne!(address.port(), 0);
        assert_eq!(companion.path(), "/tui-mcp");

        // Configured route is handled by the streamable HTTP service (GET may be
        // 405/406), while a different path is not routed at all.
        tokio::task::yield_now().await;
        let configured_status =
            tokio::task::spawn_blocking(move || probe_status(format!("http://{address}/tui-mcp")))
                .await
                .expect("configured route probe task");
        let wrong_status =
            tokio::task::spawn_blocking(move || probe_status(format!("http://{address}/wrong")))
                .await
                .expect("wrong route probe task");
        assert_ne!(configured_status, 404, "configured path must be routed");
        assert_eq!(wrong_status, 404, "unconfigured path must not be routed");

        companion.stop();
        for _ in 0..40 {
            if std::net::TcpStream::connect(address).is_err() {
                break;
            }
            tokio::task::yield_now().await;
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            std::net::TcpStream::connect(address).is_err(),
            "listener must be unreachable after stop"
        );
        shared.deactivate_runtime();
    }

    #[tokio::test]
    async fn companion_serves_tools_and_resources_over_real_mcp_http_session() {
        if !loopback_bind_available() {
            return;
        }
        let (session, shared) = active_companion_session();
        shared.publish(
            SemanticDocument::empty(DocumentMetadata {
                document_id: "fake-tab".into(),
                revision: "fake:1".into(),
                url: "https://example.test/".into(),
                title: "Example".into(),
                ready_state: "complete".into(),
                frames: vec![],
            })
            .expect("semantic document"),
        );
        let companion = start(session, shared.clone(), "/mcp".into(), 0).expect("start");
        let url = format!("http://{}/mcp", companion.address());

        let (initialize, tools, resources) = tokio::task::spawn_blocking(move || {
            let initialize_response = ureq::post(&url)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .send(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "initialize",
                        "params": {
                            "protocolVersion": "2025-06-18",
                            "capabilities": {},
                            "clientInfo": { "name": "chromewright-test", "version": "1" }
                        }
                    })
                    .to_string(),
                )
                .expect("initialize MCP session");
            let session_id = initialize_response
                .headers()
                .get("mcp-session-id")
                .expect("MCP session header")
                .to_str()
                .expect("session ID text")
                .to_string();
            let initialize = response_json(initialize_response);

            let initialized = ureq::post(&url)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .header("mcp-session-id", &session_id)
                .send(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/initialized"
                    })
                    .to_string(),
                )
                .expect("complete MCP handshake");
            assert_eq!(initialized.status().as_u16(), 202);

            let tools = ureq::post(&url)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .header("mcp-session-id", &session_id)
                .send(
                    serde_json::json!({
                        "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}
                    })
                    .to_string(),
                )
                .map(response_json)
                .expect("list tools");
            let resources = ureq::post(&url)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .header("mcp-session-id", &session_id)
                .send(
                    serde_json::json!({
                        "jsonrpc": "2.0", "id": 3, "method": "resources/list", "params": {}
                    })
                    .to_string(),
                )
                .map(response_json)
                .expect("list resources");
            (initialize, tools, resources)
        })
        .await
        .expect("MCP client task");

        assert_eq!(
            initialize["result"]["instructions"],
            "chromewright TUI companion MCP server (shared session, tui_* tools, semantic resources)"
        );
        let tool_names = tools["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert!(tool_names.contains(&"tui_render"));
        assert!(tool_names.contains(&"tui_attention_set"));
        let resource_uris = resources["result"]["resources"]
            .as_array()
            .expect("resources array")
            .iter()
            .filter_map(|resource| resource["uri"].as_str())
            .collect::<Vec<_>>();
        assert!(resource_uris.contains(&"chromewright://active/semantic.md"));
        assert!(resource_uris.contains(&"chromewright://tui/attention.json"));

        companion.stop();
        shared.deactivate_runtime();
    }

    #[test]
    fn companion_startup_fails_for_invalid_path_and_port_conflicts() {
        if !loopback_bind_available() {
            return;
        }
        let (session, shared) = active_companion_session();
        match start(session.clone(), shared.clone(), "mcp".into(), 0) {
            Ok(_) => panic!("relative path must fail"),
            Err(err) => assert!(err.contains("absolute path"), "{err}"),
        }

        let holder = TcpListener::bind(loopback_address(0)).expect("bind holder");
        let port = holder.local_addr().unwrap().port();
        match start(session, shared, "/mcp".into(), port) {
            Ok(_) => panic!("port conflict must fail"),
            Err(err) => assert!(
                err.contains("failed to bind TUI MCP companion on loopback"),
                "{err}"
            ),
        }
    }

    #[test]
    fn active_companion_registry_exposes_and_dispatches_all_tui_tools() {
        let (session, shared) = active_companion_session();
        for name in NAMES {
            assert!(session.tool_registry().has(name), "missing {name}");
        }

        shared.publish(
            SemanticDocument::empty(DocumentMetadata {
                document_id: "fake-tab".into(),
                revision: "fake:1".into(),
                url: "https://example.test/".into(),
                title: "Example".into(),
                ready_state: "complete".into(),
                frames: vec![],
            })
            .expect("semantic document"),
        );

        // Prove every registered tool dispatches against the exact shared state.
        let render = session
            .execute_tool("tui_render", serde_json::json!({}))
            .expect("tui_render");
        assert!(render.success);
        assert_eq!(
            render
                .data
                .as_ref()
                .and_then(|data| data["available"].as_bool()),
            Some(true)
        );

        let query = session
            .execute_tool("tui_query", serde_json::json!({ "limit": 128 }))
            .expect("tui_query");
        assert!(query.success);

        let inspect = session
            .execute_tool("tui_inspect", serde_json::json!({}))
            .expect("tui_inspect");
        assert!(inspect.success);

        let selection_read = session
            .execute_tool("tui_selection_read", serde_json::json!({}))
            .expect("tui_selection_read");
        assert!(selection_read.success);

        // Selection update fails closed without a resolvable ref; attention is independent.
        let selection_update = session
            .execute_tool(
                "tui_selection_update",
                serde_json::json!({ "semantic_ref": "not-a-ref" }),
            )
            .expect("tui_selection_update");
        assert!(selection_update.success);
        assert_eq!(
            selection_update
                .data
                .as_ref()
                .and_then(|data| data["available"].as_bool()),
            Some(false)
        );
        assert!(shared.selection().is_none());

        // Empty published document has no components; attention requires an exact ref.
        let attention_set = session
            .execute_tool(
                "tui_attention_set",
                serde_json::json!({ "semantic_ref": "not-a-ref", "message": "focus" }),
            )
            .expect("tui_attention_set");
        assert!(attention_set.success);
        assert_eq!(
            attention_set
                .data
                .as_ref()
                .and_then(|data| data["available"].as_bool()),
            Some(false),
            "stale attention must fail closed"
        );
        assert!(!shared.attention().is_set());

        let attention_read = session
            .execute_tool("tui_attention_read", serde_json::json!({}))
            .expect("tui_attention_read");
        assert!(attention_read.success);
        assert_eq!(
            attention_read
                .data
                .as_ref()
                .and_then(|data| data["data"]["semantic_ref"].as_str()),
            None
        );

        let attention_clear = session
            .execute_tool("tui_attention_clear", serde_json::json!({}))
            .expect("tui_attention_clear");
        assert!(attention_clear.success);
        assert!(!shared.attention().is_set());

        let refresh = session
            .execute_tool("tui_refresh", serde_json::json!({}))
            .expect("tui_refresh");
        assert!(refresh.success);
        assert_eq!(
            refresh
                .data
                .as_ref()
                .and_then(|data| data["data"]["revision"].as_str()),
            Some("fake:2")
        );
        assert_eq!(
            shared.active().expect("published").document.revision,
            "fake:2"
        );
    }

    #[test]
    fn companion_server_advertises_resources_while_stdio_server_does_not() {
        let (session, shared) = active_companion_session();
        shared.publish(
            SemanticDocument::empty(DocumentMetadata {
                document_id: "fake-tab".into(),
                revision: "fake:1".into(),
                url: "https://example.test/".into(),
                title: "Example".into(),
                ready_state: "complete".into(),
                frames: vec![],
            })
            .expect("semantic document"),
        );

        let companion = BrowserServer::from_companion(session.clone(), shared.clone());
        assert!(companion.get_info().capabilities.resources.is_some());
        let listed = crate::mcp::resources::list_resources(&shared);
        assert!(
            listed
                .resources
                .iter()
                .any(|resource| resource.uri == "chromewright://active/semantic.md")
        );

        let stdio = BrowserServer::from_shared_session(session);
        assert!(stdio.get_info().capabilities.resources.is_none());
        assert!(stdio.get_info().capabilities.tools.is_some());
    }
}
