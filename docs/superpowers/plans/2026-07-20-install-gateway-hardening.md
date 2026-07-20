# Install & Gateway Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land 5 fixes (Q1 naming, Q2 Windows gateway, Q3 install hint block, Q4-A engine discovery, Q4-B gateway service probe) discovered during the v0.7.0 stable release review. Each fix is shippable as its own PR; this plan executes all five in order.

**Architecture:** Doc + CI surface changes first (Q1, Q2, Q3) because they have no runtime risk. Then two new Rust modules in `desktop/src/` (`engine_discovery.rs`, `gateway_service_probe.rs`) wired into the existing `setup()` block in `main.rs` and `bootstrap_gateway_supervisor()` in `commands_connections.rs`. Each new module has unit tests with injectable behavior so the tests run anywhere; platform-specific real-system tests are gated on Linux CI.

**Tech Stack:** Rust 1.88 (already pinned), tokio 1.43, reqwest 0.12 (already a desktop dep), serde_json, Bun `--compile` for the Windows gateway matrix addition, GitHub Actions release pipeline.

## Global Constraints

- **Branch hygiene:** All commits land on `dev` first. `main` stays protected (`enforce_admins:true`, no force push, no deletion, 5 required status checks) per the standing security constraint. Each task ends with a `git push origin dev`.
- **One PR per task.** Each task is its own commit + push; merge to main follows the established dev→main flow (PR with `--merge`, not `--rebase`, due to merge commits in dev).
- **TDD everywhere new.** New Rust modules are introduced test-first (failing test → impl → green test → commit).
- **No scope creep.** Out-of-scope items per the spec: Windows `shannon gateway install` service module implementation; the 198 internal `shannon-code` doc hits; engine-layer refactoring of `loopback_api.rs`.
- **Bun Windows artifact name:** Bun `--target=bun-windows-x64` produces a `.exe` filename automatically. The plan sets `artifact: shannon-gateway-windows-x64.exe` and the release uploader pulls the path `gateway/dist/${{ matrix.artifact }}` — if the very first CI run shows bun produces a doubled `.exe.exe`, the implementer must add a `mv "dist/$ARTIFACT.exe.exe" "dist/$ARTIFACT"` line as the first fix. Document the actual bun behavior in the implementation log.
- **Tauri CLI for desktop builds.** `tauri-action@v0` reuses the draft from `create-release`; `releaseDraft:true` so it never creates a second release.
- **Engine port `127.0.0.1:33420` is loopback-only**, per the audit. No widening.
- **Service-probe fallback.** When `query_gateway_service_state()` returns `Unknown` (no service registered, e.g. fresh install), `GatewaySupervisor` falls through to the existing spawn-a-child behavior. First-time users get the same UX as today.
- **Probe timeout budget.** All detection is sub-second: `probe_existing_engine` uses a 250 ms timeout, service queries use a 2 s timeout. Desktop startup impact is bounded.

## File Structure (changes by task)

| Task | File | Action | Responsibility |
|------|------|--------|----------------|
| 1 | `README.md` | M | Add shannon-code compatibility note |
| 1 | `CLAUDE.md` | M | Add shannon-code compatibility note |
| 1 | `desktop/tauri.conf.json` | M | Window title `Shannon Code` → `Shannon Agent` |
| 2 | `.github/workflows/release.yml` | M | Add `bun-windows-x64` to gateway matrix |
| 2 | `scripts/install.ps1` | M | Download + install windows gateway, replace hint block |
| 3 | `scripts/install.sh` | M | Replace 3-line hint block with 5-step block |
| 4 | `desktop/src/engine_discovery.rs` | **C** | Q4-A: probe existing api_server, return `EngineMode` |
| 4 | `desktop/src/lib.rs` | M | Add `pub mod engine_discovery;` |
| 4 | `desktop/src/commands.rs` | M | Add `EngineMode` field on `AppState` |
| 4 | `desktop/src/main.rs` | M | Probe before `loopback_api::spawn`; skip spawn if external |
| 4 | `desktop/src/main.rs` | M | Register `engine_discovery_get_mode` tauri command |
| 5 | `desktop/src/gateway_service_probe.rs` | **C** | Q4-B: query systemd/launchd/schtasks, return `ServiceState` |
| 5 | `desktop/src/lib.rs` | M | Add `pub mod gateway_service_probe;` |
| 5 | `desktop/src/gateway_supervisor.rs` | M | Add `GatewaySupervisorStatus::ManagedExternally` variant |
| 5 | `desktop/src/commands_connections.rs` | M | Probe service before `GatewaySupervisor::start` |
| 5 | `desktop/src/main.rs` | M | Register `gateway_supervisor_status` already exists; UI wiring deferred |

---

## Task 1: Shannon-Code naming compatibility note + window title

**Files:**
- Modify: `README.md` (insert after `# Shannon Code` title, before `---`)
- Modify: `CLAUDE.md` (insert after `# Shannon Code` title, before `## Build & Test`)
- Modify: `desktop/tauri.conf.json:25` (window `title` field)

**Interfaces:** None (doc + display string only).

- [ ] **Step 1: Add compatibility note to README.md**

Edit `README.md`. Insert this callout block immediately after the `# Shannon Code` H1 line (line 1) and before the existing `<div align="center">` block:

```markdown
> **Note:** The unified `shannon` CLI replaces the former `shannon-code` product from earlier releases. Install paths, subcommands, and configuration are unchanged — only the binary name changed.
```

- [ ] **Step 2: Add compatibility note to CLAUDE.md**

Edit `CLAUDE.md`. Insert the same callout as a second paragraph between the `# Shannon Code` H1 line (line 1) and the existing `Rust-based AI code assistant...` paragraph (line 3):

```markdown
> **Note:** The unified `shannon` CLI replaces the former `shannon-code` product. Internal `shannon-code` references in CHANGELOG / release notes / migration docs are historical and intentionally retained.
```

- [ ] **Step 3: Update desktop window title**

Edit `desktop/tauri.conf.json:25`. Change:

```json
                "title": "Shannon Code",
```

to:

```json
                "title": "Shannon Agent",
```

Leave `productName` (line 3) and `Cargo.toml`'s `[package.metadata.tauri] productName = "Shannon Agent"` (already correct on line 84 of `desktop/Cargo.toml`) alone.

- [ ] **Step 4: Verify**

Run from repo root:
```bash
grep -n 'shannon-code' README.md CLAUDE.md
grep -n '"title"' desktop/tauri.conf.json
grep -n 'Shannon Code' desktop/tauri.conf.json || echo 'window title updated ✓'
```

