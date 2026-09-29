# Configuration Reference

Shannon is BYOK (bring your own key) and multi-provider. Configuration is layered: higher-priority sources override lower ones.

**One rule up front:** API keys are never written to a config file — not `config.toml`, not `.shannon.toml`, not `providers.toml`. Keys live in the credential store (`~/.shannon/credentials/<service>.json`, mode `0600`) or in an environment variable. A flat `api_key = "..."` line in any config file is **silently ignored by design**.

## Quick start

Three paths, pick one:

| Path | Steps |
|------|-------|
| TUI | Start `shannon`, then `/connect <provider> <your-api-key>` (e.g. `/connect anthropic sk-ant-...`). Shannon stores the key, probes it, switches to the provider's default model, and opens the model picker. `/connect ollama` needs no key. |
| Desktop | The Welcome wizard (pick a task → add a provider) or Settings → Models → Add Provider. Keys are entered in a write-only field and stored on disk with `0600` permissions. |
| Headless / scripts | `shannon providers add <id> --kind <kind> --model <model> [--base-url <url>]`, then export the provider's API key as an environment variable (see the [provider reference](#provider-reference)). The CLI never accepts a raw key. |

Per-provider walkthroughs (get a key → connect → verify → troubleshoot) live in [`docs/providers/`](providers/index.md).

## Where Shannon keeps state

| Location | Written by | Contents |
|----------|-----------|----------|
| `~/.shannon/providers.toml` | `/connect`, `/model --save`, `shannon providers add`, desktop | Active provider + model, per-provider profiles (base URL, credential *reference*, per-tier overrides, extra headers). Mode `0600`. Never contains a key. |
| `~/.shannon/credentials/<service>.json` | `/connect`, `/credentials`, desktop | One file per provider, holding the key. Mode `0600`, atomic writes. |
| `~/.shannon/config.toml` | `/config set`, hand-edited | Flat behavioral keys: `model`, `provider`, `base_url`, `max_tokens`, `temperature`, `timeout`, `debug`, `max_context_tokens`, `permission_profile`. (`[secret_guard]` has its own loader; full TOML tables are otherwise not read from this file.) |
| `.shannon.toml` (project root) | hand-edited | Same keys as `config.toml`, scoped to the project. |
| `~/.shannon/config.json` | `/config`, the agent `Config` tool | A key-value store for tooling. Only allowlisted keys are mirrored into `config.toml`. |

Auxiliary files: `preferences.json` (last model/provider/theme, written on every switch), and `~/.shannon/cache/` (`models-dev.json`, `litellm-prices.json` — 24h model/pricing caches).

## Precedence

Highest to lowest (`shannon --dump-config` prints this ladder with per-layer provenance):

1. **CLI flags** — `--model`, `--provider`, `--effort`
2. **`providers.toml`** (the "connected" layer written by `/connect`)
3. **Environment variables** (`SHANNON_*`)
4. **`.shannon.toml`** (project)
5. **`~/.shannon/config.toml`** (global)
6. Built-in defaults

A connected provider beats ambient `SHANNON_*` env vars (so `/connect` works with zero env vars), while a CLI flag still overrides a single invocation.

## Connecting a provider (TUI)

```
/connect                      list every provider + connection status
/connect <provider>           connect (prints the inline-key hint if no key is stored yet)
/connect <provider> <key>     connect with an inline key (recommended)
/disconnect <provider>        remove the saved connection (the stored key is kept)
```

`/connect <provider> <key>` does, in order:

1. Stores the key in `~/.shannon/credentials/<service>.json` (`0600`). The typed key is redacted to `***` in the chat transcript and command history.
2. Upserts the provider profile into `~/.shannon/providers.toml` (additive — connecting a second provider keeps the first).
3. Switches the engine to the provider's default model.
4. Probes the credential with a 1-token request. A hard `401` aborts the switch (`✗ Authentication failed for '<provider>'. The key was stored but the provider rejected it — check the key and run /connect again.`); any other probe error only warns (`⚠ Could not fully verify the credential ...`) and keeps the connection.
5. Hot-reloads the running client with the new key — no restart.
6. Spawns a background models.dev catalog refresh, then opens the model picker.

Dashboard status vocabulary (shared by `/connect`, `/provider`, and the welcome card; `*` marks the current provider):

