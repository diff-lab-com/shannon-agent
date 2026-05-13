# Model Configuration Research: Claude Code, OpenAI Codex CLI, and OpenCode/Crush

**Date**: 2026-05-13
**Purpose**: Comparative analysis of how three leading AI coding assistants configure models, with actionable recommendations for Shannon.

---

## Executive Summary

All three tools solve model configuration differently based on their design philosophy:

- **Claude Code** (Anthropic) is Anthropic-centric with deep model aliasing, effort levels, and third-party deployment pinning. Single provider family with Bedrock/Vertex/Foundry routing.
- **OpenAI Codex CLI** (OpenAI) uses TOML config with a pluggable provider system. Built-in OpenAI + OSS (Ollama/LM Studio) providers, with custom providers for anything else.
- **OpenCode/Crush** (Charm) uses JSON config with 75+ providers via the AI SDK. The most provider-diverse of the three, with interactive `/connect` and `/models` commands.

Shannon already has 18+ providers via its `LlmProvider` enum and adapter pattern. The key gaps relative to these tools are: model aliasing, provider-specific config sections, profile-based presets, and runtime model switching.

---

## 1. Claude Code (Anthropic)

### 1.1 CLI Flags for Model Selection

| Flag | Description | Example |
|------|-------------|---------|
| `--model` | Set model for current session (alias or full name) | `claude --model opus` |
| `--fallback-model` | Fallback model when default is overloaded (print mode only) | `claude -p --fallback-model sonnet "query"` |
| `--effort` | Set reasoning effort level | `claude --effort high` |
| `--betas` | Beta headers for API requests | `claude --betas interleaved-thinking` |

### 1.2 Environment Variables

**Model Selection:**

| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_MODEL` | Model name or alias for the session |
| `ANTHROPIC_DEFAULT_OPUS_MODEL` | Override what the `opus` alias resolves to |
| `ANTHROPIC_DEFAULT_SONNET_MODEL` | Override what the `sonnet` alias resolves to |
| `ANTHROPIC_DEFAULT_HAIKU_MODEL` | Override what the `haiku` alias resolves to |
| `CLAUDE_CODE_SUBAGENT_MODEL` | Model used for subagents |
| `ANTHROPIC_CUSTOM_MODEL_OPTION` | Add a custom entry to the `/model` picker |

**API Keys and Endpoints:**

| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | API key (overrides subscription when set) |
| `ANTHROPIC_BASE_URL` | Override API endpoint for proxy/gateway routing |
| `ANTHROPIC_AUTH_TOKEN` | Custom Authorization header value |
| `ANTHROPIC_BEDROCK_BASE_URL` | Bedrock endpoint override |
| `ANTHROPIC_VERTEX_BASE_URL` | Vertex AI endpoint override |
| `ANTHROPIC_VERTEX_PROJECT_ID` | GCP project for Vertex AI |
| `ANTHROPIC_FOUNDRY_API_KEY` | Microsoft Foundry API key |
| `ANTHROPIC_FOUNDRY_BASE_URL` | Foundry base URL |
| `ANTHROPIC_AWS_API_KEY` | Claude Platform on AWS key |
| `ANTHROPIC_AWS_BASE_URL` | AWS endpoint override |

**Effort and Thinking:**

| Variable | Purpose |
|----------|---------|
| `CLAUDE_CODE_EFFORT_LEVEL` | Set reasoning effort (low/medium/high/xhigh/max/auto) |
| `MAX_THINKING_TOKENS` | Fixed thinking budget (when adaptive thinking disabled) |

### 1.3 Config File Format and Location

**Settings files** (JSON):

| Scope | Location | Shared? |
|-------|----------|---------|
| User | `~/.claude/settings.json` | No |
| Project | `.claude/settings.json` | Yes (committed) |
| Local | `.claude/settings.local.json` | No (gitignored) |
| Managed | System-level policies | Yes (IT deployed) |

**Settings file model configuration:**

```json
{
  "model": "opus",
  "effortLevel": "high",
  "availableModels": ["sonnet", "haiku"],
  "modelOverrides": {
    "claude-opus-4-7": "arn:aws:bedrock:us-east-2:123:inference-profile/opus-prod",
    "claude-sonnet-4-6": "arn:aws:bedrock:us-east-2:123:inference-profile/sonnet-prod"
  },
  "env": {
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4-7",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-6"
  }
}
```

**MCP servers** stored separately in `~/.claude.json` (user) and `.mcp.json` (project).

### 1.4 Model Aliasing System

Claude Code has the most sophisticated aliasing system of the three tools:

| Alias | Behavior |
|-------|----------|
| `default` | Clears override, reverts to tier default |
| `best` | Most capable model (currently = opus) |
| `sonnet` | Latest Sonnet |
| `opus` | Latest Opus |
| `haiku` | Latest Haiku |
| `sonnet[1m]` | Sonnet with 1M token context |
| `opus[1m]` | Opus with 1M token context |
| `opusplan` | Opus for planning, Sonnet for execution |

Aliases are provider-aware: on Anthropic API `opus` = Opus 4.7, on Bedrock/Vertex it = Opus 4.6 unless pinned via env vars.

### 1.5 Multi-Provider Support

Claude Code is Anthropic-only at the model level but supports multiple **deployment channels**:

- **Anthropic API** (direct)
- **Amazon Bedrock** (via `ANTHROPIC_BEDROCK_BASE_URL`)
- **Google Vertex AI** (via `ANTHROPIC_VERTEX_*`)
- **Microsoft Foundry** (via `ANTHROPIC_FOUNDRY_*`)
- **Claude Platform on AWS** (via `ANTHROPIC_AWS_*`)
- **LLM Gateways** (via `ANTHROPIC_BASE_URL`)

### 1.6 Runtime Model Switching

- `/model` command opens interactive picker
- `/model <alias|name>` switches immediately
- `/effort` command with interactive slider
- Selection persists to user settings across restarts

---

## 2. OpenAI Codex CLI

### 2.1 CLI Flags for Model Selection

| Flag | Description | Example |
|------|-------------|---------|
| `--model`, `-m` | Override model for this session | `codex -m gpt-5.5` |
| `--oss` | Use local Ollama/LM Studio provider | `codex --oss` |
| `--config`, `-c` | Override arbitrary config key | `codex -c model='"gpt-5.4"'` |
| `--profile`, `-p` | Load named config profile | `codex --profile deep-review` |
| `--search` | Enable live web search | `codex --search` |

### 2.2 Environment Variables

Codex CLI uses fewer environment variables than Claude Code, relying more on `config.toml`:

| Variable | Purpose |
|----------|---------|
| `OPENAI_API_KEY` | OpenAI API key |
| `CODEX_HOME` | Config directory (default `~/.codex`) |
| `<PROVIDER_ENV_KEY>` | Per-provider API key (defined in config) |

### 2.3 Config File Format and Location

**TOML format** (a key differentiator from the other tools):

| Scope | Location | Precedence |
|-------|----------|------------|
| CLI flags | Command line | Highest |
| Profile | `~/.codex/config.toml` `[profiles.<name>]` | |
| Project | `.codex/config.toml` (closest wins) | |
| User | `~/.codex/config.toml` | |
| System | `/etc/codex/config.toml` | |
| Defaults | Built-in | Lowest |

**Example `~/.codex/config.toml`:**

```toml
model = "gpt-5.5"
model_provider = "openai"
approval_policy = "on-request"
sandbox_mode = "workspace-write"
model_reasoning_effort = "high"

# Built-in provider URL override
openai_base_url = "https://us.api.openai.com/v1"

# Custom provider definitions
[model_providers.proxy]
name = "OpenAI using LLM proxy"
base_url = "http://proxy.example.com"
env_key = "OPENAI_API_KEY"

[model_providers.local_ollama]
name = "Ollama"
base_url = "http://localhost:11434/v1"

[model_providers.mistral]
name = "Mistral"
base_url = "https://api.mistral.ai/v1"
env_key = "MISTRAL_API_KEY"