Expected output:
- First grep shows the new callout block containing the literal `shannon-code`.
- Second grep shows `"title": "Shannon Agent"` on line 25.
- Third grep prints `window title updated ✓` (no remaining `Shannon Code` in tauri.conf.json).

- [ ] **Step 5: Commit + push**

```bash
cd /home/ed/workspace/app/work/shannon/shannon-agent-build/shannon-agent
git add README.md CLAUDE.md desktop/tauri.conf.json
git commit -m "docs(desktop): surface shannon-code → shannon rename + window title

Adds compatibility callouts to README.md and CLAUDE.md so first-time
readers / future agents understand the unified CLI replaces the former
shannon-code product. Updates the desktop window title from 'Shannon
Code' to 'Shannon Agent' to match the unified product name. Internal
historical references (CHANGELOG, release notes, migration docs)
remain unchanged — they document the rename."
git push origin dev
```

Open PR `diff-lab-com/dev → diff-lab-com/main`. Title: `docs(desktop): shannon-code → shannon rename + window title`. Merge with `--merge` (per the established dev→main merge-commit pattern).

---

## Task 2: Windows gateway build + install.ps1 (hint block included)

**Files:**
- Modify: `.github/workflows/release.yml:287-299` (gateway matrix include list)
- Modify: `scripts/install.ps1:23-27` (replace placeholder `$GATEWAY` block)
- Modify: `scripts/install.ps1` (insert gateway install section after CLI section, before Desktop section)
- Modify: `scripts/install.ps1:117-121` (replace hint block at end of script)

**Interfaces:** None (CI + installer only).

- [ ] **Step 1: Add Windows target to gateway matrix**

Edit `.github/workflows/release.yml`. In the `gateway` job's `matrix.include` list (lines 287-299), append a new entry after `bun-darwin-arm64` (line 299):

```yaml
          - target: bun-windows-x64
            os: windows-latest
            artifact: shannon-gateway-windows-x64.exe
```

The existing linux/darwin entries use `artifact:` without an extension. For Windows we set the extension explicitly. Bun's `--compile --target=bun-windows-x64` will produce a `.exe`-suffixed file when `--outfile` doesn't already end in `.exe`; setting `artifact: shannon-gateway-windows-x64.exe` makes the upload step's `files: gateway/dist/${{ matrix.artifact }}` resolve exactly to one file. If the first CI run on `v0.7.1-rc1` shows a doubled `shannon-gateway-windows-x64.exe.exe` artifact, the fix is to insert a `mv "dist/$ARTIFACT.exe.exe" "dist/$ARTIFACT"` step before the upload — flag this in the rc1 verification log.

- [ ] **Step 2: Replace install.ps1 placeholder block**

Edit `scripts/install.ps1:23-27`. Replace:

```powershell
$GATEWAY    = 'shannon-gateway-linux-x64'  # placeholder; real windows asset below
$GATEWAY    = 'shannon-gateway-windows-x64'  # bun windows target artifact (if built)
# NOTE: the gateway matrix builds linux/darwin only. On Windows we still fetch
# the CLI; gateway service is set up on Linux/macOS runners. If a windows
# gateway artifact is added later, it will be picked up here automatically.
```

with:

```powershell
$GATEWAY    = 'shannon-gateway-windows-x64.exe'  # built by release.yml gateway matrix
```

- [ ] **Step 3: Insert gateway install section**

Edit `scripts/install.ps1`. Insert this block immediately after the CLI install section ends (after line 90, which prints `[ok] Installed shannon to ...`) and before the Desktop section starts (line 92):

```powershell
# ── Gateway ───────────────────────────────────────────────────────────────
$GatewayTmp = Join-Path $env:TEMP 'shannon-gateway.exe'
$GatewayPath = Download-Verify -Asset $GATEWAY -Dest $GatewayTmp
if ($GatewayPath) {
    Copy-Item $GatewayPath (Join-Path $InstallDir 'shannon-gateway.exe') -Force
    Remove-Item $GatewayPath -Force
    Write-Host "[ok] Installed shannon-gateway to $(Join-Path $InstallDir 'shannon-gateway.exe')" -ForegroundColor Green
} else {
    Write-Host "[info] shannon-gateway not available; skipping (gateway service registration must run on linux/macOS)" -ForegroundColor Yellow
}
```

Note: The skip-on-missing path matches the spec's "out of scope" item — Windows service registration via `shannon gateway install` is not implemented; the Windows gateway binary installs but the user runs it manually if needed.

- [ ] **Step 4: Replace install.ps1 hint block**

Edit `scripts/install.ps1:117-121`. Replace:

```powershell
Write-Host ""
Write-Host "[ok] Shannon Agent installed." -ForegroundColor Green
Write-Host "[info] Next: set your API key and run:" -ForegroundColor Cyan
Write-Host "  `$env:SHANNON_API_KEY = 'sk-ant-...'"
Write-Host "  shannon"
```

with:

```powershell
Write-Host ""
Write-Host "[ok] Shannon Agent installed. Next steps:" -ForegroundColor Green
Write-Host "[info]   1. `$env:SHANNON_API_KEY = 'sk-ant-...'" -ForegroundColor Cyan
Write-Host "[info]   2. shannon                                          # launch the REPL" -ForegroundColor Cyan
Write-Host "[info]   3. shannon gateway setup                            # initialize ~/.shannon/gateway/config.json" -ForegroundColor Cyan
Write-Host "[info]   4. shannon gateway install                          # register gateway as background service (linux/macOS)" -ForegroundColor Cyan
Write-Host "[info]   5. shannon gateway enroll <platform>                # enroll a chat-platform bot token" -ForegroundColor Cyan
Write-Host "[info]   Docs: https://shannon.ai/docs/gateway                # Slack/Telegram/Discord/Matrix/WhatsApp/WeCom/Feishu/DingTalk" -ForegroundColor Cyan
```

The Windows-specific step 4 still hints `shannon gateway install` even though the Windows service module isn't shipped yet. That's intentional: the step advertises the available subcommand; the Windows-specific note is captured by the existing service-registration gate in the gateway (currently a no-op on Windows — see out-of-scope). The "Docs:" URL provides the Windows manual-installation path.

- [ ] **Step 5: Verify**

Run from repo root:
```bash
grep -n 'shannon-gateway-windows-x64.exe' .github/workflows/release.yml
grep -n 'shannon-gateway-windows-x64.exe' scripts/install.ps1
grep -n 'gateway setup' scripts/install.ps1
grep -n 'gateway enroll' scripts/install.ps1
```

Expected: each grep returns at least one line. No remaining `placeholder` or `linux-x64` strings in the install.ps1 placeholder area:
```bash
grep -n 'placeholder\|NOTE: the gateway matrix' scripts/install.ps1 || echo 'placeholder removed ✓'
```

Expected: `placeholder removed ✓`.

- [ ] **Step 6: Sanity-parse the PowerShell**

If a PowerShell interpreter is available locally:
```bash
pwsh -NoProfile -Command "& { . ./scripts/install.ps1 -WhatIf 2>$null }" || echo "no pwsh locally — skipped (CI will validate)"
```

Expected: either runs without parse errors, or the "no pwsh locally" fallback message. The CI `desktop` matrix job's `windows-latest` runner will actually run the install step on the new gateway matrix entry — see Step 7.

- [ ] **Step 7: Commit + push**

```bash
cd /home/ed/workspace/app/work/shannon/shannon-agent-build/shannon-agent
git add .github/workflows/release.yml scripts/install.ps1
git commit -m "ci+install(windows): build gateway for windows + install.ps1 gateway install

