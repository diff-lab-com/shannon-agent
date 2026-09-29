# Kimi (Moonshot)

## What you need

- An API key from the [Moonshot platform](https://platform.moonshot.cn/console/api-keys).
- Environment variable: `MOONSHOT_API_KEY`. (`SHANNON_API_KEY` takes precedence if set.)

## Connect

```bash
# TUI (recommended — stores + probes the key)
/connect moonshot sk-...
```

- **Desktop** — Settings → Models → Add Provider → quick-fill **Kimi (Moonshot)** (kind `openai-compatible`, base URL `https://api.moonshot.cn/v1`), paste the key, save.
- **Headless** —

  ```bash
  shannon providers add kimi --kind openai-compatible \
    --base-url https://api.moonshot.cn/v1 --model kimi-k2.6
  export MOONSHOT_API_KEY="sk-..."
  ```

Alias: `kimi`.

## Defaults

| Setting | Value |
|---------|-------|
| Base URL | `https://api.moonshot.cn` |
| Endpoint | `POST /v1/chat/completions` |
| Auth | `Authorization: Bearer <key>` |

## Verify

- `/connect` probe success: `✓ Credential verified — 'kimi-...' is reachable with this key.`
- Probe failure (hard 401 aborts): `✗ Authentication failed for 'moonshot'. The key was stored but the provider rejected it — check the key and run /connect again.`
- Desktop: **Test** on the Kimi card, or **Test connection** in the Add/Edit Provider modal before saving (**Fetch model list** there pulls the endpoint's model ids). `/provider health` probes Moonshot via the shared OpenAI-compatible models endpoint.

## Troubleshooting

- `Authentication failed` mid-session → `/connect moonshot <new-key>`; no restart needed.
- Balance errors → Moonshot returns a provider error when the account is out of credit; top up and retry.
- Model name typos fail only at request time (provider 404) — run `/model refresh` and pick from the list instead of typing ids by hand.

## Notes

- The static catalog ships Moonshot `kimi-*` / `moonshot-v1-*` entries with context windows and pricing; the models.dev overlay keeps them current.
- Older docs' quick-fill id `moonshot-v1-8k` is a legacy short-context model; prefer the current `kimi-k2` family from the picker.