[model_providers.azure]
name = "Azure"
base_url = "https://YOUR_PROJECT.openai.azure.com/openai"
env_key = "AZURE_OPENAI_API_KEY"
query_params = { api-version = "2025-04-01-preview" }
wire_api = "responses"

# Named profiles for quick switching
[profiles.deep-review]
model = "gpt-5-pro"
model_reasoning_effort = "high"
approval_policy = "never"

[profiles.lightweight]
model = "gpt-4.1"
approval_policy = "untrusted"
```

### 2.4 Multi-Provider Support

Codex has three built-in provider IDs that cannot be overridden:

| Provider ID | Description |
|-------------|-------------|
| `openai` | OpenAI Responses API (default) |
| `ollama` | Ollama local inference |
| `lmstudio` | LM Studio local inference |

Any number of additional providers can be defined via `[model_providers.<id>]` sections. Each provider specifies:

- `name` - Display name
- `base_url` - API endpoint
- `env_key` - Environment variable name for API key
- `wire_api` - Protocol format (`"responses"` or `"chat"`)
- `http_headers` - Static headers
- `env_http_headers` - Headers from environment variables
- `auth` - Command-backed authentication (for token refresh)
- `query_params` - URL query parameters
- `request_max_retries` / `stream_max_retries` / `stream_idle_timeout_ms` - Network tuning

Special providers:
- `amazon-bedrock` (built-in, with `[model_providers.amazon-bedrock.aws]` for profile/region)
- `azure` (custom provider with `wire_api = "responses"`)

### 2.5 Profile System

A distinctive feature of Codex CLI. Profiles allow switching between complete configuration sets:

```toml
# Default profile
model = "gpt-5.4"
model_provider = "openai"

[profiles.deep-review]
model = "gpt-5-pro"
model_reasoning_effort = "high"
model_catalog_json = "/Users/me/.codex/model-catalogs/deep-review.json"