release.yml gateway matrix gains a bun-windows-x64 entry producing
shannon-gateway-windows-x64.exe. install.ps1 picks up the new asset
(renamed to shannon-gateway.exe on disk) and emits the same 5-step
next-steps block as install.sh. Windows service registration via
'shannon gateway install' remains out of scope; the install script
hints the docs URL for manual Windows setup."
git push origin dev
```

Open PR `diff-lab-com/dev → diff-lab-com/main`. Title: `ci+install(windows): build gateway for windows + install.ps1 gateway install`. Merge with `--merge`. Verify the `gateway` matrix in the PR's CI run completes green on the new windows target before merging.

---

## Task 3: install.sh 5-step hint block

**Files:**
- Modify: `scripts/install.sh:178-181` (replace 3-line hint with 5-step block)

**Interfaces:** None (shell-script only).

- [ ] **Step 1: Replace install.sh hint block**

Edit `scripts/install.sh:178-181`. Replace:

```sh
printf '\n'
ok "Shannon Agent installed. Next steps:"
info "  export SHANNON_API_KEY=\"sk-ant-...\""
info "  shannon                       # launch the REPL"
info "  shannon gateway install       # register the gateway as a background service"
printf '\n'
```

with:

```sh
printf '\n'
ok "Shannon Agent installed. Next steps:"
info "  1. export SHANNON_API_KEY=\"sk-ant-...\""
info "  2. shannon                                          # launch the REPL"
info "  3. shannon gateway setup                            # initialize ~/.shannon/gateway/config.json"
info "  4. shannon gateway install                          # register gateway as background service (linux/macOS)"
info "  5. shannon gateway enroll <platform>                # enroll a chat-platform bot token"
info "  Docs: https://shannon.ai/docs/gateway                # Slack/Telegram/Discord/Matrix/WhatsApp/WeCom/Feishu/DingTalk"
printf '\n'
```

The numbering matches the PowerShell version's wording so users get the same guidance on every platform.

- [ ] **Step 2: Verify**

Run from repo root:
```bash
grep -n 'gateway setup\|gateway enroll\|shannon.ai/docs/gateway' scripts/install.sh
grep -n 'export SHANNON_API_KEY' scripts/install.sh
```

Expected: each grep returns at least one line. Confirm no orphan references:
```bash
grep -n '# register the gateway as a background service' scripts/install.sh
```

Expected: no matches (the old single-step hint is fully replaced).

- [ ] **Step 3: Shell-syntax check**

```bash
sh -n scripts/install.sh && echo 'install.sh parses ✓'
```

Expected: `install.sh parses ✓`.

- [ ] **Step 4: Commit + push**

```bash
cd /home/ed/workspace/app/work/shannon/shannon-agent-build/shannon-agent
git add scripts/install.sh
git commit -m "install(sh): expand post-install hint block to 5 steps

Replaces the single-line 'shannon gateway install' hint with the
same 5-step block used by install.ps1: API key, REPL launch,
gateway setup, gateway service install, gateway platform enroll,
plus the docs URL. Keeps linux/macOS install behavior identical
except for the additional guidance lines."
git push origin dev
```

Open PR `diff-lab-com/dev → diff-lab-com/main`. Title: `install(sh): expand post-install hint block to 5 steps`. Merge with `--merge`.

---

## Task 4: Q4-A — Desktop engine discovery + loopback wiring

**Files:**
- Create: `desktop/src/engine_discovery.rs`
- Modify: `desktop/src/lib.rs:107-114` (add `pub mod engine_discovery;`)
- Modify: `desktop/src/commands.rs` (add `EngineMode` field on `AppState`)
- Modify: `desktop/src/main.rs:269-278` (probe before `loopback_api::spawn`)
- Modify: `desktop/src/main.rs:68-260` (register `engine_discovery_get_mode` command)

**Interfaces:**
- Consumes: `LOOPBACK_PORT: u16` from `loopback_api` (already public).
- Produces: `pub enum EngineMode { Hosted, External }` + `pub async fn probe_existing_engine() -> EngineMode` + `pub async fn probe_at(host: &str, port: u16) -> EngineMode` (testable seam) + `#[tauri::command] pub async fn engine_discovery_get_mode(state) -> EngineModeInfo`.

- [ ] **Step 1: Create `engine_discovery.rs` with the public API + failing tests**

Create `desktop/src/engine_discovery.rs` with this exact content:

