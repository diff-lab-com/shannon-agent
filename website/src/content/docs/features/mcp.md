---
title: MCP Extensions
order: 32
section: features
---

# MCP Extensions

MCP (Model Context Protocol) is the primary extensibility mechanism. It's compatible with Claude Code's MCP implementation.

## How It Works

1. Configure MCP servers in `.mcp.json` or settings files
2. Shannon discovers available tools via `tools/list`
3. Tools are registered in the tool registry alongside built-in tools
4. The LLM can use MCP tools just like built-in ones

## Server Configuration

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
    },
    "database": {
      "command": "python",
      "args": ["db_mcp_server.py"],
      "env": { "DB_URL": "postgresql://..." }
    }
  }
}
```

## Transport

- **stdio** (default) — Launch server as subprocess, communicate via stdin/stdout
- **HTTP/SSE** — Connect to remote servers
- **WebSocket** — Bidirectional streaming

## Webhooks

Register webhooks to receive events when tools are called:

```
POST /webhook/register
{
  "url": "https://example.com/webhook",
  "events": ["ToolCallStarted", "ToolCallCompleted"],
  "secret": "hmac-signing-key"
}
```

Webhooks are signed with HMAC-SHA256 and delivered with exponential backoff retry.

## Resource Subscriptions

Subscribe to MCP resource updates:

```
resources/subscribe → { uri: "file:///path/to/watch" }
```

Receive `notifications/resources/updated` when resources change.

## Deferred Schema Loading

For servers with many tools, schema loading is deferred until the tool is actually called, reducing startup time. The threshold is configurable (default: 100 tools).