[profiles.local]
model = "gpt-oss:120b"
model_provider = "ollama"
```

Usage: `codex --profile deep-review`

### 2.6 Model Catalog

Codex supports loading external model catalogs via `model_catalog_json`, which can be overridden per profile. This allows teams to share curated model lists.

---

## 3. OpenCode / Crush

### 3.1 CLI Flags for Model Selection

| Flag | Description | Example |
|------|-------------|---------|
| `-m`, `--model` | Set model as `provider_id/model_id` | `opencode -m anthropic/claude-sonnet-4` |
| `-p` | Non-interactive prompt mode | `opencode -p "query"` |
| `-f` | Output format (text/json) | `opencode -p "query" -f json` |
| `-c` | Working directory | `opencode -c /path/to/project` |

### 3.2 Environment Variables

OpenCode uses per-provider environment variables:

| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | Claude models |
| `OPENAI_API_KEY` | OpenAI models |
| `GEMINI_API_KEY` | Google Gemini models |
| `GITHUB_TOKEN` | GitHub Copilot models |
| `GROQ_API_KEY` | Groq models |
| `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_REGION` | AWS Bedrock |
| `AZURE_OPENAI_ENDPOINT` / `AZURE_OPENAI_API_KEY` | Azure OpenAI |
| `VERTEXAI_PROJECT` / `VERTEXAI_LOCATION` | Google VertexAI |
| `LOCAL_ENDPOINT` | Self-hosted models |
| `DEEPSEEK_API_KEY` | DeepSeek |
| `MISTRAL_API_KEY` | Mistral |
| And 60+ more provider-specific keys... | |

### 3.3 Config File Format and Location

**JSON format**, with a JSON Schema for editor validation:

| Location | Description |
|----------|-------------|
| `$HOME/.opencode.json` | User global |
| `$XDG_CONFIG_HOME/opencode/.opencode.json` | XDG config |
| `./.opencode.json` | Project local |

Credentials stored separately in `~/.local/share/opencode/auth.json`.

**Example config:**

```json
{
  "$schema": "https://opencode.ai/config.json",
  "model": "anthropic/claude-sonnet-4",
  "provider": {
    "anthropic": {
      "options": {
        "baseURL": "https://api.anthropic.com/v1"
      },
      "models": {
        "claude-sonnet-4-5-20250929": {
          "options": {
            "thinking": {
              "type": "enabled",
              "budgetTokens": 16000
            }
          },
          "variants": {
            "high": {
              "reasoningEffort": "high",
              "textVerbosity": "low"
            },
            "fast": {
              "disabled": true
            }
          }
        }
      }
    },
    "openai": {
      "models": {
        "gpt-5": {
          "options": {
            "reasoningEffort": "high",
            "textVerbosity": "low"
          }
        }
      }
    },
    "atomic-chat": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Atomic Chat (local)",
      "options": {
        "baseURL": "http://127.0.0.1:1337/v1"
      },
      "models": {
        "<model-id>": {
          "name": "My Local Model"
        }
      }
    }
  },
  "agents": {
    "coder": {
      "model": "anthropic/claude-sonnet-4",
      "maxTokens": 5000
    },
    "task": {
      "model": "anthropic/claude-sonnet-4",
      "maxTokens": 5000
    },
    "title": {
      "model": "anthropic/claude-sonnet-4",
      "maxTokens": 80
    }
  },
  "autoCompact": true,
  "mcpServers": { ... },
  "lsp": { ... }
}
```

### 3.4 Multi-Provider Support

OpenCode has the broadest provider support (75+), powered by the AI SDK and Models.dev:

- **Built-in providers**: Anthropic, OpenAI, Google Gemini, AWS Bedrock, Azure OpenAI, Groq, GitHub Copilot, OpenRouter, DeepSeek, Mistral, and many more
- **Custom providers**: Define any OpenAI-compatible provider with `npm` package, `baseURL`, and model list
- **Local models**: Ollama, LM Studio, Atomic Chat, any OpenAI-compatible endpoint

Provider model format: `provider_id/model_id` (e.g., `anthropic/claude-sonnet-4`).

### 3.5 Model Variants

OpenCode has a variant system for the same model with different settings:

- **Built-in variants**: Anthropic has `high`/`max` thinking, OpenAI has `none` through `xhigh` reasoning
- **Custom variants**: User-defined variants per model
- **Variant cycling**: Keybind to switch between variants quickly

### 3.6 Interactive Setup

- `/connect` command: Interactive provider credential setup with OAuth support
- `/models` command: Interactive model picker with provider grouping
- Credentials stored in `~/.local/share/opencode/auth.json`

### 3.7 Note: Project Archival

The original `opencode-ai/opencode` GitHub repository has been archived. The project continues as **Crush** (`charmbracelet/crush`), developed by the original author and the Charm team. The opencode.ai documentation remains current.

---

## 4. Comparison Table

| Feature | Claude Code | Codex CLI | OpenCode/Crush | Shannon (Current) |
|---------|-------------|-----------|----------------|-------------------|
| **Config format** | JSON | TOML | JSON | TOML |
| **CLI model flag** | `--model` | `--model`, `-m` | `-m` | `--model` |
| **CLI provider flag** | N/A (single family) | N/A (via config) | N/A (in model ID) | `--provider` |
| **Config precedence** | Managed > CLI > Local > Project > User | CLI > Profile > Project > User > System | CLI > Config > Last-used > Priority | CLI > Env > Local > Global |
| **Model aliasing** | sonnet, opus, haiku, best, opusplan, [1m] suffix | None built-in | Built-in + custom variants | None |
| **Profile system** | No | Yes (`[profiles.<name>]`) | No | No |
| **Provider count** | 1 family, 6 channels | 3 built-in + custom | 75+ built-in + custom | 18 enum variants |
| **API key env vars** | `ANTHROPIC_*` (20+ vars) | Per-provider `env_key` | Per-provider (60+ vars) | `SHANNON_API_KEY` |
| **Base URL override** | `ANTHROPIC_BASE_URL` | `openai_base_url` or custom provider | `options.baseURL` per provider | `SHANNON_BASE_URL` |
| **Runtime model switch** | `/model` command | No (restart needed) | `/models` command | No |
| **Effort/reasoning** | 5 levels + `ultrathink` keyword | `model_reasoning_effort` | Model variants | None |
| **Custom model entries** | `ANTHROPIC_CUSTOM_MODEL_OPTION` | `model_catalog_json` | Custom provider models | N/A |
| **Thinking budget** | Adaptive + fixed + toggle | `model_reasoning_summary` | Per-model options | None |
| **Subagent model** | `CLAUDE_CODE_SUBAGENT_MODEL` | Per-agent config | Per-agent config | None |
| **Enterprise controls** | `availableModels`, managed settings | `requirements.toml`, managed | N/A | N/A |
| **Auth methods** | OAuth, API key, OIDC, SigV4 | OAuth, API key, command-backed | OAuth, API key, `/connect` | API key |

---

## 5. Current Shannon Configuration

Shannon already has a solid foundation. Here is the current state:

### 5.1 Config Precedence

```
CLI args > env vars (SHANNON_*) > .shannon.toml (local) > ~/.shannon/config.toml (global)
```

### 5.2 CLI Flags

```
--model <name>      LLM model (e.g., claude-sonnet-4, gpt-4o)
--provider <name>   LLM provider (anthropic, openai, ollama, gemini, azure, bedrock, ...)
--local             Shortcut for --provider ollama with default model
```

### 5.3 Environment Variables

| Variable | Purpose |
|----------|---------|
| `SHANNON_MODEL` | Default model name |
| `SHANNON_PROVIDER` | Default provider |
| `SHANNON_API_KEY` | API key for the active provider |
| `SHANNON_BASE_URL` | Override base URL |
| `SHANNON_MAX_TOKENS` | Max response tokens |
| `SHANNON_TEMPERATURE` | Sampling temperature |
| `SHANNON_TIMEOUT` | Request timeout |
| `SHANNON_DEBUG` | Debug logging |

### 5.4 Providers (18 in LlmProvider enum)

Anthropic, OpenAI, Ollama, Custom, Gemini, Azure, Bedrock, Mistral, DeepSeek, Groq, Together, OpenRouter, Cohere, Fireworks, Perplexity, Xai, Ai21, Cloudflare, Replicate, SiliconFlow, Zhipu.

### 5.5 Config File Format (TOML)

```toml
# ~/.shannon/config.toml or .shannon.toml
model = "claude-sonnet-4"
provider = "anthropic"
```

### 5.6 What Shannon Lacks (Relative to Competitors)

1. **Model aliasing** (sonnet/opus/haiku shortcuts)
2. **Provider-specific config sections** (base_url, api_key per provider)
3. **Named profiles/presets** (quick switching between configurations)
4. **Runtime model switching** (no `/model` command)
5. **Effort/reasoning level configuration**
6. **Model variant support**
7. **Per-agent/subagent model configuration**
8. **Custom model entries in a picker**
9. **Provider credential management** (interactive `/connect` style)

---

## 6. Recommendations for Shannon

These recommendations are prioritized by impact and implementation complexity.

### 6.1 Provider-Specific Config Sections (HIGH IMPACT)

Allow configuring base_url, api_key, and default_model per provider, eliminating the need to switch env vars when changing providers.

**Proposed `.shannon.toml` format:**

```toml
model = "claude-sonnet-4"
provider = "anthropic"