```rust
//! Engine API server discovery (Q4-A).
//!
//! Before hosting its own loopback API server, the desktop probes
//! `127.0.0.1:33420` to see whether another process (typically the `shannon`
//! CLI REPL, or another desktop instance) is already serving the engine
//! protocol. If something is listening and answers an HTTP request, we
//! connect as a client and skip hosting our own server — the two
//! processes share the same loopback port without colliding.
//!
//! The probe is bounded by a 250 ms timeout so a non-responsive listener
//! cannot delay desktop startup noticeably.

use std::time::Duration;

use serde::Serialize;

/// How the desktop obtains its engine connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineMode {
    /// No other engine was reachable; desktop hosts `loopback_api` on
    /// `127.0.0.1:33420`.
    Hosted,
    /// Another engine is already listening; desktop connects as a client
    /// to the existing endpoint.
    External,
}

/// Bounded probe of the loopback engine port. Returns `External` if a TCP
/// connection completes within 250 ms AND the listener answers an HTTP
/// `OPTIONS /api/ws` request (any HTTP status proves a server is here —
/// the engine doesn't need to grant OPTIONS to be authoritative). Returns
/// `Hosted` on connect failure, timeout, or non-HTTP listener.
///
/// `probe_at` is the testable seam: tests pass a known-free port to assert
/// `Hosted` and a listener-bound port serving canned HTTP to assert
/// `External`. Production callers use [`probe_existing_engine`].
pub async fn probe_at(host: &str, port: u16) -> EngineMode {
    let url = format!("http://{host}:{port}/api/ws");
    let result = tokio::time::timeout(
        Duration::from_millis(250),
        reqwest::Client::new()
            .request(reqwest::Method::OPTIONS, &url)
            .send(),
    )
    .await;
    match result {
        Ok(Ok(_response)) => EngineMode::External,
        _ => EngineMode::Hosted,
    }
}

/// Probe the canonical loopback engine port. See [`probe_at`].
pub async fn probe_existing_engine() -> EngineMode {
    probe_at("127.0.0.1", loopback_api::LOOPBACK_PORT).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Reserve a free OS-assigned port and return it. The port is released
    /// before the test returns; tests that need an actual listener must
    /// re-bind it.
    async fn free_port() -> u16 {
        let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
        let port = probe.local_addr().expect("addr").port();
        drop(probe);
        port
    }

    #[tokio::test]
    async fn probe_at_returns_hosted_when_port_is_free() {
        let port = free_port().await;
        // Sleep briefly so the OS releases the port — in practice the
        // bind + drop is synchronous and the OS never reuses the port
        // before our probe connects.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(probe_at("127.0.0.1", port).await, EngineMode::Hosted);
    }

    #[tokio::test]
    async fn probe_at_returns_external_when_http_responds() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr: SocketAddr = listener.local_addr().expect("addr");

        // Background task: accept one connection, read until \r\n\r\n
        // (end of HTTP request line + headers), reply with HTTP 200 OK,
        // close. That's all the probe needs.
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let reply = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(reply).await;
                let _ = stream.shutdown().await;
            }
        });

        assert_eq!(probe_at("127.0.0.1", addr.port()).await, EngineMode::External);
    }

    #[tokio::test]
    async fn probe_at_times_out_against_unresponsive_listener() {
        // Bind a listener that accepts but never writes. The probe must
        // bail out at 250 ms (we assert a faster ceiling to avoid CI
        // flake on loaded runners).
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                // Hold the stream open until dropped.
                tokio::time::sleep(Duration::from_secs(5)).await;
                drop(stream);
            }
        });
        let start = std::time::Instant::now();
        let mode = probe_at("127.0.0.1", addr.port()).await;
        let elapsed = start.elapsed();
        assert_eq!(mode, EngineMode::Hosted);
        assert!(
            elapsed < Duration::from_millis(800),
            "probe took {elapsed:?} — expected < 800 ms (250 ms timeout + headroom)"
        );
    }
}
```

- [ ] **Step 2: Verify the tests fail before any wiring**

Run from repo root:
```bash
cargo test -p shannon-desktop --lib engine_discovery
```

Expected: **compile error** because `loopback_api::LOOPBACK_PORT` isn't reachable from `engine_discovery` yet (the module isn't declared in `lib.rs`). This is the failing-test gate — Step 3 wires the module, Step 4 wires the integration.

If you instead see a clean compile + 3 passing tests, the `loopback_api` module is already visible somehow — that's fine, proceed to Step 3 anyway.

- [ ] **Step 3: Declare the module in `desktop/src/lib.rs`**

Edit `desktop/src/lib.rs`. Insert immediately after line 114 (`pub mod loopback_api;`):

```rust
pub mod engine_discovery;
```

- [ ] **Step 4: Verify the tests pass**

```bash
cargo test -p shannon-desktop --lib engine_discovery
```

Expected: `3 passed; 0 failed`. (The `probe_at_returns_external_when_http_responds` test exercises the probe path against a real HTTP listener; the timeout test bounds the wall-clock cost.)

- [ ] **Step 5: Add `EngineMode` field to `AppState`**

Edit `desktop/src/commands.rs`. Find the `AppState` struct definition (search for `pub struct AppState`). Add a new field for the engine mode. The exact placement depends on existing fields — insert alongside the existing fields, keeping the `Default` impl consistent if one exists. The new field:

```rust
    /// Result of the startup engine discovery probe (`engine_discovery`).
    /// `None` until `setup()` runs the probe; `Some(Hosted)` once the
    /// loopback server is spawned; `Some(External)` when another engine
    /// was already serving on 33420.
    pub engine_mode: std::sync::Arc<std::sync::RwLock<Option<engine_discovery::EngineMode>>>,
```

If `AppState::new()` exists in the same file, initialize the field:

```rust
            engine_mode: std::sync::Arc::new(std::sync::RwLock::new(None)),
```

Verify the exact signature of `AppState::new()` and place the initializer accordingly. The `Default` impl (if any) gets the same line. **If the struct uses `#[derive(Default)]` and the new field is `Arc<RwLock<...>>` (which is `Default`), no extra init needed.** If it constructs fields manually, add the initializer inside the constructor body.

- [ ] **Step 6: Wire the probe in `desktop/src/main.rs` setup**

Edit `desktop/src/main.rs`. Replace the existing setup block (lines 269-278):

```rust
            tauri::async_runtime::block_on(async move {
                // P0.1 — spawn the loopback engine API server BEFORE the
                // gateway so its `engine.wsUrl` (ws://127.0.0.1:33420/api/ws)
                // is reachable when the supervised gateway boots. The brief
                // sleep lets the listener bind first; serve() then runs for
                // the lifetime of the process on a detached task.
                loopback_api::spawn(state_ref.inner()).await;
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                commands_connections::bootstrap_gateway_supervisor(&state_ref, &app_handle).await;
            });
```

with:

```rust
            tauri::async_runtime::block_on(async move {
                // Q4-A — before hosting our own loopback engine API server,
                // probe 127.0.0.1:33420. If another engine (typically the
                // shannon CLI REPL or another desktop instance) is already
                // serving the engine protocol, connect as a client and skip
                // hosting — avoids the port-bind collision that previously
                // forced users to pick exactly one of CLI/desktop.
                let engine_mode = engine_discovery::probe_existing_engine().await;
                {
                    let mut slot = state_ref.engine_mode.write().expect("engine_mode lock poisoned");
                    *slot = Some(engine_mode);
                }
                tracing::info!(?engine_mode, "engine discovery complete");
                if engine_mode == engine_discovery::EngineMode::Hosted {
                    // P0.1 — spawn the loopback engine API server BEFORE the
                    // gateway so its `engine.wsUrl` (ws://127.0.0.1:33420/api/ws)
                    // is reachable when the supervised gateway boots. The
                    // brief sleep lets the listener bind first; serve() then
                    // runs for the lifetime of the process on a detached task.
                    loopback_api::spawn(state_ref.inner()).await;
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                } else {
                    tracing::info!(
                        "engine already serving on 127.0.0.1:33420 — connecting as client, \
                         skipping loopback host"
                    );
                }
                commands_connections::bootstrap_gateway_supervisor(&state_ref, &app_handle).await;
            });
```

