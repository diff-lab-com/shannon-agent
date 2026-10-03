// R1-2 — mock-handler coverage tripwire (TS sibling of the Rust-side
// `desktop/tests/app_command_acl_coverage.rs` tripwire).
//
// Why this exists: demo mode — the e2e webServer (`pnpm demo`,
// VITE_MOCK_MODE=1, see playwright.config.ts) — swaps `@tauri-apps/api/core`
// for `src/lib/mock/coreMock.ts` at build time (vite.config.ts alias). Its
// `invoke()` looks the command up in the `handlers` record
// (`src/lib/mock/handlers.ts`) and THROWS "This feature is not available in
// demo mode" for anything unregistered, leaving only a console.error trace.
// #154 had to add handlers for `get_provider_status` /
// `fetch_provider_models` / `test_provider_credentials` after their absence
// broke 43 e2e specs; the Rust side already guards its own command inventory
// with a tripwire, so this test closes the same gap on the TS side:
//
//   1. the command inventory is re-derived from src by text scan (same
//      philosophy as the Rust tripwire re-deriving `generate_handler!`):
//      every `invoke('cmd')` literal in product code — `src/**/*.{ts,tsx}`
//      minus tests (`__tests__`, `*.test.*`, `*.spec.*`) and the mock layer
//      itself (`src/lib/mock/`, which defines handlers and never invokes).
//      Covers the central `tauri-api.ts` wrappers AND direct `invoke(` sites
//      (hooks/, lib/runtime/);
//   2. every scanned name must exist in `handlers` OR in UNMOCKED_ALLOWLIST
//      below. A missing entry fails the suite and the failure names every
//      offender with its call sites;
//   3. every `invoke(` call site must pass a string literal — a dynamic
//      first argument (variable/template) is invisible to this tripwire, so
//      it is refused outright. Command names are a fixed protocol; select
//      among literals instead of building them at runtime;
//   4. UNMOCKED_ALLOWLIST entries that no longer appear in src are stale and
//      fail (bidirectional rule, mirroring the Rust manifest↔handler diff).
//
// Maintenance path when adding a command: prefer adding a handler in
// handlers.ts so demo mode and e2e can exercise the feature. Only park a
// command in UNMOCKED_ALLOWLIST when demo mode genuinely never reaches it
// (or degrades gracefully), and say why in the entry's comment.

