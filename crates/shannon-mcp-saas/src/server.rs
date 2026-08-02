//! Minimal JSON-RPC stdio MCP server used as the spike backbone.
//!
//! `shannon-mcp` is a *client* crate (it ships transports that speak TO MCP
//! servers); it does not export an `McpServer` framework. The spike therefore
//! builds a small request loop directly on top of the protocol types we
//! re-export (`JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError`,
//! `ServerInfo`, `InitializeResult`, `ServerCapabilities`, `ToolsCapability`,
//! `Tool`, `ToolContent`, `ContentBlock`, `ToolAnnotations`) plus the
//! [`ServerTool`] trait implemented by every SaaS module.
//!
//! Each SaaS exposes its own `McpTool` trait (in `github::tools` and
//! `slack::tools`); we erase the type here via [`ServerTool`] so the JSON-RPC
//! loop can dispatch by trait object without caring which SaaS it serves.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader};
use tracing::{debug, error, info};

use shannon_mcp::McpError;
use shannon_mcp::protocol::{
    ContentBlock, InitializeResult, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    ServerCapabilities, ServerInfo, Tool, ToolAnnotations, ToolContent, ToolsCapability,
};

/// Transport-level tool contract. Each SaaS implements this for its
/// `Box<dyn McpTool>` instances so the JSON-RPC loop can dispatch
/// without knowing the SaaS.
#[async_trait]
pub trait ServerTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    fn is_read_only(&self) -> bool;
    fn is_destructive(&self) -> bool;
    fn is_idempotent(&self) -> bool;
    fn is_open_world(&self) -> bool;
    fn required_permission(&self) -> &'static str;
    async fn execute(&self, args: Value) -> Result<Value, McpError>;
}

/// Server identity sent in `initialize` response.
const SERVER_NAME: &str = "shannon-mcp-saas";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run the MCP server over stdio. Reads newline-delimited JSON-RPC from
/// stdin, writes responses to stdout. Exits cleanly when stdin closes.
pub async fn run_stdio(tools: &[Box<dyn ServerTool>]) -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = TokioBufReader::new(stdin);

    // Auto-grant every tool whose `required_permission()` is "read" so the
    // host doesn't have to call `tools/grant` for safe-by-default operations.
    // Write tools start *denied* — the host must call `tools/grant` after the
    // user has approved the scope. This makes the gate server-side instead of
    // self-attested via `args.permission` (which the LLM could otherwise forge).
    let grants = SessionGrants::with_read_only_defaults(tools);

    info!(tools = tools.len(), "shannon-mcp-saas stdio server ready");

    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf).await?;
        if n == 0 {
            debug!("stdin closed; exiting stdio loop");
            return Ok(());
        }
        let line = buf.trim();
        if line.is_empty() {
            continue;
        }
        let response = handle_line(line, tools, &grants);
        match response {
            Some(resp) => {
                let serialized = serde_json::to_string(&resp).expect("response serializes");
                stdout.write_all(serialized.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
            None => {
                // Notification — no response per JSON-RPC 2.0.
            }
        }
    }
}

/// Per-session grant table: which (tool, scope) pairs the host has
/// authorized. Replaces the previous self-attested `args.permission`
/// gate (which an LLM could trivially forge) with a server-side
/// capability table populated only by the host through the
/// `tools/grant` JSON-RPC method.
#[derive(Debug, Default)]
pub struct SessionGrants {
    inner: Mutex<GrantState>,
}

#[derive(Debug, Default)]
struct GrantState {
    /// Tool name -> set of granted scopes (typically "read" or "write").
    granted: HashMap<String, HashSet<String>>,
}

impl SessionGrants {
    /// Pre-grant read scope for every tool whose declared permission is "read",
    /// leave write tools denied until the host calls `tools/grant`. This
    /// preserves the previous behavior for read tools while moving the write
    /// gate server-side.
    pub fn with_read_only_defaults(tools: &[Box<dyn ServerTool>]) -> Self {
        let mut state = GrantState::default();
        for tool in tools {
            if tool.required_permission() == "read" {
                state
                    .granted
                    .entry(tool.name().to_string())
                    .or_default()
                    .insert("read".to_string());
            }
        }
        Self {
            inner: Mutex::new(state),
        }
    }