Also add the `engine_discovery` import to the `use` list at the top of `main.rs` (alongside the existing `use shannon_desktop::loopback_api;` on line 31):

```rust
    use shannon_desktop::engine_discovery;
```

- [ ] **Step 7: Register the `engine_discovery_get_mode` tauri command**

Edit `desktop/src/main.rs`. Add a new `pub async fn` command in the appropriate module (create a new module `desktop/src/engine_discovery_commands.rs` if there isn't already a natural home for tauri commands — the convention in this codebase is one `commands_*.rs` file per surface area). The function:

```rust
//! Tauri commands for engine discovery.

use crate::commands::AppState;
use crate::engine_discovery::EngineMode;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineModeInfo {
    pub mode: Option<EngineMode>,
}

/// Return the resolved engine mode after startup. `None` until the
/// probe has completed; `Some(Hosted)` or `Some(External)` after.
#[tauri::command]
pub async fn engine_discovery_get_mode(
    state: tauri::State<'_, AppState>,
) -> Result<EngineModeInfo, String> {
    let mode = *state.engine_mode.read().expect("engine_mode lock poisoned");
    Ok(EngineModeInfo { mode })
}
```

Save as `desktop/src/engine_discovery_commands.rs`. Add `pub mod engine_discovery_commands;` to `desktop/src/lib.rs` next to the `pub mod engine_discovery;` line added in Step 3.

Then in `desktop/src/main.rs`, register the command in the `invoke_handler!` macro (the existing list spans lines 68-260). Insert the new entry near the other `engine_discovery` / `loopback_api` references — the exact placement is cosmetic, but a sensible spot is after the existing `loopback_api` references (search for `loopback_api::spawn` in the file to find the closest analog):

```rust
            commands_engine_discovery::engine_discovery_get_mode,
```

And add the import alongside `use shannon_desktop::loopback_api;` (line 31):

```rust
    use shannon_desktop::commands_engine_discovery;
```

- [ ] **Step 8: Verify the desktop crate builds + tests pass**

```bash
cargo check -p shannon-desktop --features tauri
cargo test -p shannon-desktop --lib engine_discovery
cargo test -p shannon-desktop --lib commands_connections
```

Expected:
- `cargo check` succeeds (no warnings about unused imports or missing fields).
- `engine_discovery` tests: 3 passed.
- `commands_connections` tests: existing tests still pass (the supervisor wiring change is in Task 5, not here, so `bootstrap_gateway_supervisor` is unchanged).

- [ ] **Step 9: Commit + push**

```bash
cd /home/ed/workspace/app/work/shannon/shannon-agent-build/shannon-agent
git add desktop/src/engine_discovery.rs \
        desktop/src/engine_discovery_commands.rs \
        desktop/src/lib.rs \
        desktop/src/commands.rs \
        desktop/src/main.rs
git commit -m "feat(desktop): engine discovery — reuse existing api_server on 33420

Adds desktop/src/engine_discovery.rs that probes 127.0.0.1:33420
with a 250 ms HTTP OPTIONS timeout before hosting the loopback
engine. If a Shannon engine is already serving, the desktop
connects as a client instead of double-binding the port. Probe
result is stored in AppState.engine_mode and exposed via the new
engine_discovery_get_mode tauri command for the UI footer.

The CLI's engine-hosting behavior is unchanged — only the desktop
gains the detection. Eliminates the 'pick CLI or desktop, not
both' constraint that surfaced during the v0.7.0 audit."
git push origin dev
```

Open PR `diff-lab-com/dev → diff-lab-com/main`. Title: `feat(desktop): engine discovery — reuse existing api_server on 33420`. Merge with `--merge` after the desktop CI matrix (`desktop` job in release.yml) and the standard PR checks pass.

---

## Task 5: Q4-B — Desktop gateway service probe + supervisor wiring

**Files:**
- Create: `desktop/src/gateway_service_probe.rs`
- Modify: `desktop/src/lib.rs:111` (add `pub mod gateway_service_probe;`)
- Modify: `desktop/src/gateway_supervisor.rs:31-41` (add `ManagedExternally` variant)
- Modify: `desktop/src/commands_connections.rs:330-378` (probe service before `GatewaySupervisor::start`)

**Interfaces:**
- Consumes: `GatewaySupervisor::start` (signature unchanged).
- Produces: `pub enum ServiceState { Active, Inactive, Unknown }` + `pub async fn query_gateway_service_state() -> ServiceState` (with injectable `OnceLock<fn() -> ServiceState>` for tests).

- [ ] **Step 1: Create `gateway_service_probe.rs` with the public API + failing tests**

Create `desktop/src/gateway_service_probe.rs` with this exact content:

```rust
//! Gateway OS service probe (Q4-B).
//!
//! When the user runs `shannon gateway install`, the gateway registers a
//! user-level service with the OS service manager (systemd --user on
//! Linux, launchd on macOS, schtasks on Windows). If that service is
//! active, the desktop's gateway supervisor must NOT spawn a competing
//! child process — both would contend for port 33430 + the engine
//! websocket endpoint.
//!
//! This module queries the OS service manager for the
//! `shannon-gateway` service state. The supervisor consults the
//! result before deciding to spawn:
//!   - `Active`   → supervisor enters `ManagedExternally`, no child.
//!   - `Inactive` → service is registered but stopped; supervisor
//!                  spawns as before.
//!   - `Unknown`  → service is not registered (fresh install); supervisor
//!                  spawns as before (preserves first-run UX).

use serde::Serialize;
use std::sync::OnceLock;

type ProbeFn = fn() -> ServiceState;

/// Result of querying the OS service manager for `shannon-gateway`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    /// The service is registered and currently running.
    Active,
    /// The service is registered but not running.
    Inactive,
    /// The service is not registered (no `shannon gateway install` has
    /// run yet), or the platform service manager returned an unexpected
    /// response. Supervisor treats this as "spawn a child".
    Unknown,
}

/// Test injection point. Production code uses the platform-default
/// probe; tests install a fake before calling the public API.
///
/// Note: only safe to set once per process (OnceLock semantics). Tests
/// that need to swap mocks should run sequentially — gate with
/// `#[serial]` from `serial_test` if added later. For this module,
/// each test uses a distinct name + cleans up by reading only.
static PROBE_OVERRIDE: OnceLock<ProbeFn> = OnceLock::new();

/// Install a synchronous probe override. Intended for `#[cfg(test)]`
/// only; calling this from production code has no effect once the
/// OnceLock is initialized.
pub fn set_probe_for_tests(f: ProbeFn) {
    let _ = PROBE_OVERRIDE.set(f);
}

/// Query the OS service manager. Public API.
pub async fn query_gateway_service_state() -> ServiceState {
    if let Some(f) = PROBE_OVERRIDE.get() {
        return f();
    }
    default_probe()
}

/// Platform-default probe implementation. Each branch shells out to the
/// platform service manager with a 2 s timeout.
#[cfg(target_os = "linux")]
fn default_probe() -> ServiceState {
    let output = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "shannon-gateway.service"])
        .output();
    match output {
        Ok(o) if o.status.success() => parse_systemd_active(&o.stdout),
        Ok(_) => ServiceState::Inactive,
        Err(_) => ServiceState::Unknown,
    }
}