import { readdirSync, readFileSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { handlers } from '@/lib/mock/handlers'

const SRC_ROOT = resolve(process.cwd(), 'src')

/**
 * Command names invoked by product code but deliberately NOT mocked.
 *
 * Started empty per the R1-2 brief, but the first scan proved the
 * "everything is mocked today" assumption wrong: of 280 invoked commands,
 * 187 have handlers and 97 do not (the mock README's "every Tauri command in
 * tauri-api.ts has a handler" claim is aspirational — e2e simply never
 * reaches these 97). Rather than leave the gate red or quietly narrow the
 * scan, the actual inventory is recorded here so:
 *   - a NEW unmocked command still fails this test (must be mocked or
 *     explicitly parked here with a reason), and
 *   - removing a mocked handler still fails (mocked commands are not in
 *     this list — #154's regression class).
 * Entries are grouped by why demo mode tolerates the gap. Shrink this list
 * by adding real handlers, never by editing the scan.
 */
const UNMOCKED_ALLOWLIST: Record<string, string> = {
  // Session/worktree lifecycle — static demo sessions are never branched or
  // attached to real worktrees/session windows. (archive_session /
  // unarchive_session left this list in the W2 chat-testing wave: the
  // sidebar Archive action got a real demo handler + scripted seed registry,
  // same as list_archived_sessions before it.)
  branch_session: 'session mutation on a live engine session',
  set_session_working_dir: 'session mutation on a live engine session',
  reveal_session_in_main: 'multi-window navigation (demo has one window)',
  open_session_window: 'session windows need the real Tauri shell',
  close_session_window: 'session windows need the real Tauri shell',
  list_session_windows: 'session windows need the real Tauri shell',
  create_session_worktree: 'git worktree ops on a real repo checkout',
  create_task_worktree: 'git worktree ops on a real repo checkout',
  remove_task_worktree: 'git worktree ops on a real repo checkout',
  list_task_worktrees: 'git worktree ops on a real repo checkout',
  prune_task_worktrees: 'git worktree ops on a real repo checkout',

  // Inbox triage — operates on engine-owned inbox items.
  archive_triage_item: 'live engine inbox items',
  list_triage_items: 'live engine inbox items',
  mark_triage_read: 'live engine inbox items',
  get_triage_stats: 'live engine inbox items',

  // Dream passes — long-running engine jobs over the real repo.
  apply_dream_proposal: 'dream runs need a live engine + repo',
  discard_dream_proposal: 'dream runs need a live engine + repo',
  list_dream_proposals: 'dream runs need a live engine + repo',
  read_dream_report: 'dream runs need a live engine + repo',
  read_dream_state: 'dream runs need a live engine + repo',
  run_dream_pass: 'dream runs need a live engine + repo',

  // Skill candidates / skill loop / sub-agents — review workflows over
  // engine-proposed artifacts; nothing proposes anything in demo mode.
  approve_skill_candidate: 'engine-proposed artifacts (none in demo)',
  reject_skill_candidate: 'engine-proposed artifacts (none in demo)',
  refine_skill_candidate: 'engine-proposed artifacts (none in demo)',
  list_agent_authored_skills: 'engine-proposed artifacts (none in demo)',
  skill_loop_approve: 'engine-proposed artifacts (none in demo)',
  skill_loop_evaluate: 'engine-proposed artifacts (none in demo)',
  skill_loop_generate: 'engine-proposed artifacts (none in demo)',
  skill_loop_reject: 'engine-proposed artifacts (none in demo)',
  list_subagents: 'sub-agents exist only in live engine runs',

  // Catalog / data sources / extensions / MCP registry + OAuth — network
  // installers and browser-free OAuth loops; demo surfaces the seeded
  // catalog data instead of probing upstreams.
  clear_catalog_report: 'marketplace/catalog probing needs the gateway',
  list_catalog_reports: 'marketplace/catalog probing needs the gateway',
  list_catalog_upstreams: 'marketplace/catalog probing needs the gateway',
  report_catalog_entry: 'marketplace/catalog probing needs the gateway',
  get_extension_stats: 'marketplace/catalog probing needs the gateway',
  list_data_source_adapters: 'data-source adapters are engine-side',
  list_data_source_catalog: 'data-source adapters are engine-side',
  list_installed_data_sources: 'data-source adapters are engine-side',
  install_data_source: 'data-source install writes engine config',
  uninstall_data_source: 'data-source install writes engine config',
  query_data_source: 'data-source queries execute in the engine',
  read_data_source_config: 'data-source queries execute in the engine',
  install_mcp_mcpb: 'MCP installs/OAuth run outside the browser',
  install_mcp_stdio: 'MCP installs/OAuth run outside the browser',
  install_mcp_oauth_authorize_url: 'MCP installs/OAuth run outside the browser',
  install_mcp_oauth_complete: 'MCP installs/OAuth run outside the browser',
  install_mcp_oauth_loopback: 'MCP installs/OAuth run outside the browser',
  list_mcp_registry_servers: 'MCP installs/OAuth run outside the browser',
  uninstall_mcp_server: 'MCP installs/OAuth run outside the browser',

  // Voice / Whisper / STT — model downloads (GB-scale) and OS audio capture
  // have no browser equivalent; the UI degrades via error toasts.
  delete_whisper_model: 'Whisper models/OS audio — no browser equivalent',
  download_whisper_model: 'Whisper models/OS audio — no browser equivalent',
  list_whisper_models: 'Whisper models/OS audio — no browser equivalent',
  get_stt_config: 'Whisper models/OS audio — no browser equivalent',
  save_stt_config: 'Whisper models/OS audio — no browser equivalent',
  get_voice_local_config: 'Whisper models/OS audio — no browser equivalent',
  save_voice_local_config: 'Whisper models/OS audio — no browser equivalent',
  transcribe_audio: 'Whisper models/OS audio — no browser equivalent',
  transcribe_audio_local: 'Whisper models/OS audio — no browser equivalent',
  transcribe_audio_local_base64: 'Whisper models/OS audio — no browser equivalent',

  // Desktop-shell / OS surfaces — updater, CLI installer, secrets, file
  // system, notifications. A browser demo cannot perform any of these and
  // callers handle the rejection.
  check_app_update: 'OS/updater/file surface — browser cannot perform it',
  open_release_page: 'OS/updater/file surface — browser cannot perform it',
  get_cli_install_status: 'OS/updater/file surface — browser cannot perform it',
  install_cli_to_path: 'OS/updater/file surface — browser cannot perform it',
  get_surface_info: 'OS/updater/file surface — browser cannot perform it',
  export_diagnostics: 'OS/updater/file surface — browser cannot perform it',
  gateway_get_secret: 'OS/updater/file surface — browser cannot perform it',
  open_external: 'OS/updater/file surface — browser cannot perform it',
  open_with_default_app: 'OS/updater/file surface — browser cannot perform it',
  open_artifact_externally: 'OS/updater/file surface — browser cannot perform it',
  path_exists: 'OS/updater/file surface — browser cannot perform it',
  read_text_file: 'OS/updater/file surface — browser cannot perform it',
  // (save_text_file left this list in the W2 chat-testing wave: J15 /export
  // and the J20 PlanPanel write-back both need a demo handler — an in-memory
  // store, plus the scripted saveTextFileFails failure fixture so the 计划 tab
  // journey can pin both the write-back and the rollback. No fs behind the mock.)
  save_text_file_via_dialog: 'OS/native save dialog + fs write — browser cannot perform it',
  reveal_in_folder: 'OS/updater/file surface — browser cannot perform it',
  // (run_file_diagnostics left the list: wave-2 J18 gave it a real handler —
  // the chat-inline editor fires it on every file load, and the demo
  // console.error'd on each open. It answers a quiet empty verdict.)
  send_notification: 'OS/updater/file surface — browser cannot perform it',
  read_attachment: 'OS/updater/file surface — browser cannot perform it',
  read_attachments: 'OS/updater/file surface — browser cannot perform it',

  // Permission prompts / slash registry — only fire on live engine turns.
  request_permission: 'only fires on live engine turns',
  detect_slash: 'only fires on live engine turns',

  // Misc one-off engine/gateway probes that demo pages never reach.
  // (register/unregister_interactive_artifact left this list in the W2
  // chat-testing wave: the chat-fence HTML artifact path DOES reach them —
  // the demo handler throws so HtmlRenderer's static-hint fallback is the
  // demo truth, and the failure stays off coreMock's console.error path.)
  get_provider_allowlist: 'engine/gateway probe demo never reaches',
  featured_vendor_to_entry: 'engine/gateway probe demo never reaches',
  verify_signature: 'engine/gateway probe demo never reaches',
  probe_url_frameable: 'engine/gateway probe demo never reaches',
  seed_sample_data: 'engine/gateway probe demo never reaches',
  test_all_providers: 'engine/gateway probe demo never reaches',
  scan_prompt_injection: 'engine/gateway probe demo never reaches',
  scan_prompt_injection_with_readme: 'engine/gateway probe demo never reaches',
  get_webhook_config: 'engine/gateway probe demo never reaches',
  save_webhook_config: 'engine/gateway probe demo never reaches',
  clear_webhook_config: 'engine/gateway probe demo never reaches',
  list_routine_templates: 'engine/gateway probe demo never reaches',
  instantiate_routine_template: 'engine/gateway probe demo never reaches',
}

/** Product source files: src/**, without tests and without the mock layer. */
function productFiles(dir: string, relDir = ''): string[] {
  const out: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name === '__tests__') continue
      // src/lib/mock defines the handlers themselves (coreMock.ts even
      // exports its own `invoke` wrapper) — it is the coverage TARGET, not
      // product call sites, and would self-flag as a dynamic invoke.
      if (relDir === 'lib' && entry.name === 'mock') continue
      out.push(...productFiles(p, relDir ? `${relDir}/${entry.name}` : entry.name))
    } else if (/\.(ts|tsx)$/.test(entry.name) && !/\.(test|spec)\.[jt]sx?$/.test(entry.name)) {
      out.push(p)
    }
  }
  return out
}