    pub fn grant(&self, tool: &str, scope: &str) {
        let mut state = self.inner.lock().expect("grants poisoned");
        state
            .granted
            .entry(tool.to_string())
            .or_default()
            .insert(scope.to_string());
    }

    pub fn revoke(&self, tool: &str) {
        let mut state = self.inner.lock().expect("grants poisoned");
        state.granted.remove(tool);
    }

    pub fn check(&self, tool: &str, scope: &str) -> bool {
        let state = self.inner.lock().expect("grants poisoned");
        state
            .granted
            .get(tool)
            .map(|scopes| scopes.contains(scope))
            .unwrap_or(false)
    }
}

/// Synchronous helper used by the async loop above (and by tests).
/// Returns `None` for notifications (no response).
pub fn handle_line(
    line: &str,
    tools: &[Box<dyn ServerTool>],
    grants: &SessionGrants,
) -> Option<JsonRpcResponse> {
    let parsed: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            error!(error = %e, "invalid JSON on stdin");
            return Some(JsonRpcResponse::error("null", JsonRpcError::parse_error()));
        }
    };

    // JSON-RPC id may be string, number, or null. We coerce to String for
    // `JsonRpcResponse::ok` / `JsonRpcResponse::error`, both of which take
    // `impl Into<String>`. Notifications omit the `id` field entirely.
    let id_string: String = match parsed.get("id") {
        None => "null".to_string(),
        Some(serde_json::Value::Null) => "null".to_string(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
    };

    if parsed.get("method").is_none() {
        return Some(JsonRpcResponse::error(
            id_string,
            JsonRpcError::invalid_request(),
        ));
    }

    let has_request_id = parsed
        .as_object()
        .and_then(|o| o.get("id"))
        .map(|v| !v.is_null())
        .unwrap_or(false);
    if !has_request_id {
        debug!(
            method = parsed.get("method").and_then(|m| m.as_str()),
            "notification (no id)"
        );
        return None;
    }

    let request: JsonRpcRequest = match serde_json::from_value(parsed) {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "invalid JSON-RPC request");
            return Some(JsonRpcResponse::error(
                id_string,
                JsonRpcError::invalid_request(),
            ));
        }
    };

    Some(dispatch(&id_string, &request, tools, grants))
}

fn dispatch(
    id: &str,
    request: &JsonRpcRequest,
    tools: &[Box<dyn ServerTool>],
    grants: &SessionGrants,
) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(id),
        "tools/list" => handle_tools_list(id, tools),
        "tools/call" => handle_tools_call(id, request.params.clone(), tools, grants),
        "tools/grant" => handle_tools_grant(id, request.params.clone(), grants),
        "tools/revoke" => handle_tools_revoke(id, request.params.clone(), grants),
        "ping" => JsonRpcResponse::ok(id, json!({})),
        other => {
            debug!(method = other, "method not found");
            JsonRpcResponse::error(id, JsonRpcError::method_not_found())
        }
    }
}

/// Host control channel: grant a scope on a tool. Called by the Shannon
/// `PermissionRuleChecker` after the user has approved a write scope.
/// The grant lives for the rest of the MCP server's lifetime; revoke via
/// `tools/revoke`.
fn handle_tools_grant(id: &str, params: Option<Value>, grants: &SessionGrants) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params()),
    };
    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n,
        None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params()),
    };
    let scope = match params.get("scope").and_then(Value::as_str) {
        Some(s) => s,
        None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params()),
    };
    grants.grant(name, scope);
    JsonRpcResponse::ok(id, json!({ "granted": { "name": name, "scope": scope } }))
}

fn handle_tools_revoke(id: &str, params: Option<Value>, grants: &SessionGrants) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params()),
    };
    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n,
        None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params()),
    };
    grants.revoke(name);
    JsonRpcResponse::ok(id, json!({ "revoked": name }))
}

