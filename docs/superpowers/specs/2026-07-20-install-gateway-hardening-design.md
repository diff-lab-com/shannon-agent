# Install & Gateway Hardening — Design

**Date:** 2026-07-20
**Status:** Awaiting user approval
**Scope:** Fix 5 gaps in shannon-agent's install/release/gateway story, found
during the v0.7.0 stable release review. All 5 fixes are independent and
shippable in any order, but combined into one spec because they share the
same release pipeline + install-script surface.

## Background (from v0.7.0 audit)

After v0.7.0 stable shipped (run 29692138251, 20 assets, `latest`),
reviewing the install + 3-product story surfaced 5 gaps:

1. **`shannon-code` naming confusion** — the old product name persists in
   198 doc locations (including the desktop window title "Shannon Code").
   The release ships it as `shannon` (the unified CLI), but no
   compatibility note explains the rename.
2. **Windows gateway missing** — `release.yml` gateway matrix builds only
   linux/darwin. `install.ps1` explicitly skips gateway on Windows.
3. **No gateway config guidance in install** — `install.sh` prints only
   `shannon gateway install`; no mention of `shannon gateway setup`,
   `~/.shannon/gateway/config.json`, or that platform bot tokens must be
   enrolled in the OS keyring.
4. **Port 33420 api_server contention** — both `shannon` CLI and
   `shannon-desktop` host their own `ShannonApiServer` on
   `127.0.0.1:33420`. They cannot run concurrently; second to bind fails
   (`desktop/src/loopback_api.rs:28`, `crates/shannon-cli/src/main.rs:1785`).
5. **Gateway double-spawn** — `shannon gateway install` registers a
   systemd/launchd user service that auto-starts the gateway, AND the
   desktop's `gateway_supervisor.rs` spawns its own child gateway process
   (`desktop/src/commands_connections.rs:260`). Two gateway processes
   contend for port 33430 + the engine's WS endpoint.

See [[install-gateway-gaps]] for full context.

## Decisions (user-approved via brainstorming 2026-07-20)

- **Q4-A strategy:** Desktop detects and reuses an existing api_server
  before hosting its own loopback server. CLI behavior is unchanged.
- **Q4-B strategy:** Desktop's `gateway_supervisor` checks whether a
  systemd/launchd-managed gateway service is already running; if so it
  treats that as authoritative and does not spawn a child.

## Architecture

### Q1 — Shannon-Code naming compatibility

**Goal:** A first-time reader of `README.md`/`CLAUDE.md` and a first-time
launcher of `shannon-desktop` understands that the `shannon` binary is
the former `shannon-code`.

**Change:**
- `README.md`: add a one-line "Compatibility" callout under the title
  block: `> Note: The unified \`shannon\` CLI replaces the former
  \`shannon-code\` product from earlier releases. Install + usage are the
  same; only the binary name changed.`