/**
 * Blank out comments while preserving every character position (spaces in
 * place of comment text, newlines kept), so regex scans over the result keep
 * real line numbers. String literals are kept verbatim — command names live
 * in them. Plain state machine, so a regex literal containing `//` hides the
 * rest of its line from the scan; keep `invoke(` calls off such lines (they
 * are one-per-line statements throughout src today).
 */
function stripComments(src: string): string {
  const out = src.split('')
  const n = src.length
  let i = 0
  while (i < n) {
    const c = src[i]
    if (c === '/' && src[i + 1] === '/') {
      while (i < n && src[i] !== '\n') { out[i] = ' '; i++ }
    } else if (c === '/' && src[i + 1] === '*') {
      while (i < n && !(src[i] === '*' && src[i + 1] === '/')) {
        if (src[i] !== '\n') out[i] = ' '
        i++
      }
      if (i < n) { out[i] = ' '; out[i + 1] = ' '; i += 2 }
    } else if (c === '\'' || c === '"' || c === '`') {
      i++
      while (i < n) {
        if (src[i] === '\\') { i += 2; continue }
        if (src[i] === c) { i++; break }
        i++
      }
    } else {
      i++
    }
  }
  return out.join('')
}

function lineAt(text: string, index: number): number {
  let line = 1
  for (let i = 0; i < index; i++) if (text[i] === '\n') line++
  return line
}

