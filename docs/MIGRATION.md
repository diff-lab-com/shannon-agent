# Migrating from Plugin System to MCP

Shannon Code has unified on MCP (Model Context Protocol) as its single extension mechanism, removing the custom `plugin.json`-based plugin system. This guide helps existing plugin users migrate to the MCP format.

## Why the Change

- MCP is an industry standard supported by Claude Code, Cursor, and other tools
- Claude Code's plugin ecosystem can be used directly
- No need to maintain a Shannon-specific plugin format with no ecosystem

## Migration Steps

### 1. Convert `plugin.json` to MCP Server Config

**Before** (`~/.shannon/plugins/my-plugin/plugin.json`):
```json
{
  "name": "my-plugin",
  "version": "1.0.0",
  "tools": [{
    "name": "my_tool",
    "command": "python3 /path/to/tool.py",
    "input_schema": {"type": "object"},
    "description": "Does something useful"
  }],
  "hooks": {
    "PreToolUse": "python3 /path/to/hook.py"
  }
}
```

**After** (`~/.shannon/settings.json` or `.mcp.json`):
```json
{
  "mcpServers": {
    "my-plugin": {
      "command": "python3",
      "args": ["/path/to/server.py"],
      "env": {}
    }
  }
}
```

### 2. Convert Tool Scripts to MCP Servers

Your tool scripts need to become MCP servers that communicate via JSON-RPC over stdio.

**Before** (plugin tool — stdin/stdout JSON):
```python
# tool.py — reads JSON from stdin, writes JSON to stdout
import json, sys

input_data = json.load(sys.stdin)
result = do_something(input_data)
json.dump({"result": result}, sys.stdout)
```

**After** (MCP server — JSON-RPC protocol):
```python
# server.py — MCP server using the SDK
from mcp.server import Server
from mcp.types import Tool, TextContent

server = Server("my-plugin")

@server.list_tools()
async def list_tools():
    return [Tool(
        name="my_tool",
        description="Does something useful",
        inputSchema={"type": "object", "properties": {}}
    )]

@server.call_tool()
async def call_tool(name, arguments):
    result = do_something(arguments)
    return [TextContent(type="text", text=str(result))]
```

### 3. Convert Hooks to Config-Based Hooks

Hooks are configured in settings files, not in plugin manifests.

**Before** (plugin.json hooks):
```json
{
  "hooks": {
    "PreToolUse": "python3 /path/to/hook.py"
  }
}
```

**After** (`~/.shannon/settings.json` or `.claude/settings.json`):
```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "command": "python3 /path/to/hook.py",
            "timeout": 5
          }
        ]
      }
    ]
  }
}
```

### 4. Convert Custom Commands to MCP Prompts

Custom slash commands should become MCP prompts.

**Before** (plugin.json commands):
```json
{
  "commands": [{
    "name": "review",
    "prompt": "Review the following code for bugs: $ARGUMENTS"
  }]
}
```

**After** (MCP server with prompts):
```python
@server.list_prompts()
async def list_prompts():
    return [Prompt(
        name="review",
        description="Review code for bugs",
        arguments=[PromptArgument(name="code", required=True)]
    )]

@server.get_prompt()
async def get_prompt(name, arguments):
    return GetPromptResult(
        messages=[TextMessage(
            role="user",
            content=f"Review the following code for bugs: {arguments['code']}"
        )]
    )
```

This registers as `/mcp__my-plugin__review` in Shannon.

## Config File Locations

MCP server configs are discovered from (later files override earlier):

| Path | Scope |
|------|-------|
| `~/.claude/settings.json` | User-level (Claude Code compatible) |
| `~/.shannon/settings.json` | User-level (Shannon-specific) |
| `.mcp.json` | Project-level |
| `.claude/settings.json` | Project-level (Claude Code compatible) |
| `.shannon/settings.json` | Project-level (Shannon-specific) |

## Quick Reference: Available MCP SDKs

| Language | Package |
|----------|---------|
| Python | `pip install mcp` |
| TypeScript | `npm install @modelcontextprotocol/sdk` |
| Go | `go get github.com/mark3labs/mcp-go` |
| Rust | `cargo add rmcp` |

## Environment Variables

Environment variable expansion (`${VAR}`) is supported in `env`, `command`, `args`, and `url` fields:

```json
{
  "mcpServers": {
    "my-api": {
      "command": "npx",
      "args": ["-y", "my-mcp-server"],
      "env": {
        "API_KEY": "${MY_API_KEY}"
      }
    }
  }
}
```

## Need Help?

Run `/mcp help` in Shannon for available MCP management commands, including `/mcp add`, `/mcp remove`, `/mcp list`, and `/mcp reload`.
