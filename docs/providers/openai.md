# OpenAI (GPT)

## What you need

- An API key from the [OpenAI platform](https://platform.openai.com/api-keys) (keys start with `sk-`).
- Environment variable: `OPENAI_API_KEY`. (`SHANNON_API_KEY` takes precedence if set.)

## Connect

```bash
# TUI (recommended — stores + probes the key)
/connect openai sk-...
```

- **Desktop** — Settings → Models → Add Provider → quick-fill **OpenAI**, paste the key, save.
- **Headless** —

  ```bash
  shannon providers add openai --kind openai --model gpt-4o
  export OPENAI_API_KEY="sk-..."
  ```

Aliases: `gpt`, `chatgpt`.

## Defaults

| Setting | Value |
|---------|-------|
| Base URL | `https://api.openai.com` |
| Endpoint | `POST /v1/chat/completions` |
| Auth | `Authorization: Bearer <key>` |

## Verify

- `/connect` probe success: `✓ Credential verified — 'gpt-...' is reachable with this key.`
- Probe failure (hard 401, the switch is aborted): `✗ Authentication failed for 'openai'. The key was stored but the provider rejected it — check the key and run /connect again.`
- Desktop: **Test** on the OpenAI card or **Test all**; the Add/Edit Provider modal also has a pre-save **Test connection** (and **Fetch model list** for custom base URLs).
- Any time: `/provider health` — OpenAI is probed with the shared models endpoint.

## Troubleshooting

- `Authentication failed` mid-session → `/connect openai <new-key>`; no restart needed.
- `429 Rate limit exceeded` → the request is retried automatically; if it persists, `/model` to a smaller tier (`/model --tier fast openai`).
- Wrong org/project key → create the key under the intended project on the platform; keys are project-scoped.

## Notes

- `OPENAI_BASE_URL` / `OPENAI_MODEL` in your shell are picked up as fallbacks, but a connected `providers.toml` profile wins over them.
- Azure OpenAI is a separate provider: `/connect azure` with `AZURE_OPENAI_API_KEY` (deployment-based endpoints at `https://<resource>.openai.azure.com`).
- Custom OpenAI-compatible gateways (LiteLLM, NewAPI, self-hosted proxies): use kind `openai-compatible` with an explicit base URL — see the provider reference in [configuration.md](../configuration.md#provider-reference).