interface CallSite { name: string | null; location: string }

/** Extract every `invoke('cmd')` literal in one file; non-literals → null. */
function scanFile(relPath: string, raw: string): CallSite[] {
  const stripped = stripComments(raw)
  const sites: CallSite[] = []
  const call = /\binvoke\s*([<(])/g
  let m: RegExpExecArray | null
  while ((m = call.exec(stripped)) !== null) {
    let i = m.index + m[0].length
    if (m[1] === '<') {
      // Skip a (possibly nested) generic parameter, then expect `(`.
      let depth = 1
      while (i < stripped.length && depth > 0) {
        if (stripped[i] === '<') depth++
        else if (stripped[i] === '>') depth--
        i++
      }
      while (i < stripped.length && /\s/.test(stripped[i])) i++
      if (stripped[i] !== '(') {
        sites.push({ name: null, location: `${relPath}:${lineAt(stripped, m.index)}` })
        continue
      }
      i++
    }
    while (i < stripped.length && /\s/.test(stripped[i])) i++
    const q = stripped[i]
    if (q === '\'' || q === '"' || q === '`') {
      let name = ''
      let j = i + 1
      while (j < stripped.length && stripped[j] !== q) {
        if (stripped[j] === '\\') j++
        if (j < stripped.length && stripped[j] !== q) { name += stripped[j]; j++ }
      }
      sites.push({ name, location: `${relPath}:${lineAt(stripped, m.index)}` })
    } else {
      sites.push({ name: null, location: `${relPath}:${lineAt(stripped, m.index)}` })
    }
  }
  return sites
}

const invokedByName = new Map<string, string[]>()
const dynamicCalls: string[] = []
for (const file of productFiles(SRC_ROOT)) {
  const rel = relative(SRC_ROOT, file)
  for (const site of scanFile(rel, readFileSync(file, 'utf8'))) {
    if (site.name === null) {
      dynamicCalls.push(`${site.location} — invoke() without a string-literal command name`)
    } else {
      const locs = invokedByName.get(site.name) ?? []
      locs.push(site.location)
      invokedByName.set(site.name, locs)
    }
  }
}

const handlerKeys = new Set(Object.keys(handlers))
const missing = [...invokedByName.keys()]
  .filter((name) => !handlerKeys.has(name) && !(name in UNMOCKED_ALLOWLIST))
  .sort()
const staleAllowlist = Object.keys(UNMOCKED_ALLOWLIST)
  .filter((name) => !invokedByName.has(name))
  .sort()

describe('mock-handler coverage tripwire (R1-2)', () => {
  it('scans a real inventory (non-empty, #154 canaries present)', () => {
    // Guards against the scan silently no-op-ing (path/layout regression):
    // a zero-command or canary-less inventory must fail, not pass vacuously.
    expect(invokedByName.size).toBeGreaterThanOrEqual(50)
    expect(invokedByName.has('send_message')).toBe(true)
    expect(invokedByName.has('get_provider_status')).toBe(true)
    expect(handlerKeys.size).toBeGreaterThanOrEqual(50)
  })

  it('every invoke() call site passes a string-literal command name', () => {
    // Dynamic names are invisible to the coverage scan below — refuse them
    // so coverage can never be bypassed by `invoke(cmdFromVariable)`.
    expect(
      dynamicCalls,
      `\n${dynamicCalls.length} dynamic invoke() call site(s) — inline the command literal so the tripwire can see it:\n${dynamicCalls.map((d) => `  - ${d}`).join('\n')}`,
    ).toEqual([])
  })

  it('every invoked Tauri command has a mock handler or is explicitly allowlisted', () => {
    const detail = missing
      .map((name) => `  - ${name}\n      invoked at:\n${invokedByName.get(name)!.map((loc) => `        ${loc}`).join('\n')}`)
      .join('\n')
    expect(
      missing,
      `\n${missing.length} invoked Tauri command(s) have no mock handler — demo mode (pnpm demo / Desktop E2E) throws "not available in demo mode" for them. Add each to src/lib/mock/handlers.ts, or justify it in UNMOCKED_ALLOWLIST in this test:\n${detail}`,
    ).toEqual([])
  })

  it('UNMOCKED_ALLOWLIST has no stale entries', () => {
    expect(
      staleAllowlist,
      `\n${staleAllowlist.length} allowlist entr(y|ies) no longer appear in any invoke() call — remove them so the allowlist stays an exact inventory:\n${staleAllowlist.map((s) => `  - ${s}`).join('\n')}`,
    ).toEqual([])
  })
})
