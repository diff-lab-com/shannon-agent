# GLM / Zhipu (Z.ai)

Shannon supports all four Zhipu/GLM endpoints as distinct providers:

| Slug (aliases) | Endpoint | Auth | Env var |
|----------------|----------|------|---------|
| `zhipu` (`zhipu-cn`, `glm`) | `https://open.bigmodel.cn/api/paas/v4` | Bearer (JWT when key is `id.secret`) | `ZHIPU_API_KEY` |
| `zhipu-intl` (`glm-intl`, `zhipu-international`) | `https://open.international.bigmodel.cn/api/paas/v4` | same | `ZHIPU_INTL_API_KEY` |
| `zhipu-coding` (`zhipu-anthropic`) | `https://open.bigmodel.cn/api/anthropic` | `x-api-key` (Anthropic wire) | `ZHIPU_API_KEY` |
| `zhipu-coding-plan` (`glm-plan`, `zhipu-plan`) | `https://open.bigmodel.cn/api/coding/paas/v4` | plain Bearer | `ZHIPU_API_KEY` |

## What you need

- A key from the Zhipu open platform ([bigmodel.cn](https://open.bigmodel.cn), API Keys in the console) or Z.ai for the international endpoint. GLM Coding Plan subscriptions keys come from the same console.
- Environment variable: `ZHIPU_API_KEY` (CN / Coding / Coding Plan) or `ZHIPU_INTL_API_KEY` (International).

## Connect

```bash
# TUI — mainland open platform
/connect glm <your-api-key>
# TUI — GLM Coding Plan quota (plain Bearer, Anthropic-compatible tools)
/connect glm-plan <your-key>
```

- **Desktop** — Settings → Models → Add Provider → quick-fill **GLM (Zhipu)** (kind `openai-compatible`, base URL `https://open.bigmodel.cn/api/paas/v4`), paste the key, save.
- **Headless** —

  ```bash
  shannon providers add glm --kind openai-compatible \
    --base-url https://open.bigmodel.cn/api/paas/v4 --model glm-4-plus
  export ZHIPU_API_KEY="..."
  ```

## Verify

- `/connect` probe success: `✓ Credential verified — 'glm-...' is reachable with this key.`
- Probe failure (hard 401 aborts): `✗ Authentication failed for 'zhipu'. The key was stored but the provider rejected it — check the key and run /connect again.`
- Desktop: **Test** on the provider card, or **Test connection** in the Add/Edit Provider modal before saving (**Fetch model list** there pulls the endpoint's model ids). `/provider health` probes Zhipu via the shared OpenAI-compatible models endpoint.

## Troubleshooting

- 401 with an `id.secret`-style key → Shannon signs it as a JWT automatically (see Notes). A 401 here usually means a revoked/expired key: `/connect glm <new-key>`.
- Wrong regional endpoint → `zhipu` and `zhipu-intl` have different consoles and keys; make sure the key matches the slug.
- Coding Plan 401 → use the `zhipu-coding-plan` slug (plain Bearer), not `zhipu-coding` (`x-api-key`).

## Notes

- **JWT quirk:** for `zhipu` / `zhipu-intl`, a key of the form `id.secret` is exchanged for a short-lived JWT (HMAC-SHA256) on every request and sent as `Authorization: Bearer <jwt>`. Keys without the `id.secret` shape are sent as-is.
- **Prompt caching:** Shannon auto-injects Anthropic `cache_control` breakpoints only on `api.anthropic.com` / Bedrock hosts — not on Zhipu's Anthropic-compatible `/api/anthropic` endpoint.
- `glm-4-plus`, `glm-4-flash`, `glm-4-long`, `glm-4-air` and newer GLM models ship in the static catalog; `/model refresh` pulls the latest from models.dev.
