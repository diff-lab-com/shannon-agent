// 启动时检查更新 (批 1) — the launch-time, check-only update probe.
//
// Contract (C1① semi-automatic updates, ADR-0011): the app NEVER installs
// anything automatically. This trigger runs the existing `check_app_update`
// command (the very same GitHub Releases compare the About pane's
// 「检查更新」button drives) ONCE per app run, after launch, and — only when
// a newer release exists — surfaces the About pane's existing
// 「发现新版本」toast. No update → silence (non-intrusive); any failure
// (offline, old backend, ACL denial) → silence too: a launch-time nicety
// must never toast an error or block boot.
//
// The gate is `update_check_at_launch` (default ON; Settings → 通用 → 启动).
// It is read fresh here rather than through CatalogContext so the check does
// not wait on (or race) the catalog's bootstrap.

import { toast } from 'sonner'
import { messageFor } from '@/i18n'
import * as api from '@/lib/tauri-api'

/** One shot per app run — session windows and the main window share the
 *  module instance only within one webview, which is exactly the scope of
 *  "once at launch" (each window checks its own config, harmless duplicate
 *  reads, at most one toast per window). */
let ranThisRun = false

export async function runStartupUpdateCheck(apiImpl: Pick<typeof api, 'getConfig' | 'checkAppUpdate'> = api): Promise<void> {
  if (ranThisRun) return
  ranThisRun = true
  try {
    const config = await apiImpl.getConfig()
    // Absent key / old backend → default ON (matches the serde default).
    if (config.update_check_at_launch === false) return
    const info = await apiImpl.checkAppUpdate()
    if (info.updateAvailable && info.latestVersion) {
      // The About pane's exact copy — one glossary, one promise: notify
      // only, never auto-install.
      toast.success(messageFor('settings.about.updateAvailable', { version: info.latestVersion }))
    }
  } catch {
    // Launch-time nicety: every failure mode stays silent.
  }
}

/** Test hook — resets the once-per-run guard. */
export function resetStartupUpdateCheckForTests(): void {
  ranThisRun = false
}
