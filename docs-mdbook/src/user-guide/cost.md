# Cost Control

Shannon is BYOK (bring your own key): you pay providers directly, per use. The product's job is to make that spending **visible and bounded** — no subscription quotas, no black boxes.

## See the cost

- **`/cost`** in the REPL — tokens and estimated cost for the current session.
- **Usage page** in the desktop app — per-session and aggregated history.
- **Context breakdown** (desktop status bar) — how the current context is composed: system prompt, tool definitions, skills, memory, MCP tools, conversation. Plus **cache hit-rate** and **tokens/s**.

> Tip: switching provider or model mid-session can invalidate the provider's prompt cache. Shannon warns you before the switch so a one-line experiment doesn't silently re-bill your whole context.

## Bound the cost

- **Goal budgets** — `shannon --goal "..." --budget 5` hard-stops a run when spend exceeds $5.
- **Session budget caps** — set a per-session cap; when it trips, the session pauses and asks: continue, raise the cap, or stop.
- **Token watchdog** — `SHANNON_TOKEN_BUDGET` nudges the agent away from wholesale re-reads toward targeted reads.

## Pay less

- **Prompt caching** — Anthropic three-layer cache breakpoints are injected automatically (see [Prompt Caching](../features/caching.md)).
- **Cheaper models for cheap work** — per-agent model overrides put the small model on grunt work (see [Agent Teams](agent-teams.md)).
- **Local models** — Ollama runs the whole workload on your machine for zero API cost; auto-detected when running.
