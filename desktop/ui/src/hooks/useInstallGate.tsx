// useInstallGate — shared call/retry plumbing for the Dangerous-install gate.
//
// One implementation for every surface that invokes one of the five gated
// install commands (`install_mcp_stdio`, `install_mcp_mcpb`,
// `install_skill_from_repo`, `install_native_skill`, `install_agent_from_repo`).
//
// Contract (2026-10-10 gate design):
// - The first attempt runs WITHOUT a confirmation. When the backend rejects
//   with the structured `confirmation_required` payload, the confirm drawer
//   opens and the caller's install promise stays pending — the backend
//   refused before writing anything, so a cancel later is a true no-op.
// - Confirm re-invokes the SAME command with
//   `confirmation: { acknowledged_risk: 'dangerous', typed_name }`. The
//   drawer closes on the retry attempt; if the fresh rescan gates again the
//   drawer reopens with the new payload. Success resolves the caller's
//   promise with the InstallResult; a non-gate failure rejects it so the
//   surface's existing error handling (toast/feedback) runs unchanged.
// - Cancel resolves the caller's promise with `null` — nothing installed,
//   no error toast (the honest state).
// - Non-gate errors bypass the drawer entirely and reject the caller's
//   promise with the original error.

import { useCallback, useRef, useState } from 'react'
import type { InstallConfirmation, InstallResult } from '@/lib/tauri-api'
import {
  buildInstallConfirmation,
  parseConfirmationRequired,
  type ConfirmationRequiredPayload,
} from '@/lib/installGate'
import InstallConfirmDrawer from '@/components/extensions/InstallConfirmDrawer'

/** One install attempt. `null` = first try (no override yet). */
export type InstallAttempt = (confirmation: InstallConfirmation | null) => Promise<InstallResult>

interface PendingInstall {
  attempt: InstallAttempt
  resolve: (result: InstallResult | null) => void
  reject: (err: unknown) => void
}

export interface InstallGate {
  /**
   * Wrap one install command call. Resolves with the `InstallResult` when
   * the install lands (first try or confirmed retry), or `null` when the
   * user cancelled the gate drawer. Rejects with the original error for
   * every non-gate failure.
   */
  installWithGate: (attempt: InstallAttempt) => Promise<InstallResult | null>
  /** The confirm drawer node — drop it anywhere in the surface's JSX. */
  gateDrawer: React.ReactNode
}

export function useInstallGate(): InstallGate {
  const [payload, setPayload] = useState<ConfirmationRequiredPayload | null>(null)
  const pendingRef = useRef<PendingInstall | null>(null)

  const runAttempt = useCallback(
    (pending: PendingInstall, confirmation: InstallConfirmation | null) => {
      pending
        .attempt(confirmation)
        .then((result) => {
          pendingRef.current = null
          setPayload(null)
          pending.resolve(result)
        })
        .catch((err: unknown) => {
          const gate = parseConfirmationRequired(err)
          if (gate) {
            // Gate fired — show why (or refresh the drawer when a confirmed
            // retry gates AGAIN with a fresh payload). The caller's promise
            // stays pending: the backend refused before mutating anything.
            pendingRef.current = pending
            setPayload(gate)
            return
          }
          pendingRef.current = null
          setPayload(null)
          pending.reject(err)
        })
    },
    [],
  )

  const installWithGate = useCallback(
    (attempt: InstallAttempt): Promise<InstallResult | null> =>
      new Promise<InstallResult | null>((resolve, reject) => {
        runAttempt({ attempt, resolve, reject }, null)
      }),
    [runAttempt],
  )

  const confirmInstall = useCallback(
    (typedName: string) => {
      const pending = pendingRef.current
      if (!pending) return
      setPayload(null) // the drawer closes on the retry attempt…
      runAttempt(pending, buildInstallConfirmation(typedName))
    },
    [runAttempt],
  )

  const cancelInstall = useCallback(() => {
    const pending = pendingRef.current
    pendingRef.current = null
    setPayload(null)
    // The backend already refused — cancel installs nothing and must not
    // read as an error.
    pending?.resolve(null)
  }, [])

  const gateDrawer = payload ? (
    <InstallConfirmDrawer payload={payload} onConfirm={confirmInstall} onCancel={cancelInstall} />
  ) : null

  return { installWithGate, gateDrawer }
}
