//! MCP completion/complete integration for REPL Tab autocomplete.
//!
//! Detects when the user is typing an argument to an MCP prompt slash command
//! (`/server:prompt <partial>` or `/mcp__server__prompt <partial>`) and queries
//! the originating server's `completion/complete` endpoint for suggestions.

use shannon_mcp::McpProcessPool;

/// Maximum time to wait for a server's completion response before giving up
/// and falling back to local completion logic.
const COMPLETION_TIMEOUT_MS: u64 = 800;

/// Parsed context for an MCP prompt argument completion request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCompletionContext {
    pub server: String,
    pub prompt: String,
    pub argument_name: String,
    pub argument_value: String,
}

/// Detect an MCP prompt invocation with a partial argument in the input line.
///
/// Returns `Some(ctx)` only when:
/// - Input starts with `/`
/// - The command matches `/mcp__{server}__{prompt}` or `/{server}:{prompt}`
/// - The command is registered in `command_names` (guards against false
///   positives like `/config:set`)
/// - A non-empty argument value follows the command
pub fn detect_mcp_prompt_context(
    input: &str,
    command_names: &[String],
    arg_names: &[String],
) -> Option<McpCompletionContext> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') {
        return None;
    }

    let mut split = trimmed.splitn(2, char::is_whitespace);
    let cmd = split.next()?;
    let arg_value = split.next()?.trim_start();
    if arg_value.is_empty() {
        return None;
    }

    let (server, prompt) = parse_mcp_prompt_command(cmd)?;

    let full_name = format!("mcp__{server}__{prompt}");
    if !command_names.iter().any(|n| n == &full_name) {
        return None;
    }

    let argument_name = arg_names.first().cloned().unwrap_or_default();

    Some(McpCompletionContext {
        server,
        prompt,
        argument_name,
        argument_value: arg_value.to_string(),
    })
}

/// Parse `/mcp__{server}__{prompt}` or `/{server}:{prompt}` into `(server, prompt)`.
fn parse_mcp_prompt_command(cmd: &str) -> Option<(String, String)> {
    let cmd = cmd.strip_prefix('/')?;
    if let Some(rest) = cmd.strip_prefix("mcp__") {
        let mut parts = rest.splitn(2, "__");
        let server = parts.next().filter(|s| !s.is_empty())?;
        let prompt = parts.next().filter(|s| !s.is_empty())?;
        return Some((server.to_string(), prompt.to_string()));
    }
    let mut parts = cmd.splitn(2, ':');
    let server = parts.next().filter(|s| !s.is_empty())?;
    let prompt = parts.next().filter(|s| !s.is_empty())?;
    if server.contains(' ') || prompt.contains(' ') {
        return None;
    }
    Some((server.to_string(), prompt.to_string()))
}

/// Fetch completion suggestions from an MCP server.
///
/// Returns an empty vec on timeout, transport error, or deserialization
/// failure — the caller falls back to regular completion logic silently.
pub async fn fetch_completion(pool: &McpProcessPool, ctx: &McpCompletionContext) -> Vec<String> {
    let fut = pool.complete(
        &ctx.server,
        "ref/prompt",
        None,
        Some(&ctx.prompt),
        &ctx.argument_name,
        &ctx.argument_value,
    );
    match tokio::time::timeout(std::time::Duration::from_millis(COMPLETION_TIMEOUT_MS), fut).await {
        Ok(Ok(result)) => result
            .completion
            .values
            .into_iter()
            .map(|v| v.value)
            .collect(),
        Ok(Err(e)) => {
            tracing::debug!(server = %ctx.server, error = %e, "MCP completion request failed");
            Vec::new()
        }
        Err(_) => {
            tracing::debug!(server = %ctx.server, "MCP completion timed out");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: &[&str] = &["mcp__reviewer__code-review", "mcp__writer__summarize"];

    fn names() -> Vec<String> {
        NAMES.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_alias_format() {
        assert_eq!(
            parse_mcp_prompt_command("/reviewer:code-review"),
            Some(("reviewer".to_string(), "code-review".to_string()))
        );
    }

    #[test]
    fn parse_qualified_format() {
        assert_eq!(
            parse_mcp_prompt_command("/mcp__reviewer__code-review"),
            Some(("reviewer".to_string(), "code-review".to_string()))
        );
    }

    #[test]
    fn parse_rejects_non_mcp_commands() {
        assert_eq!(parse_mcp_prompt_command("/help"), None);
        assert_eq!(parse_mcp_prompt_command("reviewer:code-review"), None);
        assert_eq!(parse_mcp_prompt_command("/server:"), None);
        assert_eq!(parse_mcp_prompt_command("/:prompt"), None);
        assert_eq!(parse_mcp_prompt_command("/mcp__server"), None);
    }

    #[test]
    fn detect_context_with_argument() {
        let ctx = detect_mcp_prompt_context(
            "/reviewer:code-review src/main.rs",
            &names(),
            &["file".to_string()],
        );
        let ctx = ctx.expect("should detect context");
        assert_eq!(ctx.server, "reviewer");
        assert_eq!(ctx.prompt, "code-review");
        assert_eq!(ctx.argument_name, "file");
        assert_eq!(ctx.argument_value, "src/main.rs");
    }

    #[test]
    fn detect_context_qualified_format() {
        let ctx = detect_mcp_prompt_context(
            "/mcp__writer__summarize hello world",
            &names(),
            &["text".to_string()],
        );
        let ctx = ctx.expect("should detect qualified context");
        assert_eq!(ctx.server, "writer");
        assert_eq!(ctx.prompt, "summarize");
        assert_eq!(ctx.argument_value, "hello world");
    }

    #[test]
    fn detect_context_rejects_unregistered_command() {
        // /config:set looks like server:prompt but isn't a registered MCP prompt
        let ctx = detect_mcp_prompt_context("/config:set value", &names(), &[]);
        assert!(ctx.is_none());
    }

    #[test]
    fn detect_context_no_argument_returns_none() {
        let ctx = detect_mcp_prompt_context("/reviewer:code-review", &names(), &[]);
        assert!(ctx.is_none());
    }

    #[test]
    fn detect_context_empty_argument_returns_none() {
        let ctx = detect_mcp_prompt_context("/reviewer:code-review   ", &names(), &[]);
        assert!(ctx.is_none());
    }

    #[test]
    fn detect_context_uses_first_arg_name() {
        let ctx = detect_mcp_prompt_context(
            "/reviewer:code-review value",
            &names(),
            &["file".to_string(), "line".to_string()],
        );
        assert_eq!(ctx.unwrap().argument_name, "file");
    }

    #[test]
    fn detect_context_no_arg_names_defaults_to_empty() {
        let ctx = detect_mcp_prompt_context("/reviewer:code-review value", &names(), &[]);
        assert_eq!(ctx.unwrap().argument_name, "");
    }
}
