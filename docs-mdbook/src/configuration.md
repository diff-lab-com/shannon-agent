# Configuration

Shannon Code uses a layered configuration system. Later sources override earlier ones:

**CLI args > Environment variables > Project config > Global config**

## Global Config

Path: `~/.shannon/config.toml`

```toml
provider = "anthropic"
api_key = "sk-ant-..."
model = "claude-sonnet-4"
max_tokens = 16384
temperature = 1.0
permissions_mode = "auto-allow"
```

## Project Config

Path: `.shannon.toml` (in project root)

Same format as global config. Project settings override global settings.

## Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `SHANNON_API_KEY` | API key for the LLM provider | `sk-ant-...` |
| `SHANNON_MODEL` | Model to use | `claude-sonnet-4` |
| `SHANNON_PROVIDER` | LLM provider | `anthropic`, `openai`, `ollama` |
| `SHANNON_BASE_URL` | Custom API base URL | `http://localhost:11434/v1` |
| `SHANNON_MAX_TOKENS` | Max response tokens | `16384` |
| `SHANNON_TEMPERATURE` | Response randomness (0-1) | `0.7` |
| `SHANNON_TIMEOUT` | Request timeout (seconds) | `120` |
| `SHANNON_DEBUG` | Enable debug logging | `true` |
| `SHANNON_PERMISSION_PROFILE` | Permission profile | `balanced` |

Fallback env vars: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`.

## CLI Flags

| Flag | Description |
|------|-------------|
| `--prompt <text>` | Run in headless/CI mode |
| `--resume [UUID]` | Resume a session |
| `--continue`, `-c` | Resume most recent session |
| `--model <name>` | Override model for this session |
| `--pipe` | Read stdin as prompt input |
| `--allowed-tools <list>` | Restrict available tools |
| `--max-turns <n>` | Limit conversation turns |
| `--diff-only` | Show only diffs, no chat |
| `--schema <file>` | Validate output against JSON Schema |
| `--yes` | Auto-approve all permissions |
| `--register-url-scheme` | Register `shannon://` URL handler |
| `--unregister-url-scheme` | Unregister URL handler |

## MCP Servers

MCP servers are configured in `.mcp.json`, `~/.claude/settings.json`, or `~/.shannon/settings.json`:

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

Tools are auto-discovered via `tools/list`.

## Permission Profiles

| Profile | Description |
|---------|-------------|
| `strict` | Approve all tool calls |
| `balanced` | Auto-approve reads, approve writes |
| `permissive` | Auto-approve non-destructive, deny destructive |
| `auto-allow` | Auto-approve everything except critical |

Set via config or `SHANNON_PERMISSION_PROFILE` env var.

## Usage Signals (Opt-in Analytics)

Shannon can report **anonymous aggregate counters** about product usage
(§4.15). The feature ships **disabled** and, even when enabled, transmits
counts only — never conversation content, tool arguments, file paths,
repository names, or session ids.

### Switches

| Variable | Description | Default |
|----------|-------------|---------|
| `SHANNON_SIGNALS_UPLOAD` | Enable outbound posting (`1`/`true`) | unset = off |
| `SHANNON_SIGNALS_ENDPOINT` | Target URL receiving the JSON payload | unset |
| `SHANNON_SIGNALS_SECRET` | Optional HMAC-SHA256 secret (`X-Shannon-Signature` header) | unset |

With all three unset the counters live only on your disk: every CLI exit
appends one aggregate line to `<home>/analytics/counters.jsonl`
(`$SHANNON_HOME` respected, default `~/.shannon`). No network client is ever
constructed while the switch is off.

### Data items (complete list)

| Counter | Meaning |
|---------|---------|
| `feedback_up` / `feedback_down` | `shannon feedback up|down` counts |
| `turns_ended` | turns reaching any terminal close |
| `turns_interrupted` | turns closed as interrupted (interruption rate) |
| `turns_user_takeover` | turns where a human answered a permission prompt (takeover rate) |
| `permission_prompts` | permission asks surfaced |
| `rewind_conversation` / `rewind_code` / `rewind_both` / `rewind_file` | `/rewind` usage by kind |

The wire payload adds only four metadata fields:
`schema`, `app_version`, `generated_at_utc`, `period_day_utc`.

### Commands

```bash
shannon feedback up      # +1 to feedback_up, flush per switches
shannon signals status   # print counters, rates and switch state (local only)
shannon signals push     # flush now; upload only when opted in
```

### Version trend dashboard

```bash
cargo run -p shannon-core --example eval_runner    # produce runs/
cargo run -p shannon-core --example signals_dashboard
# open $SHANNON_HOME/eval/dashboard.html
```

The board is a static, offline page (inline CSS; no scripts, no external
references): a version×metric comparison matrix plus the chronological run
sequence over `runs/<run-id>/report.json`.
