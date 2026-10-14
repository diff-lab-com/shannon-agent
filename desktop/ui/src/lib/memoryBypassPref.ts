// design-parity R2 (audit §3.2 M3, design 12-settings-general.html:224-232)
// — 记忆旁路默认: when on, sessions created via the 「新建会话」 action start
// as temporary chats (no memory injection, no auto-extraction).
//
// Why localStorage and not a configure() key: the engine-side investigation
// (2026-10-14) found NO default field on DesktopConfig (config.rs:50-130 —
// it has the global `memory_enabled` master switch only) and the per-session
// flag lives in a dedicated sidecar (~/.shannon/desktop/
// session-memory-bypass.json, session_memory_bypass.rs) written through
// set_session_memory_bypass. The configure command is a strict allowlist
// (commands_config.rs `_ => Err("Unknown config key: …")`), so
// configure('memory_bypass_default') would be rejected engine-side.
// Persistence therefore follows the lib/sendBehaviorPref.ts localStorage
// pattern; AppContext.createSession / createSessionInWorktree apply the
// pref to each just-created session via setSessionMemoryBypass (the same
// per-session pin the composer's toggle writes, ChatInput.tsx:538-548).

export const MEMORY_BYPASS_DEFAULT_KEY = 'shannon.chat.memoryBypassDefault'

/** Read the persisted default. Missing or unrecognized values fall back to
 *  OFF — the historical default (new sessions use memory). */
export function readMemoryBypassDefault(): boolean {
  try {
    return localStorage.getItem(MEMORY_BYPASS_DEFAULT_KEY) === 'true'
  } catch { /* noop */ }
  return false
}

export function setMemoryBypassDefault(on: boolean) {
  try { localStorage.setItem(MEMORY_BYPASS_DEFAULT_KEY, on ? 'true' : 'false') } catch { /* noop */ }
}
