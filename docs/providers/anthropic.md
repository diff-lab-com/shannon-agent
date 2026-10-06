# Anthropic (Claude)

## What you need

- A Claude API key from the [Anthropic Console](https://console.anthropic.com/settings/keys) (keys start with `sk-ant-`).
- Environment variable: `ANTHROPIC_API_KEY`. Shannon also honors `CLAUDE_API_KEY` and `ANTHROPIC_AUTH_TOKEN` (handy for Claude Code migrants); `SHANNON_API_KEY` beats all of them.

## Connect

```bash
# TUI (recommended — stores + probes the key)
/connect anthropic sk-ant-api03-...
```

- **Desktop** — Settings → Models → Add Provider → quick-fill **Anthropic**, paste the key, save. (Or the Welcome wizard on first run.)
- **Headless** —

  ```bash
  shannon providers add anthropic --kind anthropic --model claude-sonnet-4-6
  export ANTHROPIC_API_KEY="sk-ant-..."
  ```

Alias: `/connect claude ...` works too.

## Defaults

| Setting | Value |
|---------|-------|
| Base URL | `https://api.anthropic.com` |
| Endpoint | `POST /v1/messages` |
| Auth | `x-api-key` header + `anthropic-version` (default `2023-06-01`, override with `ANTHROPIC_API_VERSION`) |

## Verify

- `/connect` probe success: `✓ Credential verified — 'claude-...' is reachable with this key.`
- Probe failure (hard 401, the switch is aborted): `✗ Authentication failed for 'anthropic'. The key was stored but the provider rejected it — check the key and run /connect again.`
- Desktop: **Test** on the Anthropic card, or **Test connection** inside the Add/Edit Provider modal before saving (never persists anything).
- Any time: `/provider health` — Anthropic is probed with the shared models endpoint.

## Troubleshooting

- `Authentication failed` mid-session → `/connect anthropic <new-key>`; the new key is hot-reloaded, no restart needed.
- Requests 401 only after switching models → some beta models require `anthropic-beta` headers; Shannon injects model-declared beta headers automatically (e.g. 1M-context unlock).
- Region/endpoint issues → set `ANTHROPIC_BASE_URL` (or the profile base URL) to your gateway. Note the quirk below.

## Notes

- **Prompt caching is auto-injected only on Anthropic-owned hosts** (`api.anthropic.com`, `bedrock-runtime.*.amazonaws.com`). Third-party Anthropic-compatible gateways may reject the `cache_control` field, so Shannon does not send cache breakpoints to them.
- `ANTHROPIC_BASE_URL` / `ANTHROPIC_MODEL` in your shell are picked up as fallbacks, but a connected `providers.toml` profile wins over them.