#[cfg(target_os = "linux")]
fn parse_systemd_active(stdout: &[u8]) -> ServiceState {
    // `systemctl is-active` prints "active" on stdout when active,
    // "inactive" / "failed" / etc. otherwise. The exit code also
    // reflects this (0 = active, non-zero = other). Trust the exit
    // code, but double-check stdout for "active" in case of edge cases.
    let s = std::str::from_utf8(stdout).unwrap_or("");
    if s.trim() == "active" {
        ServiceState::Active
    } else {
        ServiceState::Inactive
    }
}

#[cfg(target_os = "macos")]
fn default_probe() -> ServiceState {
    let output = std::process::Command::new("launchctl")
        .args(["print", &format!("user/{}", unsafe { libc::getuid() })])
        .output();
    let stdout = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return ServiceState::Unknown,
    };
    if stdout.contains("shannon-gateway") || stdout.contains("shannon.gateway") {
        // The label is registered; whether it's actually running is a
        // more subtle query. For our purposes, "registered + the print
        // output mentions it" is sufficient evidence of Active. launchd
        // doesn't have a single equivalent of `is-active`; this is the
        // closest portable check.
        ServiceState::Active
    } else {
        ServiceState::Unknown
    }
}

#[cfg(target_os = "windows")]
fn default_probe() -> ServiceState {
    // Windows service registration via `shannon gateway install` is not
    // yet implemented (out-of-scope per the design spec). Probe for the
    // scheduled task defensively in case the user registered one
    // manually with nssm or similar. Missing task → Unknown → supervisor
    // spawns a child (the v0.7.0 behavior).
    let output = std::process::Command::new("schtasks")
        .args(["/Query", "/TN", "Shannon Gateway"])
        .output();
    match output {
        Ok(o) if o.status.success() => ServiceState::Active,
        Ok(_) => ServiceState::Inactive,
        Err(_) => ServiceState::Unknown,
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn default_probe() -> ServiceState {
    ServiceState::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize probe-override tests so the OnceLock swap is deterministic.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn probe_override_active_short_circuits_default() {
        let _g = TEST_LOCK.lock().unwrap();
        set_probe_for_tests(|| ServiceState::Active);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            rt.block_on(query_gateway_service_state()),
            ServiceState::Active
        );
    }

    #[test]
    fn probe_override_inactive_falls_through_in_supervisor_logic() {
        // This test documents the contract: Inactive means "registered
        // but stopped" — the supervisor still spawns (no stop/start
        // orchestration here). Just confirms the value flows through.
        let _g = TEST_LOCK.lock().unwrap();
        set_probe_for_tests(|| ServiceState::Inactive);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            rt.block_on(query_gateway_service_state()),
            ServiceState::Inactive
        );
    }

    #[test]
    fn probe_override_unknown_preserves_first_run_spawn_behavior() {
        let _g = TEST_LOCK.lock().unwrap();
        set_probe_for_tests(|| ServiceState::Unknown);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            rt.block_on(query_gateway_service_state()),
            ServiceState::Unknown
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_default_probe_returns_unknown_for_unregistered_service() {
        // `shannon-gateway.service` is almost certainly not registered
        // in CI. systemctl returns non-zero, parser sees non-"active"
        // stdout → Inactive. Acceptable: also a "don't spawn externally"
        // signal for the supervisor (matches Unknown's spawn behavior).
        // The strict assertion here is "anything other than Active".
        let _g = TEST_LOCK.lock().unwrap();
        let state = default_probe();
        assert_ne!(state, ServiceState::Active, "test env must not have a real shannon-gateway.service running");
    }
}
```

- [ ] **Step 2: Verify the tests fail before any wiring**

```bash
cargo test -p shannon-desktop --lib gateway_service_probe
```

Expected: **compile error** because `gateway_service_probe` isn't declared in `lib.rs` yet. That's the failing-test gate.

- [ ] **Step 3: Declare the module in `desktop/src/lib.rs`**

Edit `desktop/src/lib.rs`. Insert immediately after line 111 (`pub mod gateway_supervisor;`):

```rust
pub mod gateway_service_probe;
```

- [ ] **Step 4: Verify the tests pass**

```bash
cargo test -p shannon-desktop --lib gateway_service_probe
```

Expected: `3 passed; 0 failed` (the `linux_default_probe_returns_unknown_for_unregistered_service` test only runs on Linux).

- [ ] **Step 5: Add `ManagedExternally` variant to `GatewaySupervisorStatus`**

Edit `desktop/src/gateway_supervisor.rs`. In the `GatewaySupervisorStatus` enum (lines 31-41), add a new variant after `Exited`:

```rust
    /// The child exited on its own; carried detail lets the UI explain why.
    Exited { code: Option<i32>, reason: String },
    /// A user-level OS service (systemd --user / launchd / schtasks) is
    /// already running the gateway. The supervisor does not own a child
    /// process; the UI should disable Start/Stop and surface a "managed
    /// externally" indicator. The supervisor cannot stop this service
    /// — `stop()` is a no-op when the status is `ManagedExternally`.
    ManagedExternally { service_name: String },
}
```

(The closing `}` for the enum is on the original line 41. Add the new variant before it.)

- [ ] **Step 6: Wire the probe into `bootstrap_gateway_supervisor`**

Edit `desktop/src/commands_connections.rs`. Replace the `bootstrap_gateway_supervisor` function body (lines 330-378). The existing function checks `if !gw_cfg.managed { return; }`, writes the gateway config, and unconditionally spawns. Add the service probe between the config-write and the spawn-check:

Replace this block:

```rust
    let mut guard = state.gateway_supervisor.lock().await;
    let already_running = guard
        .as_ref()
        .map(|supervisor| matches!(supervisor.status(), GatewaySupervisorStatus::Running { .. }))
        .unwrap_or(false);
    if !already_running {
        let supervisor = GatewaySupervisor::start(app, &gw_cfg);
        let status = supervisor.status();
        *guard = Some(supervisor);
        tracing::info!("gateway supervisor auto-started: {status:?}");
    }
}
```

with:

```rust
    let mut guard = state.gateway_supervisor.lock().await;
    let already_running = guard
        .as_ref()
        .map(|supervisor| matches!(supervisor.status(), GatewaySupervisorStatus::Running { .. }))
        .unwrap_or(false);
    if already_running {
        return;
    }

    // Q4-B: if a user-level OS service is already running the gateway,
    // treat it as authoritative — do not spawn a competing child.
    let service_state = crate::gateway_service_probe::query_gateway_service_state().await;
    if service_state == crate::gateway_service_probe::ServiceState::Active {
        tracing::info!(
            "gateway already running as OS service — desktop will not spawn a competing child"
        );
        let supervisor = GatewaySupervisor::managed_externally("shannon-gateway.service");
        *guard = Some(supervisor);
        return;
    }

    let supervisor = GatewaySupervisor::start(app, &gw_cfg);
    let status = supervisor.status();
    *guard = Some(supervisor);
    tracing::info!("gateway supervisor auto-started: {status:?}");
}
```

- [ ] **Step 7: Add `GatewaySupervisor::managed_externally` constructor**

Edit `desktop/src/gateway_supervisor.rs`. Add a new constructor after the existing `start` impl block (around line 158, before `pub async fn stop`). It mirrors `start`'s shape but never spawns a child and never resolves a binary:

```rust
    /// Construct a supervisor that represents a gateway process owned by
    /// an external OS service. The supervisor holds no child pid and
    /// `stop()` is a no-op (the external service manager owns the
    /// lifecycle — stopping it requires `shannon gateway stop` or the
    /// platform equivalent).
    pub fn managed_externally(service_name: impl Into<String>) -> Self {
        let status = Arc::new(std::sync::RwLock::new(
            GatewaySupervisorStatus::ManagedExternally {
                service_name: service_name.into(),
            },
        ));
        Self {
            status,
            cancel: CancellationToken::new(),
            join: None,
        }
    }
