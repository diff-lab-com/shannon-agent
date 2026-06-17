import { useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'

export type NotificationLevel = 'info' | 'warning' | 'error' | 'success'

export interface NotificationInput {
  title: string
  body: string
  level?: NotificationLevel
}

/**
 * Fire a native OS notification via the `send_notification` Tauri command.
 *
 * Thin wrapper over `invoke('send_notification', ...)`. Errors are surfaced
 * via the returned Promise — callers that want silent failure should `.catch`
 * the rejection. Most callers should let errors propagate so they show up in
 * dev tooling; the underlying Rust command only rejects when the OS
 * notification syscall itself fails (rare).
 *
 * The command is self-contained in shannon-desktop and does NOT depend on the
 * shannon-core notifications pipeline (P1). When the desktop bumps its pin
 * past P1, this hook will gain cooldown/dedup behaviour transparently.
 */
export function useNotification() {
  return useCallback(({ title, body, level }: NotificationInput) => {
    return invoke('send_notification', { payload: { title, body, level } })
  }, [])
}
