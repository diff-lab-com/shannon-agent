# Shannon Desktop — Mock Mode

A dev/test mode that lets you run the full UI in any browser **without the Tauri backend**, with realistic seed data for every page. Designed for demo recordings, screenshot generation, design review, and new-contributor onboarding.

## When to use it

| Scenario | Why mock mode helps |
|---|---|
| Recording a demo video | No backend setup; data is predictable |
| Screenshot for docs | Every page is populated, no empty states |
| Design review / handoff | Designer can run `pnpm demo` and click around |
| New contributor onboarding | Explore the UI without Rust toolchain |
| Component screenshot tests | Mock returns deterministic responses |
| Manual QA of empty/loading/error states | Easily editable in `data/*.ts` |

## Enable mock mode

Three ways (any one):

1. **URL flag** — append `?demo=1` to any URL: <http://localhost:1420/?demo=1>
2. **localStorage** — `localStorage.setItem('shannon:mock', '1')` in the console
3. **npm script** — `pnpm demo` (preferred — auto-sets the env flag)

A purple `DEMO MODE` badge appears bottom-left when active.

## Run it

```bash
cd ui
pnpm demo
```

This runs `vite` with `VITE_MOCK_MODE=1`. Open <http://localhost:1420>.

To turn it off without restarting: `localStorage.removeItem('shannon:mock')` and reload.

## What's mocked

Every Tauri command in `src/lib/tauri-api.ts` has a handler in `handlers.ts`. If you call an unmapped command, the console warns and falls through to real Tauri (which will fail in a browser, which is what you want — surfaces missing mocks).

Mock data lives in `data/`:

| File | Used by |
|---|---|
| `data/core.ts` | Chat, Tasks, MissionControl, Extensions, OPC, QuickFix |
| `data/automation.ts` | Scheduled routines, Routines, Hooks, Profiles |
| `data/analytics.ts` | Triage, OPC metrics, Perf, Billing, Goals, Diagnostics |
| `data/config.ts` | Settings, status, models |

## Editing mock data

Mock data is plain TS — edit `data/*.ts` and hot-reload picks it up.

Some handlers keep **mutable state** (a small in-memory store) so the UI feels live:
- `list_tasks` / `update_task` — task status updates persist for the session
- `start_background_task` / `cancel_background_task` — background tasks appear live
- `create_scheduled_task` / `delete_scheduled_task` — scheduled routines add/remove

If you want a deterministic snapshot, restart the dev server.

## Adding new mock handlers

If you add a new Tauri command to `tauri-api.ts`:

1. Add a handler in `handlers.ts` with the same command name
2. Add seed data in `data/*.ts` if needed
3. Run `pnpm demo` and verify

The mock will throw `[mock] unhandled command: <name>` in the console for any missing handler, so you'll notice.

## Mocking async events (Tauri event listeners)

`AppContext.tsx` listens to Tauri events for streaming tokens and tool calls. In mock mode these are not emitted automatically. To test streaming UI:

```ts
import { emit } from '@tauri-apps/api/event'
emit('query_text', { content: 'Hello ' })
emit('query_text', { content: 'world' })
```

This is rarely needed for screenshots; the static `MOCK_MESSAGES` array already gives the Chat page a populated state.

## Tests

Mock mode is **dev-only**. Vitest tests import the real API module and pass `invoke` via `vi.mock('@tauri-apps/api/core')` per-test. See existing tests under `__tests__/` for patterns.

## Disable in production

`isMockMode()` returns `false` when:
- No `?demo=1` URL flag, AND
- No `localStorage.shannon:mock`, AND
- `VITE_MOCK_MODE` is not set, AND
- Either not in dev OR running inside Tauri

In a production Tauri build, mock mode never activates.
