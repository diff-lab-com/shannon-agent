# Ollama (local)

Ollama needs no API key and is Shannon's zero-config fallback: with no credential, no base URL, and no provider configured anywhere, Shannon targets `http://localhost:11434` with model `llama3`.

## What you need

- [Ollama](https://ollama.com) installed and running, with at least one model pulled (`ollama pull llama3`).

## Connect

```bash
# TUI — no key required
/connect ollama
```

- **Desktop** — Settings → Models → Add Provider → quick-fill **Ollama (local)** (base URL `http://localhost:11434`), save.
- **Headless** —

  ```bash
  shannon providers add ollama --kind ollama --base-url http://localhost:11434 --model llama3
  ```

  (`--base-url` is required for the `ollama` kind — Shannon never silently assumes localhost in the CLI.)

## Defaults

| Setting | Value |
|---------|-------|
| Base URL | `http://localhost:11434` |
| Endpoint | `POST /api/chat` |
| Auth | none (`no auth` status everywhere) |
| Timeout | 300s (vs 120s for hosted providers) |

## Verify

- `/connect ollama` succeeds immediately (no probe failure path — there is no auth).
- `/local-models` probes `localhost:11434` and lists installed models (also LM Studio on `localhost:1234`).
- `/provider health` — Ollama is probed with its models endpoint; reachable shows `● reachable`.
- Desktop: the Add/Edit Provider modal has a pre-save **Test connection** and a **Fetch model list** that reads the installed models from the Ollama endpoint.

## Troubleshooting

- **Models not detected in the picker** — Shannon detects models via the `ollama` CLI (`ollama list`). Make sure `ollama` is on `PATH` and the daemon is running (`ollama serve`).
- **Remote Ollama host** — export `OLLAMA_HOST` (the `ollama` CLI honors it) and point the provider base URL at the host. URL detection treats `:11434` or `ollama` in the URL as Ollama.
- **First message fails with a connection error** — the daemon is down or the model isn't pulled; run `ollama list` to check both.
- **Truncated/garbled output** — Ollama sometimes returns malformed streams; Shannon detects the known patterns and retries.

## Notes

- **Context window:** Shannon reads each detected model's real context length with `ollama show` (the `num_ctx` parameter wins; the architectural `context length` is the fallback) and uses it for the context budget. The engine additionally queries Ollama's `/api/show` for tool support. Overrides: `SHANNON_MAX_CONTEXT_TOKENS` or `max_context_tokens` win; when nothing is parseable a conservative 4,096 floor applies.
- Detected models are free (`$0.0/$0.0` pricing) and marked cheap/fast for tier resolution — `/model --tier fast ollama` works out of the box.
