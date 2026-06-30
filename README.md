# shannon-gateway

Node/TypeScript gateway that connects chat platforms — Slack, Telegram,
Discord, Matrix, WhatsApp — to the [Shannon](https://github.com/shannon-agent)
engine's built-in `api_server`.

The gateway is the **inbound entry point**: platform messages flow in through
transport adapters, get normalized and routed per-conversation, and are
dispatched to the engine over its existing HTTP/SSE/WebSocket API. Tool calls
that need human approval are rendered as in-channel buttons; the decision is
posted back to the engine. The engine remains the single source of truth for
conversation history, memory, and compaction — the gateway only owns transport,
routing, and channel UX.

## Architecture (four layers)

1. **Transport adapters** — one per platform, implementing `ChannelAdapter`.
   Produce `NormalizedInbound`; consume `ReplyTarget`.
2. **Normalizer** — platform-native event → unified envelope.
3. **Session router** — `sessionKey` → engine session; per-session lane queue
   (serial within a conversation, parallel across conversations).
4. **Engine client + approval** — HTTP/SSE/WS to `api_server`; renders
   `ToolUseRequest` as in-channel approval buttons.

See `claudedocs/social-connection-architecture.md` in `shannon-desktop` for the
full design, decision record, and phased plan.

## Status

Phase 1 scaffold. The engine-side prerequisites (Phase 0: approval roundtrip,
cancel, session persistence) land in `shannon-code` PRs #65–#68.

## Develop

```bash
pnpm install
pnpm typecheck   # tsc --noEmit
pnpm test        # vitest
pnpm dev         # tsx src/index.ts
```

Requires Node 20+ and pnpm 10+.

## Security

Platform credentials never live in this repo — they are stored in the OS
keyring and read at runtime (fixes F14). `.env*` is gitignored.