- `CLAUDE.md`: add the same one-line callout (the project context file is
  loaded into every session, so future agents don't get confused either).
- `desktop/tauri.conf.json`: change the window title from `"Shannon Code"`
  to `"Shannon Agent"` so the GUI matches the unified product name.

**Scope boundary:** Deeper doc rewrites (the 198 internal hits) are out
of scope for this fix — they're historical context, not user-facing.
Mentioning `shannon-code` in CHANGELOG/SCHEDULED-FIX-PLAN/release-notes
is correct and should remain (it documents the migration).

### Q2 — Windows gateway build

**Goal:** `install.ps1` installs the gateway on Windows, just like
`install.sh` does on linux/macOS.

**Change:**
- `.github/workflows/release.yml`: add `bun-windows-x64` to the gateway
  matrix. Bun `--target=bun-windows-x64` produces
  `shannon-gateway-windows-x64.exe` (note the `.exe` suffix). Runner:
  `windows-latest`. Update the matrix `artifact` field accordingly.
- `scripts/install.ps1`:
  - Define `$GATEWAY = 'shannon-gateway-windows-x64.exe'`.
  - Download + verify (existing `Download-Verify` helper works with the
    `.exe` suffix; checksum sidecar naming still applies).
  - Copy to `$InstallDir\shannon-gateway.exe` so `which`/`shannon
    gateway` lookups work.
  - Print the same hint block as install.sh (see Q3).

**Asset name in release:** `shannon-gateway-windows-x64.exe` (matches the
`artifact` matrix entry, follows bun's default naming).

### Q3 — Gateway configuration guidance in install scripts

**Goal:** A user who just ran `curl ... | sh` (or `irm ... | iex` on
Windows) knows the exact next 3 commands to make the gateway do
something useful.

**Change to both install scripts** — replace the existing 3-line hint
block with:

```
[ok] Shannon Agent installed. Next steps:
[info]   1. export SHANNON_API_KEY="sk-ant-..."            (or $env:SHANNON_API_KEY=... on PowerShell)
[info]   2. shannon                                          # launch the REPL
[info]   3. shannon gateway setup                            # initialize ~/.shannon/gateway/config.json
[info]   4. shannon gateway install                          # register gateway as background service (linux/macOS)
[info]   5. shannon gateway enroll <platform>                # enroll a chat-platform bot token (one per adapter)
[info]  Docs: https://shannon.ai/docs/gateway                # platform-specific setup for Slack/Telegram/Discord/Matrix/WhatsApp/WeCom/Feishu/DingTalk
```

`shannon gateway setup` and `shannon gateway enroll` already exist
(`gateway/src/index.ts` imports them from `./service/service.js`; the CLI
delegates to them via `run_gateway_command`). They're currently
undocumented from the install path.

### Q4-A — Desktop detects + reuses existing api_server

**Goal:** When the desktop app starts, it first checks if anything is
already listening on `127.0.0.1:33420` and serving the api_server WebSocket
protocol. If yes, it connects as a client. Only hosts its own loopback
server when nothing is there.

**Current state:**
- `desktop/src/loopback_api.rs` always builds + binds
  `ShannonApiServer` on `127.0.0.1:33420`. The wiring happens in
  `desktop/src/main.rs:271` (per earlier audit grep).
- `crates/shannon-cli/src/main.rs:1785` does the same for the CLI.
- `crates/shannon-cli/src/main.rs:2717-2721` already has a port check
  that WARNs if 33420 is taken — but it doesn't *honor* the existing
  server, it just complains. The CLI itself runs the engine, so this is
  the expected shape; only the desktop needs the new behavior.

**Change:**

1. Add a new module `desktop/src/engine_discovery.rs` (sibling to
   `loopback_api.rs`) with a single function:

   ```rust
   /// Try to connect to an existing api_server on `127.0.0.1:33420`.
   /// Returns `Some(())` if a reachable api_server is there (we should
   /// connect as client), `None` otherwise (we should host our own).
   ///
   /// Health check: open a TCP connection to 127.0.0.1:33420 with a
   /// 250 ms timeout, then send an `OPTIONS` to `/api/ws` and check
   /// for a 2xx/4xx response (4xx proves the server speaks HTTP; that
   /// rules out an unrelated service squatting on the port). Both
   /// checks must succeed.
   pub async fn probe_existing_engine() -> bool { ... }
   ```

2. Modify `desktop/src/main.rs` startup sequence: call
   `engine_discovery::probe_existing_engine().await` *before* building
   the loopback server. If `true`, skip the `loopback_api` build and
   wire the WebSocket client directly to `ws://127.0.0.1:33420/api/ws`
   (the address already in `CANONICAL_ENGINE_WS_URL` at
   `commands_connections.rs:108`).

3. Surface the choice in the UI: a small footer indicator
   "Engine: loopback (hosted by this app)" vs "Engine: external
   (connected to running shannon CLI)" — helps users debug.

4. Tests:
   - Unit test for `probe_existing_engine` with no listener → `false`.
   - Unit test with a mock TCP listener that returns 200 OK → `true`.
   - Integration: launch a `shannon` subprocess, then start desktop
     loopback logic in the same process, assert it detects and skips
     loopback bind.

**Race condition:** Two desktops starting concurrently would both probe
→ both see "no server" → both bind → second fails. Acceptable: this is a
multi-desktop-on-one-machine scenario, not a normal usage. The TCP bind
error is recoverable (warn user, retry once with a fresh probe).

### Q4-B — Desktop gateway supervisor prefers systemd/launchd service

**Goal:** If the user already ran `shannon gateway install` (which
registers a systemd --user / launchd service), the desktop should treat
that running service as the authoritative gateway and NOT spawn its own
child.

**Current state:**
- `desktop/src/gateway_supervisor.rs` `resolve_binary` looks at: explicit
  path → resource dir → `which("shannon-gateway")` (lines 71, 82, 243).
- `desktop/src/commands_connections.rs:260` `GatewaySupervisor::start`
  always spawns a child if binary resolves.
- `gateway/src/service/service.ts` exposes `status(profile)` that runs
  `systemctl --user is-active shannon-gateway.service` (or launchctl on
  macOS) and returns the service state.

**Change:**

1. Add a sibling module `desktop/src/gateway_service_probe.rs`:

   ```rust
   /// Query the OS service manager for a registered `shannon-gateway`
   /// service. Returns the active state ("active", "inactive",
   /// "unknown") so the supervisor can decide whether to spawn.
   /// Falls back to "unknown" when no service is registered (the
   /// install hint was never followed), in which case the supervisor
   /// continues with its current spawn-a-child behavior.
   pub async fn query_gateway_service_state() -> ServiceState { ... }
   ```

   Implementations per platform:
   - **linux:** `systemctl --user is-active shannon-gateway.service`
   - **macos:** `launchctl list | grep shannon.gateway` (or
     `launchctl print user/$UID | grep shannon-gateway`)
   - **windows:** `schtasks /Query /TN "Shannon Gateway"`. (Even though
     windows gateway is now built, the service-registration subcommand
     may not be implemented yet — treat absence as `Unknown`, which
     falls through to spawn-a-child.)

2. Modify `desktop/src/commands_connections.rs:260`
   `GatewaySupervisor::start`:
   - First call `gateway_service_probe::query_gateway_service_state`.
   - If state is `Active` → log "gateway already running as OS service;
     using authoritative instance" and return a `GatewaySupervisor` in
     a new variant `GatewaySupervisorStatus::ManagedExternally {
     service_name }` (no child pid, no stop capability).
   - Otherwise → current spawn-a-child behavior.

3. UI:
   - "Connections" panel shows "Gateway: managed by shannon-gateway
     service" with a disabled Stop button when external, vs the current
     start/stop controls when supervised.

4. Tests:
   - Mock `query_gateway_service_state` to return `Active`; assert no
     child process is started.
   - Mock to return `Inactive` / `Unknown`; assert supervisor spawns as
     before.
   - Real binary test on linux CI: `shannon-gateway install && shannon
     desktop` → assert supervisor reports external management.

**Note:** The user's authorization was scoped to Q4-A and Q4-B but did
not include actually implementing `shannon gateway install` as a
subcommand for windows. The windows service path here is
defensive-only (gracefully falls through to spawn-a-child when no
service is registered). The install.ps1 hint will not advertise
`shannon gateway install` on Windows until the service module supports
it (see "out of scope" below).

## File Structure (changes by task)

| File | Created / Modified | Responsibility |
|------|-------------------|----------------|
| `README.md` | M | Add shannon-code compatibility note |
| `CLAUDE.md` | M | Add shannon-code compatibility note |
| `desktop/tauri.conf.json` | M | Window title "Shannon Code" → "Shannon Agent" |
| `.github/workflows/release.yml` | M | Add windows target to gateway matrix |
| `scripts/install.sh` | M | Expanded hint block |
| `scripts/install.ps1` | M | Download windows gateway + expanded hint block |
| `desktop/src/engine_discovery.rs` | **C** | Q4-A: probe existing api_server |
| `desktop/src/main.rs` | M | Wire probe before loopback bind |
| `desktop/src/commands_connections.rs` | M | Q4-B: probe service before spawning |
| `desktop/src/gateway_service_probe.rs` | **C** | Q4-B: query systemd/launchd/schtasks |
| `desktop/src/gateway_supervisor.rs` | M | Add `ManagedExternally` status variant |
| `desktop/ui/src/...` (status footer + connections panel) | M | Show "engine: external vs loopback" + "gateway: managed by service" |
| `docs/superpowers/plans/2026-07-20-install-gateway-hardening.md` | **C** | Task-by-task implementation plan |

## Out of Scope

- Implementing `shannon gateway install` for Windows (the gateway
  service module's windows path via schtasks). The Q2 fix ships the
  Windows *binary*; users can run it manually with `nssm` or similar.
  Adding full Windows service support is a follow-up.
- Cleanup of the 198 internal `shannon-code` doc hits. Touched surfaces
  are user-facing only (README, CLAUDE.md, window title).
- Migrating `desktop/src/loopback_api.rs` to be lazy (built only when
  discovery says needed). Done as part of Q4-A implementation but
  architectural refactoring of the engine layer is not.
- Catching the `tauri.conf.json` updater endpoint
  (`https://gitea.diff-lab.com/...`) — out of scope, that URL is
  pre-existing and unrelated.

## Testing Strategy

- **Unit tests:** All new modules (`engine_discovery`,
  `gateway_service_probe`) have unit tests with mocks.
- **Integration tests on Linux CI:** Spawn a real `shannon` subprocess
  in a test, then probe discovery, assert reuse. For Q4-B, `shannon
  gateway install` into a temp systemd --user dir, then probe supervisor,
  assert external.
- **Manual verification on macOS:** Required because launchctl behaves
  differently from systemctl. CI doesn't cover this; document in the
  verification log.
- **Release verification:** After fix lands, cut v0.7.1-rc1 to validate
  the full Windows gateway matrix and the new desktop detection logic.

## Self-Review Checklist (filled in after writing)

- [x] No `shannon-code` rename is proposed — only a note + window title.
- [x] All 5 fixes preserve CLI standalone behavior (the CLI can always
      be used without the desktop).
- [x] Q4-A detection uses a sub-second timeout (250 ms) so desktop
      startup isn't noticeably slower.
- [x] Q4-B fallback (spawn-a-child) is preserved when no service is
      registered, so first-time users get the same behavior as today.
- [x] install.sh/install.ps1 hint blocks reference real subcommands
      that exist (`setup`, `install`, `enroll` are imported in
      `gateway/src/index.ts`).
- [x] Windows gateway binary name (`shannon-gateway-windows-x64.exe`)
      follows bun's default naming so no special artifact config needed.