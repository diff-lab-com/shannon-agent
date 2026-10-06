# MiniMax

## What you need

- An API key from the [MiniMax open platform](https://platform.minimaxi.com) (international: `platform.minimax.io`).
- Environment variable: `MINIMAX_API_KEY`. (`SHANNON_API_KEY` takes precedence if set.)

## Connect

```bash
# TUI (recommended — stores + probes the key)
/connect minimax <your-api-key>
```

- **Desktop** — Settings → Models → Add Provider → quick-fill **MiniMax** (kind `openai-compatible`, base URL `https://api.minimax.chat/v1`), paste the key, save.
- **Headless** —

  ```bash
  shannon providers add minimax --kind openai-compatible \
    --base-url https://api.minimax.chat/v1 --model MiniMax-M2.7
  export MINIMAX_API_KEY="..."
  ```

Alias: `mm`.

## Defaults

| Setting | Value |
|---------|-------|
| Base URL | `https://api.minimax.chat` |
| Endpoint | `POST /v1/chat/completions` |
| Auth | `Authorization: Bearer <key>` |

## Verify

- `/connect` probe success: `✓ Credential verified — 'MiniMax-...' is reachable with this key.`
- Probe failure (hard 401 aborts): `✗ Authentication failed for 'minimax'. The key was stored but the provider rejected it — check the key and run /connect again.`
- Desktop: **Test** on the MiniMax card, or **Test connection** in the Add/Edit Provider modal before saving (**Fetch model list** there pulls the endpoint's model ids). `/provider health` probes MiniMax via the shared OpenAI-compatible models endpoint.

## Troubleshooting

- `Authentication failed` mid-session → `/connect minimax <new-key>`; no restart needed.
- Region mismatch → the CN platform (`api.minimax.chat`) and international platform (`api.minimax.io`) have separate keys; set the profile base URL to match your key's region.

## Notes

- The static catalog ships `MiniMax-M*` entries (context window, pricing, tier hints); `/model refresh` refreshes them from models.dev.
- `/provider health` probes MiniMax through the shared OpenAI-compatible `/models` endpoint, so a missing model id does not affect the health verdict.