fn handle_initialize(id: &str) -> JsonRpcResponse {
    let result = InitializeResult {
        protocol_version: shannon_mcp::MCP_PROTOCOL_VERSION.to_string(),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: false,
            }),
            ..Default::default()
        },
        server_info: Some(ServerInfo {
            name: SERVER_NAME.to_string(),
            version: SERVER_VERSION.to_string(),
        }),
    };
    let value = serde_json::to_value(&result).expect("InitializeResult serializes");
    JsonRpcResponse::ok(id, value)
}

fn handle_tools_list(id: &str, tools: &[Box<dyn ServerTool>]) -> JsonRpcResponse {
    let tool_defs: Vec<Tool> = tools
        .iter()
        .map(|t| Tool {
            name: t.name().to_string(),
            description: t.description().to_string(),
            input_schema: Some(t.input_schema()),
            annotations: Some(ToolAnnotations {
                read_only_hint: t.is_read_only(),
                destructive_hint: t.is_destructive(),
                idempotent_hint: t.is_idempotent(),
                open_world_hint: t.is_open_world(),
            }),
        })
        .collect();
    JsonRpcResponse::ok(id, json!({ "tools": tool_defs }))
}

fn handle_tools_call(
    id: &str,
    params: Option<serde_json::Value>,
    tools: &[Box<dyn ServerTool>],
    grants: &SessionGrants,
) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params()),
    };
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params()),
    };
    let tool = match tools.iter().find(|t| t.name() == name) {
        Some(t) => t.as_ref(),
        None => {
            return JsonRpcResponse::ok(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": format!("tool not found: {name}")
                    }],
                    "isError": true,
                }),
            );
        }
    };

    // Server-side capability check. The previous gate read `args.permission`,
    // which the LLM could forge; we now require an explicit `tools/grant`
    // call from the host (which has gone through PermissionRuleChecker).
    let required = tool.required_permission();
    if !grants.check(&name, required) {
        return JsonRpcResponse::ok(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": format!("{required} scope not granted for tool {name}; host must call tools/grant first")
                }],
                "isError": true,
            }),
        );
    }

    // Strip any host-mis-injected `permission` field from the arguments before
    // forwarding to the tool, so the tool never sees it (defence in depth:
    // even if the tool still has a stale `require_write` check, it'll be
    // looking at a stripped payload).
    let mut arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    if let Some(obj) = arguments.as_object_mut() {
        obj.remove("permission");
    }

    let content = match futures_like_block_on(tool.execute(arguments)) {
        Ok(value) => ToolContent {
            content: vec![ContentBlock::Text {
                text: serde_json::to_string(&value).unwrap_or_else(|_| value.to_string()),
            }],
            is_error: Some(false),
        },
        Err(err) => ToolContent {
            content: vec![ContentBlock::Text {
                text: format!("{err}"),
            }],
            is_error: Some(true),
        },
    };

    let value = serde_json::to_value(&content).expect("ToolContent serializes");
    JsonRpcResponse::ok(id, value)
}