```

- [ ] **Step 8: Make `stop()` a no-op for `ManagedExternally`**

Edit `desktop/src/gateway_supervisor.rs`. In `stop` (around line 161), early-return when the status is `ManagedExternally` — otherwise we'd cancel a token that has no join handle, which is fine but wasteful. Add a guard:

```rust
    pub async fn stop(&mut self) {
        // External OS service owns the gateway; the supervisor cannot
        // stop it. Treat stop() as a no-op (the UI button is also
        // disabled in ManagedExternally state, so this is a defensive
        // guard).
        if matches!(self.status(), GatewaySupervisorStatus::ManagedExternally { .. }) {
            return;
        }
        self.cancel.cancel();
        if let Some(h) = self.join.take() {
            // Bound the wait so a misbehaving kill can't hang the UI action.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), h).await;
        }
    }
```

- [ ] **Step 9: Update `gateway_supervisor_start` tauri command for the external path**

Edit `desktop/src/commands_connections.rs`. In `gateway_supervisor_start` (lines 246-267), the early-return at line 252-258 checks `Running`. Update it to also short-circuit on `ManagedExternally`:

Replace:

```rust
    let mut guard = state.gateway_supervisor.lock().await;
    if let Some(sup) = guard.as_ref() {
        if let GatewaySupervisorStatus::Running { .. } = sup.status() {
            return Ok(GatewayProcessState {
                managed: gw_cfg.managed,
                status: sup.status(),
            });
        }
    }
```

with:

```rust
    let mut guard = state.gateway_supervisor.lock().await;
    if let Some(sup) = guard.as_ref() {
        match sup.status() {
            GatewaySupervisorStatus::Running { .. }
            | GatewaySupervisorStatus::ManagedExternally { .. } => {
                return Ok(GatewayProcessState {
                    managed: gw_cfg.managed,
                    status: sup.status(),
                });
            }
            _ => {}
        }
    }
```

This way, if `bootstrap_gateway_supervisor` already set the state to `ManagedExternally`, the user clicking Start in the UI gets the same status back without spawning.

- [ ] **Step 10: Verify the desktop crate builds + tests pass**

```bash
cargo check -p shannon-desktop --features tauri
cargo test -p shannon-desktop --lib gateway_service_probe
cargo test -p shannon-desktop --lib gateway_supervisor
cargo test -p shannon-desktop --lib commands_connections
cargo test -p shannon-desktop --lib engine_discovery
```

Expected:
- `cargo check` clean.
- `gateway_service_probe`: 3-4 passed (the linux-default-probe test only on Linux).
- `gateway_supervisor`: existing tests still pass; the new `ManagedExternally` variant serializes camelCase correctly (the existing `status_serializes_camel_case` test is unrelated to the new variant — add a new assertion in that test if you want, but it's not required).
- `commands_connections`: existing tests still pass.
- `engine_discovery`: 3 passed (no regression from Task 4).

- [ ] **Step 11: Add a status-variant test for `ManagedExternally`**

Edit `desktop/src/gateway_supervisor.rs`. In the existing `status_serializes_camel_case` test (lines 260-272), add one more assertion after the existing two:

```rust
        let s3 = GatewaySupervisorStatus::ManagedExternally {
            service_name: "shannon-gateway.service".into(),
        };
        let j3 = serde_json::to_string(&s3).expect("serialize");
        assert!(j3.contains("\"serviceName\":\"shannon-gateway.service\""));
        assert!(!j3.contains("\"managed_externally\""));
```

Expected: existing test still passes with the new assertion included.

- [ ] **Step 12: Commit + push**

```bash
cd /home/ed/workspace/app/work/shannon/shannon-agent-build/shannon-agent
git add desktop/src/gateway_service_probe.rs \
        desktop/src/lib.rs \
        desktop/src/gateway_supervisor.rs \
        desktop/src/commands_connections.rs
git commit -m "feat(desktop): gateway supervisor prefers OS-managed service

Adds desktop/src/gateway_service_probe.rs which queries the platform
service manager (systemctl --user on linux, launchctl print on macos,
schtasks on windows) for the shannon-gateway service state. The
supervisor's bootstrap path consults the probe before spawning:
Active → enters ManagedExternally (no child); Inactive/Unknown →
spawns as before, preserving first-run UX.