[providers.anthropic]
api_key_env = "ANTHROPIC_API_KEY"
base_url = "https://api.anthropic.com"

[providers.anthropic.models]
# Optional per-provider model aliases
sonnet = "claude-sonnet-4-20250514"
opus = "claude-opus-4-20250115"

[providers.openai]
api_key_env = "OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"

[providers.ollama]
base_url = "http://localhost:11434"

[providers.custom]
base_url = "https://my-gateway.example.com"
api_key_env = "MY_GATEWAY_KEY"
wire_format = "openai"  # "anthropic", "openai", "gemini"
```

This follows Codex CLI's `[model_providers.<id>]` pattern and OpenCode's `"provider": {...}` pattern.

### 6.2 Model Aliasing (HIGH IMPACT)

Add shorthand aliases that resolve to full model names, following Claude Code's pattern.

**Proposed TOML:**

```toml
[aliases]
sonnet = "claude-sonnet-4-20250514"
opus = "claude-opus-4-20250115"
haiku = "claude-haiku-3-20240307"
gpt4 = "gpt-4o"
flash = "gemini-2.0-flash"
local = "llama3"
```

**Resolution order**: CLI arg > env var > alias > literal model name > provider default.

### 6.3 Named Profiles (MEDIUM IMPACT)

Follow Codex CLI's profile system for quick configuration switching.

```toml
[profiles.work]
model = "claude-sonnet-4"
provider = "anthropic"

