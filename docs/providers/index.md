# Provider Guides

Per-provider setup pages: where to create the API key, how to connect Shannon (TUI, desktop, or headless CLI), how to verify the connection, and provider-specific quirks.

| Provider | Slug(s) | Key env var | Guide |
|----------|---------|-------------|-------|
| Anthropic (Claude) | `anthropic` | `ANTHROPIC_API_KEY` | [anthropic.md](anthropic.md) |
| OpenAI (GPT) | `openai` | `OPENAI_API_KEY` | [openai.md](openai.md) |
| DeepSeek | `deepseek` | `DEEPSEEK_API_KEY` | [deepseek.md](deepseek.md) |
| GLM / Zhipu (CN, International, Coding) | `zhipu`, `zhipu-intl`, `zhipu-coding`, `zhipu-coding-plan` | `ZHIPU_API_KEY` (International: `ZHIPU_INTL_API_KEY`) | [glm-zai.md](glm-zai.md) |
| Kimi / Moonshot | `moonshot` | `MOONSHOT_API_KEY` | [kimi-moonshot.md](kimi-moonshot.md) |
| MiniMax | `minimax` | `MINIMAX_API_KEY` | [minimax.md](minimax.md) |
| Ollama (local) | `ollama` | — (no key) | [ollama.md](ollama.md) |
| OpenRouter | `openrouter` | `OPENROUTER_API_KEY` | [openrouter.md](openrouter.md) |

Shannon also supports Groq, Mistral, xAI, Perplexity, Cohere, Together, Fireworks, SiliconFlow, AI21, DashScope/Qwen, Azure, Bedrock, Cloudflare, Replicate, and any OpenAI-compatible endpoint — see the provider reference table in [configuration.md](../configuration.md#provider-reference).

## The short version

Every auth-requiring provider connects the same way; only the slug, the key, and (for OpenAI-compatible endpoints) the base URL differ.

**TUI:**

```
/connect <provider> <your-api-key>
```

**Desktop:** Settings → Models → Providers → Add Provider (or the Welcome wizard on first run). Pick a quick-fill chip, paste the key, save.

**Headless:**

```bash
shannon providers add <id> --kind <kind> --model <model> [--base-url <url>]
export <PROVIDER>_API_KEY="..."   # or run the TUI's /connect once
```

## Verify anywhere

- **TUI** — `/connect` probes the key with a 1-token request: `✓ Credential verified — '<model>' is reachable with this key.` means done. `✗ Authentication failed ...` means the key was rejected — check it and `/connect` again. Later, `/provider health` live-probes every provider.
- **Desktop** — the **Test** button on each provider card (or **Test all**) runs the same non-billable probe.
