# DeepSeek

## What you need

- An API key from the [DeepSeek platform](https://platform.deepseek.com/api_keys).
- Environment variable: `DEEPSEEK_API_KEY`. (`SHANNON_API_KEY` takes precedence if set.)

DeepSeek is a first-class provider in Shannon (native kind `deepseek`) — no `base_url` wiring needed.

## Connect

```bash
# TUI (recommended — stores + probes the key)
/connect deepseek sk-...
```

- **Desktop** — Settings → Models → Add Provider → quick-fill **DeepSeek**, paste the key, save.
- **Headless** —

  ```bash
  shannon providers add deepseek --kind deepseek --model deepseek-chat
  export DEEPSEEK_API_KEY="sk-..."
  ```

Alias: `ds`.

## Defaults

| Setting | Value |
|---------|-------|
| Base URL | `https://api.deepseek.com` |
| Endpoint | `POST /v1/chat/completions` |
| Auth | `Authorization: Bearer <key>` |
| Common models | `deepseek-chat` (V3), `deepseek-reasoner` (R1) |

## Verify

- `/connect` probe success: `✓ Credential verified — 'deepseek-chat' is reachable with this key.`
- Probe failure (hard 401, the switch is aborted): `✗ Authentication failed for 'deepseek'. The key was stored but the provider rejected it — check the key and run /connect again.`
- Desktop: **Test** on the DeepSeek card, or **Test connection** in the Add/Edit Provider modal before saving (there you can also **Fetch model list** to fill the model field).
- Any time: `/provider health` — DeepSeek has a dedicated probe.

## Troubleshooting

- `Authentication failed` mid-session → `/connect deepseek <new-key>`; no restart needed.
- Top-up required errors → DeepSeek returns a provider error with account balance details; charge the account and retry.

## Notes

- Older Shannon docs suggested `provider = "openai"` + `base_url` for DeepSeek — that flat config style is obsolete. Use `/connect deepseek` (or the native `deepseek` kind in the desktop / CLI) instead.
- Tier mapping: `deepseek-chat` covers the standard tier, `deepseek-reasoner` the pro tier, so `/model --tier pro deepseek` resolves out of the box.
