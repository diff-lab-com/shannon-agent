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
| `data/analytics.ts` | Inbox (triage), OPC metrics, Perf, Billing, Goals, Diagnostics |
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

R1 chat-testing infra replaced the old (broken) advice — `emit('query_text', …)`
in the console never worked, because the real `@tauri-apps/api/event` module
walks `window.__TAURI_INTERNALS__` (not the aliased core module) and none of
the `plugin:event|*` commands had handlers. They do now:

- `src/lib/mock/eventBridge.ts` installs `window.__TAURI_INTERNALS__` /
  `window.__TAURI_EVENT_PLUGIN_INTERNALS__` and registers
  `plugin:event|listen|unlisten|emit`, so `listen()` from
  `@tauri-apps/api/event` works in demo mode exactly like in the real shell
  (callbacks receive `{ event, id, payload }`).
- `src/lib/mock/scripted/` adds a **scripted player** that replays full AI
  conversation flows — streaming chunks, thinking, tool calls, permission
  pauses, budget events, errors, cancellation — from a declarative script.

### Driving a conversation by script (ScriptedBackend)

Scripts live in `desktop/ui/e2e/scripts/*.yaml` (schema:
`src/lib/mock/scripted/schema.ts`). In a browser console you can drive the
player by hand:

```js
// Emit one event exactly like the player does (auto query_id/session_id are
// NOT filled here — you control the raw payload):
__shannonMock.emit('query:text', { content: 'Hello ', session_id: null })

// Load a script object (or JSON string), then just send a message in the UI:
__shannonMock.loadScript({ name: 'demo', turns: [{ user: 'hi', script: [
  { event: 'query:text', chunks: ['a', 'b', 'c'], chunkDelayMs: 100 },
  { event: 'query:completed' },
] }] })

// Step controls:
__shannonMock.control.pauseAt(1)   // park when the turn reaches step 1
__shannonMock.control.resume()     // release a waitFor / permission park
__shannonMock.control.speed = 2    // stream twice as fast
__shannonMock.snapshot()           // { phase, turnIndex, stepIndex, … }
__shannonMock.reset()              // back to the default demo seed data
```

While a script is armed, `send_message` replays the next turn's events
instead of the default instant no-op; `respond_permission` resumes a
permission pause (and is recorded in `snapshot().permissionLog`);
`cancel_query` emits the script's `onCancel` steps (default: an immediate
`query:cancelled`). After the last turn the mock falls back to its standard
behavior. E2E should use the Playwright helpers instead
(`e2e/helpers/` — `loadChatScript(page, 'happy-path')`), which seed the
script before the app boots via `window.__SHANNON_SCRIPT__`.

## Tests

Mock mode is **dev-only**. Vitest tests import the real API module and pass `invoke` via `vi.mock('@tauri-apps/api/core')` per-test. See existing tests under `__tests__/` for patterns.

## Disable in production

`isMockMode()` returns `false` when:
- No `?demo=1` URL flag, AND
- No `localStorage.shannon:mock`, AND
- `VITE_MOCK_MODE` is not set, AND
- Either not in dev OR running inside Tauri

In a production Tauri build, mock mode never activates.