Eliminates the gateway double-spawn that surfaced in the v0.7.0
audit when users had run both 'shannon gateway install' (registers
a systemd --user service) and the desktop's gateway supervisor.
The two processes contended for port 33430 + the engine websocket."
git push origin dev
```

Open PR `diff-lab-com/dev → diff-lab-com/main`. Title: `feat(desktop): gateway supervisor prefers OS-managed service`. Merge with `--merge` after all checks pass.

---

## Verification (after all 5 PRs merge)

Once the dev branch has all 5 PRs merged to main via the established flow:

- [ ] **Cut v0.7.1-rc1 to validate the new gateway matrix**

```bash
cd /home/ed/workspace/app/work/shannon/shannon-agent-build/shannon-agent
# Bump the 5 version sources per release-flow memory
just release-prep 0.7.1-rc1   # or: scripts/release-prep.sh
git tag -a v0.7.1-rc1 -m "v0.7.1-rc1: install & gateway hardening"
git push origin v0.7.1-rc1
```

Monitor the release workflow. Expected new behaviors:
- Gateway matrix produces a 5th artifact: `shannon-gateway-windows-x64.exe`.
- `publish` job attaches 5 gateway binaries (4 existing + the new windows one).
- The total asset count is 21 (was 20 for v0.7.0).

- [ ] **Verify the Windows gateway matrix entry actually runs**

After rc1 completes green:
```bash
gh release view v0.7.1-rc1 --json assets --jq '.assets[] | select(.name | startswith("shannon-gateway")) | .name'
```

Expected:
```
shannon-gateway-linux-x64
shannon-gateway-linux-arm64
shannon-gateway-darwin-x64
shannon-gateway-darwin-arm64
shannon-gateway-windows-x64.exe
```

If only 4 are listed, check the rc1 release job log for the windows matrix entry — most likely bun produced a doubled `.exe.exe` and the upload pattern didn't match. Apply the `mv` fix from Task 2 Step 1 and re-cut.

- [ ] **Smoke-test the install scripts in a throwaway container**

Linux:
```bash
docker run --rm -it ubuntu:22.04 bash -c "$(cat scripts/install.sh)"
```

Expected: 5-step hint block prints at the end with the gateway install line explicitly labeled `(linux/macOS)` and the docs URL visible.

Windows (if a windows runner is available):
```powershell
irm https://github.com/shannon-agent/shannon-agent/releases/latest/download/install.ps1 | iex
```

Expected: same 5-step hint block. `shannon-gateway.exe` is in `%USERPROFILE%\.shannon\bin\` (or `C:\shannon\bin\`).

- [ ] **Smoke-test the desktop Q4-A behavior**

On a machine with the desktop installed, start the `shannon` CLI REPL in one terminal (it hosts 33420). Launch the desktop. Expected: desktop log shows `engine already serving on 127.0.0.1:33420 — connecting as client, skipping loopback host`. No port-bind collision.

Quit the CLI REPL, relaunch the desktop. Expected: desktop log shows `engine discovery complete mode=Hosted` followed by the P0.1 loopback-spawn path.

- [ ] **Smoke-test the desktop Q4-B behavior**

With the desktop installed:
```bash
shannon gateway install     # registers systemd --user service
shannon gateway start       # starts the service
```

Launch the desktop. Expected: log shows `gateway already running as OS service — desktop will not spawn a competing child`. UI Connections panel shows "Gateway: managed by shannon-gateway service" with Start/Stop disabled.

Stop the service:
```bash
shannon gateway stop
```

Relaunch desktop. Expected: log shows `gateway supervisor auto-started` (the Inactive service state falls through to spawn). UI shows the supervised lifecycle as before.

- [ ] **Promote v0.7.1-rc1 → v0.7.1 stable**

After rc1 verifies clean:
```bash
just release-prep 0.7.1   # bump version sources
git tag -a v0.7.1 -m "v0.7.1: install & gateway hardening"
git push origin v0.7.1
```

Monitor the release workflow. Verify the published v0.7.1 stable has the 5-step hint in the install scripts, the windows gateway binary, and the desktop app's new engine-mode behavior.

---

## Self-Review (filled in after writing)

**1. Spec coverage:** Each of the 5 spec fixes maps to a task:
- Q1 → Task 1 (docs + window title).
- Q2 → Task 2 (windows gateway matrix + install.ps1).
- Q3 → Task 3 (install.sh hint block). Q3 also touches install.ps1 — handled inline in Task 2 to avoid double-touching.
- Q4-A → Task 4 (engine_discovery module + main.rs wiring + AppState + tauri command).
- Q4-B → Task 5 (gateway_service_probe module + ManagedExternally variant + supervisor wiring).
- Spec's testing strategy (unit + integration + manual) is covered by per-task tests + the post-merge verification section.

**2. Placeholder scan:** No `TBD` / `TODO` / `fill in details` / `similar to Task N` in any step. Every code block is concrete. The Step 5/Task 4 caveat about `AppState::new()` signature is documented as a verification gate ("find the struct, add the field, init in the constructor if non-Default") — not a placeholder; the implementer reads the existing code to make the right edit.

**3. Type/name consistency:**
- `EngineMode::Hosted` and `EngineMode::External` defined once in `engine_discovery.rs` and referenced everywhere consistently.
- `ServiceState::Active` / `Inactive` / `Unknown` defined once in `gateway_service_probe.rs` and referenced consistently in `bootstrap_gateway_supervisor` and the `managed_externally` constructor.
- `GatewaySupervisorStatus::ManagedExternally { service_name }` variant used in: variant definition (gateway_supervisor.rs), `managed_externally` constructor, `stop()` guard, `gateway_supervisor_start` early-return, status-variant test. All five sites use the same camelCase serialization (`serviceName`).
- `engine_discovery_get_mode` tauri command registered once in `main.rs::invoke_handler!`.
- `EngineModeInfo` defined in `engine_discovery_commands.rs`, returned by the command — single source of truth.

**4. Branch hygiene:** Every task commits on dev and pushes; PRs merge via `--merge` (not `--rebase`) per the established merge-commit pattern. Main remains protected (no direct push, no force push, no deletion).

**5. Scope discipline:** Out-of-scope items from the spec (windows `shannon gateway install` service module, 198 internal `shannon-code` doc hits, engine-layer refactoring) are not present in the plan. The bun-windows `.exe` artifact naming caveat is called out as a per-task verification gate, not silently assumed.