/// Execute a future on a single-threaded runtime when called from a sync
/// context (e.g. tests). When already inside a multi-thread runtime
/// (production `main`) we use `block_in_place` to avoid starving workers.
fn futures_like_block_on<F: std::future::Future>(fut: F) -> F::Output {
    if tokio::runtime::Handle::try_current().is_ok() {
        let handle = tokio::runtime::Handle::current();
        return tokio::task::block_in_place(|| handle.block_on(fut));
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build test runtime");
    rt.block_on(fut)
}

/// Print the registered tool names to **stderr**, one per line. Used by
/// `main` before entering the stdio loop so operators can verify what
/// the binary will advertise. **Stderr only** — stdout is reserved for
/// the JSON-RPC stream and any text written there would corrupt the
/// MCP protocol framing.
pub fn print_tool_listing(tools: &[Box<dyn ServerTool>]) {
    let mut out = std::io::stderr().lock();
    let _ = writeln!(out, "shannon-mcp-saas registered tools:");
    for t in tools {
        let _ = writeln!(out, "  {}", t.name());
    }
    let _ = out.flush();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::tools::all_tools_unauth;

    fn fresh_tools() -> Vec<Box<dyn ServerTool>> {
        crate::github::tools::as_server_tool(all_tools_unauth())
    }

    fn fresh_grants(tools: &[Box<dyn ServerTool>]) -> SessionGrants {
        SessionGrants::with_read_only_defaults(tools)
    }

    #[test]
    fn handle_initialize_returns_protocol_version() {
        let tools = fresh_tools();
        let grants = fresh_grants(&tools);
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":"1","method":"initialize","params":{}}"#,
            &tools,
            &grants,
        )
        .expect("response");
        assert_eq!(resp.id, "1");
        let value = resp.result.expect("result");
        assert_eq!(
            value["protocolVersion"],
            serde_json::json!(shannon_mcp::MCP_PROTOCOL_VERSION)
        );
        assert_eq!(value["serverInfo"]["name"], serde_json::json!(SERVER_NAME));
    }

    #[test]
    fn handle_tools_list_advertises_six_github_tools() {
        let tools = fresh_tools();
        let grants = fresh_grants(&tools);
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":"2","method":"tools/list","params":{}}"#,
            &tools,
            &grants,
        )
        .expect("response");
        let tool_list = resp.result.expect("result")["tools"]
            .as_array()
            .expect("array")
            .clone();
        let names: Vec<String> = tool_list
            .into_iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "github_list_issues",
                "github_get_issue",
                "github_create_issue",
                "github_comment",
                "github_list_prs",
                "github_review_pr",
            ]
        );
    }

    /// Security regression: a write tool invocation must be denied unless
    /// the host has called `tools/grant` first, even if the LLM-supplied
    /// `args.permission` field is set to "write" (which used to bypass the
    /// old self-attested gate).
    #[test]
    fn write_tool_without_grant_is_denied_even_if_args_permission_set() {
        let tools = fresh_tools();
        let grants = fresh_grants(&tools);
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":"3","method":"tools/call","params":{"name":"github_create_issue","arguments":{"owner":"o","repo":"r","title":"t","permission":"write"}}}"#,
            &tools,
            &grants,
        )
        .expect("response");
        let result = resp.result.expect("result");
        assert_eq!(result["isError"], serde_json::json!(true));
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        assert!(
            text.contains("write scope not granted"),
            "denial text should explain the missing grant; got: {text}"
        );
    }

    #[test]
    fn write_tool_succeeds_after_tools_grant() {
        let tools = fresh_tools();
        let grants = fresh_grants(&tools);

        // Grant write on github_create_issue.
        let grant_resp = handle_line(
            r#"{"jsonrpc":"2.0","id":"g1","method":"tools/grant","params":{"name":"github_create_issue","scope":"write"}}"#,
            &tools,
            &grants,
        )
        .expect("response");
        assert_eq!(
            grant_resp.result.expect("result")["granted"]["name"],
            "github_create_issue"
        );

        // The call should now pass the gate. It will still fail upstream
        // (no real token), but it should not be denied for missing scope.
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":"4","method":"tools/call","params":{"name":"github_create_issue","arguments":{"owner":"o","repo":"r","title":"t"}}}"#,
            &tools,
            &grants,
        )
        .expect("response");
        let result = resp.result.expect("result");
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        assert!(
            !text.contains("write scope not granted"),
            "after grant, call must not be denied for missing scope; got: {text}"
        );
    }

    #[test]
    fn revoke_unsets_a_grant() {
        let tools = fresh_tools();
        let grants = fresh_grants(&tools);
        grants.grant("github_create_issue", "write");
        assert!(grants.check("github_create_issue", "write"));
        grants.revoke("github_create_issue");
        assert!(!grants.check("github_create_issue", "write"));
    }

    #[test]
    fn read_only_defaults_pre_grant_read_tools() {
        let tools = fresh_tools();
        let grants = fresh_grants(&tools);
        // github_list_issues declares required_permission "read" and should
        // be auto-granted so the host doesn't have to call tools/grant for
        // every safe-by-default operation.
        assert!(grants.check("github_list_issues", "read"));
        // Write tools are denied at startup.
        assert!(!grants.check("github_create_issue", "write"));
    }
}