[profiles.local]
model = "llama3"
provider = "ollama"

[profiles.review]
model = "gpt-4o"
provider = "openai"
max_tokens = 4096
temperature = 0.2
```

**Usage**: `shannon --profile local` or `SHANNON_PROFILE=local shannon`

### 6.4 Runtime Model Switching (MEDIUM IMPACT)

Add a `/model` command to the REPL (similar to Claude Code and OpenCode) that allows switching models and providers during a session without restart.

### 6.5 Per-Provider API Key Resolution (MEDIUM IMPACT)

Currently Shannon uses a single `SHANNON_API_KEY`. Add automatic key resolution per provider:

```
Provider = Anthropic -> check SHANNON_API_KEY, then ANTHROPIC_API_KEY
Provider = OpenAI    -> check SHANNON_API_KEY, then OPENAI_API_KEY
Provider = DeepSeek  -> check SHANNON_API_KEY, then DEEPSEEK_API_KEY
Provider = Groq      -> check SHANNON_API_KEY, then GROQ_API_KEY
```

This follows OpenCode's pattern and reduces configuration friction.

### 6.6 Wire Format Detection (LOW-MEDIUM IMPACT)

Add a `wire_format` field to provider config so custom endpoints can declare their protocol:

```toml
[providers.my-gateway]
base_url = "https://gateway.example.com"
wire_format = "openai"  # "anthropic" | "openai" | "gemini" | "ollama"
```

This follows Codex CLI's `wire_api` field and allows Shannon to correctly serialize/deserialize for any endpoint.

### 6.7 Effort/Reasoning Configuration (LOW IMPACT, future)

Add optional reasoning effort support for models that support it:

```toml
[reasoning]
effort = "high"  # Maps to reasoning_effort for OpenAI, budget_tokens for Anthropic
max_thinking_tokens = 16000
```

### 6.8 Implementation Priority

| Priority | Feature | Effort | Impact |
|----------|---------|--------|--------|
| 1 | Per-provider API key resolution | Small | High |
| 2 | Model aliasing | Small | High |
| 3 | Provider-specific config sections | Medium | High |
| 4 | Named profiles | Medium | Medium |
| 5 | Runtime model switching | Medium | Medium |
| 6 | Wire format field | Small | Medium |
| 7 | Effort/reasoning config | Small | Low |

Items 1-3 can be done in a single pass since they all modify the config loading logic. Item 4 builds on the same config infrastructure. Items 5-6 require changes to the REPL and adapter layer respectively.

---

## Sources

- Claude Code CLI reference: https://docs.anthropic.com/en/docs/claude-code/cli-usage
- Claude Code model configuration: https://docs.anthropic.com/en/docs/claude-code/model-config
- Claude Code environment variables: https://docs.anthropic.com/en/docs/claude-code/env-vars
- Claude Code settings: https://docs.anthropic.com/en/docs/claude-code/settings
- OpenAI Codex CLI reference: https://developers.openai.com/codex/cli/reference
- Codex config basics: https://developers.openai.com/codex/config-basic
- Codex config advanced: https://developers.openai.com/codex/config-advanced
- Codex config reference: https://developers.openai.com/codex/config-reference
- Ollama + Codex integration: https://docs.ollama.com/integrations/codex
- OpenCode GitHub (archived): https://github.com/opencode-ai/opencode
- OpenCode providers docs: https://opencode.ai/docs/providers/
- OpenCode models docs: https://opencode.ai/docs/models/
- Shannon source: `/home/ed/workspace/backup/shannon-code/crates/shannon-core/src/api/types.rs`
- Shannon CLI: `/home/ed/workspace/backup/shannon-code/crates/shannon-cli/src/main.rs`