| Status | Meaning |
|--------|---------|
| `no auth` | Provider needs no key (Ollama) |
| `✓ connected` | Profile persisted **and** key stored |
| `key stored` | Key on disk, but no persisted profile yet |
| `no key` | Auth required, nothing stored |

## Headless / CLI

```
shannon providers add <ID> --kind <kind> --model <model> [options]
shannon providers remove <ID>
shannon list-providers [--json]
shannon config                        list stored config keys
shannon config <key>                  print one key
shannon config <key>=<value>          set a key (writable keys are mirrored into config.toml)
shannon --dump-config
```

`providers add` options:

| Option | Meaning |
|--------|---------|
| `--kind` | `anthropic` \| `openai` \| `openai-compatible` \| `ollama` \| `gemini` \| `deepseek` |
| `--base-url <URL>` | Required for `openai-compatible` and `ollama`; otherwise defaults to the kind's canonical endpoint |
| `--tier <TIER>` | Pin the model to `fast` / `standard` / `pro` (aliases like `haiku` are rejected — canonical names only) |
| `--extra-header K=V` | Repeatable; sent on every request to this provider |
| `--api-key-ref <SERVICE>` | Credential service name (defaults to the provider id). A **reference**, never the secret |
| `--set-active` | Documented no-op: the added provider always becomes active |

