# OpenRouter

## What you need

- An API key from [openrouter.ai/settings/keys](https://openrouter.ai/settings/keys).
- Environment variable: `OPENROUTER_API_KEY`. (`SHANNON_API_KEY` takes precedence if set.)

## Connect

```bash
# TUI (recommended — stores + probes the key)
/connect openrouter sk-or-...
```

- **Desktop** — Settings → Models → Add Provider → kind `openai-compatible`, base URL `https://openrouter.ai/api/v1`, paste the key, save. Use **Test connection** to check the key and **Fetch model list** to pull the vendor-prefixed ids into the model field before saving.
- **Headless** —

  ```bash
  shannon providers add openrouter --kind openai-compatible \
    --base-url https://openrouter.ai/api/v1 --model anthropic/claude-sonnet-4
  export OPENROUTER_API_KEY="sk-or-..."
  ```

## Defaults

| Setting | Value |
|---------|-------|
| Base URL | `https://openrouter.ai` |
| Endpoint | `POST /api/v1/chat/completions` |
| Auth | `Authorization: Bearer <key>` |

## Verify

- `/connect` probe success: `✓ Credential verified — '<model>' is reachable with this key.`
- Probe failure (hard 401 aborts): `✗ Authentication failed for 'openrouter'. The key was stored but the provider rejected it — check the key and run /connect again.`
- Desktop: **Test** on the OpenRouter card, or **Test connection** in the Add/Edit Provider modal before saving. `/provider health` probes the OpenRouter models endpoint.

## Troubleshooting

- `Authentication failed` mid-session → `/connect openrouter <new-key>`; no restart needed.
- `404` on a model id → OpenRouter ids are **vendor-prefixed** (`anthropic/claude-...`, `openai/gpt-...`, `deepseek/deepseek-chat`). Set the full id: `/model openrouter/anthropic/claude-sonnet-4`.
- Id typo or missing model → the built-in catalog ships the main OpenRouter entries (`anthropic/claude-sonnet-4`, `anthropic/claude-opus-4`, `openai/gpt-5`, `google/gemini-2.5-pro`, `deepseek/deepseek-chat`, `meta-llama/llama-3.3-70b-instruct`); anything else must be typed in full (it is used as-is even when unknown to the catalog) — **Fetch model list** in the desktop modal gives you the exact ids.

## Notes

- Because ids are vendor-prefixed, the qualified form is `/model openrouter/<vendor>/<model>` — the first slug selects Shannon's provider, the rest is the OpenRouter id.
- `/connect openrouter` and `/provider openrouter` auto-pick a default from the catalog (`anthropic/claude-sonnet-4`); switch with `/model openrouter/<vendor>/<model>` for anything else.
- Extra OpenRouter headers (e.g. app attribution) can be added per profile via `--extra-header K=V` or the desktop Advanced → extra headers rows.
