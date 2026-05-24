# shannon-mcp

MCP (Model Context Protocol) integration — Claude Code compatible.

## Transport Layers

| Transport | Use Case |
|-----------|----------|
| stdio | Local process (default) |
| HTTP | Remote servers |
| SSE | Server-sent events |
| WebSocket | Bidirectional streaming |

## Key Components

### McpProcessPool
Manages persistent connections to MCP servers. Handles:
- Server lifecycle (start, stop, restart)
- Tool discovery via `tools/list`
- Resource subscriptions
- Progress tracking

### Webhook System (`webhook/`)
- `WebhookRegistry` — Register/unregister webhooks with HMAC-SHA256 signing
- `EventPublisher` — Non-blocking event delivery with exponential backoff retry
- Events: ToolCallStarted/Completed, ServerConnected/Disconnected, NotificationReceived

### Resource Subscriptions
- `ResourceSubscriptionManager` — Subscribe to resource updates per server/URI
- Callback dispatch on `notifications/resources/updated`

## Configuration

In `.mcp.json` or `~/.shannon/settings.json`:

```json
{
  "mcpServers": {
    "my-server": {
      "command": "npx",
      "args": ["-y", "my-mcp-server"],
      "env": { "API_KEY": "..." }
    }
  }
}
```

Tools are auto-discovered and registered in the tool registry.