There is intentionally no `--api-key <raw>` flag. For a fully headless setup, store the key as an environment variable and Shannon falls back to it when the credential store has no entry for the provider (see [Credentials](#credentials)). The added provider is persisted to `~/.shannon/providers.toml` and becomes the active target.

Other relevant flags: `--model <id|provider/model>`, `--provider <slug>`, `--effort <low|medium|standard|high|max>`, `--dump-config` (JSON snapshot of every config layer and the merged result).

## Desktop

- **Welcome wizard** — first run: pick a task type, then add a provider via the Add Provider modal. If a provider is already configured via environment variables, the wizard detects it and lets you skip.
- **Settings → Models** —
  - Performance strategy pills (Balanced / Speed / High Quality).
  - Active model card + searchable quick-switcher over the full catalog.
  - Provider management: add / edit / delete connections, **Test** per connection and **Test all** (non-billable probes), activate a connection.
  - Provider visibility (which providers appear in the pickers).
  - Catalog list with per-tier and models.dev badges, context window, and input/output pricing.
  - Global parameters: temperature and max-tokens sliders.

The Add Provider modal has quick-fill chips (Anthropic, OpenAI, DeepSeek, GLM, Kimi, MiniMax, Ollama, Custom OpenAI-compatible) and an **Advanced** section: extra headers, `default_max_tokens`, per-tier model overrides (fast/standard/pro), and fallback models. The API key field is write-only (`type="password"`); when editing a connection, leaving it empty keeps the stored key. Before saving you can **Test connection** (spinner, then success with latency, or a categorized failure: invalid key / rate limited / unreachable / provider error; with the key field left empty in edit mode it tests the stored credential — the test never persists anything) and **Fetch model list** (queries the provider's models endpoint with the same key/base URL and offers the ids as suggestions; an empty result or a failure leaves the field free-text, with the failure reason shown inline).

## Models & tiers

```
/model                                  open the model picker
/model <id>                             set by catalog id or alias (e.g. sonnet)
/model <provider>/<model-id>            set a qualified id (e.g. ollama/llama3)
/model --tier <tier> [provider] [--save]
/model --max-tokens <N|clear> [--save]
/model refresh                          re-pull models.dev + LiteLLM pricing (background)
```

- **Tier aliases** — input sugar, resolved against the active provider:

  | Input | Canonical tier |
  |-------|----------------|
  | `fast`, `haiku`, `flash`, `mini`, `nano` | `fast` |
  | `standard`, `sonnet`, `plus`, `medium`, `turbo` | `standard` |
  | `pro`, `opus`, `ultra`, `max`, `large` | `pro` |
  | `auto` | resolved at run time to the best of standard → pro → fast |

  `/model --tier auto` switches to the concrete tier it resolved to (`auto → standard`, for example); `auto` itself is never persisted. `--save` pins the `(provider, tier) → model` mapping **and** the active target in `providers.toml`; without `--save` the switch lasts for the session only.
- **Model sources** — three layers, merged additively:
  1. Static catalog (~50 curated models with context window, pricing, capabilities).
  2. models.dev overlay — fetched by `/model refresh` or automatically after `/connect`; cached at `~/.shannon/cache/models-dev.json` for 24h; offline reads use the cache. Provider slugs surfaced from the overlay: `anthropic`, `openai`, `google`, `deepseek`, `mistral`, `xai`, `cohere`, `moonshotai` (Moonshot), `perplexity`, `zhipuai` (Zhipu), `minimax`, `alibaba` (DashScope) — other slugs in the feed are dropped. Static-catalog entries always win over overlay entries for the same id.
  3. Ollama detection — models reported by `ollama list` appear for the `ollama` provider; each model's context window is read with `ollama show` (falling back to 4,096 only when the output is unparseable).
- **Pricing** — resolved per model from, in order: the static catalog → the built-in gap-filler table → a `.shannon-pricing.json` file in the project root → `SHANNON_PRICING_JSON` → the LiteLLM community feed (24h cache at `~/.shannon/cache/litellm-prices.json`, refreshed by `/model refresh`; used only for models the sources above do not price). Negative prices in overrides are rejected.
- An id unknown to both catalogs is used as-is with a warning: `⚠ '<id>' is not in the catalog; using as-is. Run /model refresh to pull the latest models, or /model <provider>/<id> for a qualified id.`
- `/model --max-tokens N` sets a per-provider output ceiling used when a request doesn't specify one; `clear` (or `0`) reverts to the catalog default. It reports `(not saved)` unless `--save` is passed.

## Provider reference

Slugs are accepted by `/connect`, `/provider`, `--provider`, and `SHANNON_PROVIDER` (case-insensitive). Default base URLs can be overridden per profile (`--base-url`, desktop base-URL field, or `SHANNON_BASE_URL`).

| Slug (aliases) | Key env var | Default base URL | Wire format |
|----------------|-------------|------------------|-------------|
| `anthropic` (`claude`) | `ANTHROPIC_API_KEY` | `https://api.anthropic.com` | Anthropic `/v1/messages` |
| `openai` (`gpt`, `chatgpt`) | `OPENAI_API_KEY` | `https://api.openai.com` | OpenAI `/v1/chat/completions` |
| `ollama` (`local`) | — (no auth) | `http://localhost:11434` | Ollama `/api/chat` |
| `gemini` (`google`) | `GEMINI_API_KEY` | `https://generativelanguage.googleapis.com` | Gemini `generateContent` |
| `deepseek` (`ds`) | `DEEPSEEK_API_KEY` | `https://api.deepseek.com` | OpenAI |
| `azure` (`azure-openai`) | `AZURE_OPENAI_API_KEY` | `https://openai.azure.com` | OpenAI (deployments) |
| `bedrock` (`aws`) | — (extra headers / SigV4) | `https://bedrock-runtime.us-east-1.amazonaws.com` | Anthropic |
| `mistral` (`mistral-ai`) | `MISTRAL_API_KEY` | `https://api.mistral.ai` | OpenAI |
| `groq` | `GROQ_API_KEY` | `https://api.groq.com` | OpenAI |
| `together` (`together-ai`) | `TOGETHER_API_KEY` | `https://api.together.xyz` | OpenAI |
| `openrouter` | `OPENROUTER_API_KEY` | `https://openrouter.ai` | OpenAI `/api/v1` |
| `cohere` | `COHERE_API_KEY` | `https://api.cohere.com` | OpenAI `/v2/chat` |
| `fireworks` | `FIREWORKS_API_KEY` | `https://api.fireworks.ai` | OpenAI |
| `perplexity` | `PERPLEXITY_API_KEY` | `https://api.perplexity.ai` | OpenAI |
| `xai` (`grok`) | `XAI_API_KEY` | `https://api.x.ai` | OpenAI |
| `ai21` | `AI21_API_KEY` | `https://api.ai21.com` | OpenAI |
| `siliconflow` (`sf`) | `SILICONFLOW_API_KEY` | `https://api.siliconflow.cn` | OpenAI |
| `zhipu` (`zhipu-cn`, `glm`) | `ZHIPU_API_KEY` | `https://open.bigmodel.cn` | OpenAI `/api/paas/v4` (JWT auth) |
| `zhipu-intl` (`glm-intl`, `zhipu-international`) | `ZHIPU_INTL_API_KEY` | `https://open.international.bigmodel.cn` | OpenAI `/api/paas/v4` (JWT auth) |
| `zhipu-coding` (`zhipu-anthropic`) | `ZHIPU_API_KEY` | `https://open.bigmodel.cn/api/anthropic` | Anthropic (`x-api-key`) |
| `zhipu-coding-plan` (`glm-plan`, `zhipu-plan`) | `ZHIPU_API_KEY` | `https://open.bigmodel.cn/api/coding/paas/v4` | OpenAI (plain Bearer) |
| `moonshot` (`kimi`) | `MOONSHOT_API_KEY` | `https://api.moonshot.cn` | OpenAI |
| `minimax` (`mm`) | `MINIMAX_API_KEY` | `https://api.minimax.chat` | OpenAI |
| `dashscope` (`qwen`, `aliyun`) | `DASHSCOPE_API_KEY` | `https://dashscope.aliyuncs.com` | OpenAI `/compatible-mode/v1` |
| `cloudflare` (`cf`) | — | `https://api.cloudflare.com` | OpenAI (Workers AI) |
| `replicate` | — | `https://api.replicate.com` | OpenAI |

Not `/connect` slugs, but related names you will see:

| Name | Where it applies | Wire format |
|------|------------------|-------------|
| `openai-compatible` | A `--kind` value for `shannon providers add` (and the desktop's Custom OpenAI-compatible chip) — `--base-url` required. An unrecognized host with this kind uses the OpenAI wire format. | OpenAI |
| `custom` | The engine's label for a base URL it does not recognize (e.g. `SHANNON_BASE_URL` pointing at your own gateway; default fallback `http://localhost:8080`). Anthropic-compatible wire with `Authorization: Bearer` from the stored key. | Anthropic |

Detailed pages: [Anthropic](providers/anthropic.md) · [OpenAI](providers/openai.md) · [DeepSeek](providers/deepseek.md) · [GLM / Zhipu](providers/glm-zai.md) · [Kimi / Moonshot](providers/kimi-moonshot.md) · [MiniMax](providers/minimax.md) · [Ollama](providers/ollama.md) · [OpenRouter](providers/openrouter.md).

## Credentials

- **Store layout** — one JSON file per provider at `~/.shannon/credentials/<service>.json`, mode `0600`, written atomically. `<service>` is the provider slug (`anthropic`, `openai`, `zhipu`, ...).
- **Resolution order** for the active provider:
  1. The active profile's credential reference in `providers.toml` — normally a store reference (the file above). When it resolves to a key, that key wins.
  2. Otherwise the environment chain: `SHANNON_API_KEY` first, then the provider's canonical variable (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `DEEPSEEK_API_KEY`, `ZHIPU_API_KEY`, ... — see the table above). For Anthropic, `CLAUDE_API_KEY` and `ANTHROPIC_AUTH_TOKEN` are also honored.
  3. Empty → "no key" (queries to auth-required providers fail with `401`).
- **Rotation** — just run `/connect <provider> <new-key>` again (or update the key in Settings → Models). `/disconnect <provider>` removes the connection but intentionally keeps the stored key.

## Configuration files & precedence

`config.toml` / `.shannon.toml` accept these flat keys (project overrides global):

```toml
model = "claude-sonnet-4-20250514"
provider = "anthropic"           # any slug from the table above
base_url = ""                    # optional endpoint override
max_tokens = 8192
temperature = 0.7                # clamped to 0.0–2.0
timeout = 120                    # seconds (300 for Ollama)
debug = false
max_context_tokens = 200000
permission_profile = "balanced"  # strict | balanced | permissive | custom:<name>
```

> **Warning:** `api_key = "..."` (or any secret) in `config.toml` / `.shannon.toml` is **ignored by design**. The loader only recognizes the keys above; `/config set` refuses secret keys outright (`api_key` is not in the writable allowlist). Put keys in the credential store (`/connect`) or the environment.

`/config set <key> <value>` writes the JSON KV store and mirrors engine-readable keys (`model`, `provider`, `max_tokens`, `temperature`, `timeout`, `debug`) into `~/.shannon/config.toml`, where the next launch picks them up. `/config reset <key>` removes it from both. The CLI is aligned: `shannon config model=deepseek-chat` mirrors the same writable keys into `config.toml`; a key outside the allowlist is stored in `config.json` only (the output says so), and a secret-shaped key (`api_key`, `token`, `secret`, ...) is refused outright — secrets belong in the credential store or the environment.

`shannon --dump-config` prints a JSON ladder — `builtin` → `user-global` → `project` → `env-vars` → `connected` → `cli-overlay` — with each layer's path and the merged result, so you can see exactly which source supplied the active provider/model/credential reference.

## Environment variable reference

| Variable | Purpose |
|----------|---------|
| `SHANNON_API_KEY` | Credential of first resort; beats the per-provider canonical var |
| `SHANNON_MODEL` | Default model |
| `SHANNON_PROVIDER` | Default provider (slug) |
| `SHANNON_BASE_URL` | Default endpoint override |
| `SHANNON_MAX_TOKENS` | Default output cap |
| `SHANNON_TEMPERATURE` | Default temperature |
| `SHANNON_TIMEOUT` | Request timeout (seconds) |
| `SHANNON_DEBUG` | Enable debug logging |
| `SHANNON_ENABLE_TOOLS` | Force tool calling on (`true`) / off (`false`) |
| `SHANNON_MAX_CONTEXT_TOKENS` | Override the context-window budget |
| `SHANNON_PERMISSION_PROFILE` | `strict` / `balanced` / `permissive` / `custom:<name>` |
| `SHANNON_EFFORT` | Reasoning effort: `low` / `medium` / `standard` / `high` / `max` (same values as `--effort`; high/max enable extended thinking) |
| `SHANNON_ENABLED_PROVIDERS` | Comma-separated allowlist of slugs shown in pickers |
| `SHANNON_DISABLED_PROVIDERS` | Comma-separated denylist; applied within the allowlist |
| `SHANNON_PRICING_JSON` | JSON pricing overrides (highest-priority pricing source) |
| `ANTHROPIC_API_VERSION` | Anthropic API version header (default `2023-06-01`) |
| `ANTHROPIC_MODEL` / `OPENAI_MODEL` | Model fallbacks when `SHANNON_MODEL` is unset |
| `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL` | Endpoint fallbacks when `SHANNON_BASE_URL` is unset |
| `CLAUDE_API_KEY` / `ANTHROPIC_AUTH_TOKEN` | Additional Anthropic credential fallbacks (Claude Code migration) |
| `OLLAMA_HOST` | Endpoint the `ollama` CLI talks to (`ollama list` / `ollama show` power local-model detection) |
| `<PROVIDER>_API_KEY` | Canonical per-provider keys — see the provider reference table |

With no credential, no base URL, and no provider configured anywhere, Shannon falls back to local Ollama at `http://localhost:11434` with model `llama3`. Provider visibility filtering fails open: a typo'd `SHANNON_ENABLED_PROVIDERS` never empties the picker.

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `401` / `Authentication failed` mid-session | Update the key with `/connect <provider> <new-key>` — **not** `/config` (it cannot set keys). The new key is hot-reloaded, no restart. |
| `⚠ '<id>' is not in the catalog` after `/model` | Run `/model refresh` to pull the latest models.dev catalog, or use the qualified form `/model <provider>/<model-id>`. |
| Ollama models not detected | Ensure `ollama` is on `PATH` and running (`ollama list` must work). For a non-default host, export `OLLAMA_HOST`; endpoints are auto-detected from `:11434` or `ollama` in the URL. `/local-models` probes `localhost:11434` and LM Studio on `localhost:1234`. |
| `warning: ~/.shannon/providers.toml exists but is not a valid provider config; it is being ignored until fixed (writes to it are refused): ...` | The file failed to parse (the schema rejects unknown fields — often a hand edit). Fix or remove the file; Shannon refuses to overwrite it so your edits are never silently destroyed. |
| `/provider health` shows no row for Gemini / Bedrock / Azure / Replicate | Expected: they have no shared list-models endpoint to probe and are skipped. Everything else is probed concurrently (5s timeout each). |
| Provider is down | `/provider health` lists reachable candidates and suggests `/provider <name>` — switching is always manual; Shannon ships no automatic model router. |
| Config change seems ignored | Run `shannon --dump-config` and check which layer won. A `providers.toml` entry (the connected layer) beats `SHANNON_*` env vars; a CLI flag beats both. |
