// Dangerous-install confirmation gate — UI side of the backend contract in
// desktop/src/extensions_commands.rs (`ConfirmationRequiredError`) and
// desktop/src/extensions/types.rs (`InstallConfirmation`).
//
// The five gated install commands (`install_mcp_stdio`, `install_mcp_mcpb`,
// `install_skill_from_repo`, `install_native_skill`, `install_agent_from_repo`)
// re-scan the exact content they are about to persist. A `Dangerous` verdict
// is refused with the structured payload below serialized as JSON inside the
// command's `Err(String)`; every OTHER error from these commands stays a
// plain (non-JSON) string, unchanged from before the gate existed.
//
// Detection contract: `JSON.parse` succeeds AND `.error ===
// 'confirmation_required'`. Anything else is a normal install error.

import type { InjectionMatch, InjectionRisk } from '@/lib/tauri-api'

/**
 * UI → backend override payload (Rust `InstallConfirmation`; the enum
 * serializes snake_case, so the wire value is `"dangerous"`).
 */
export type InstallConfirmationPayload = {
  acknowledged_risk: 'dangerous'
  typed_name: string
}

/** Structured refusal the backend returns when the gate fires. */
export interface ConfirmationRequiredPayload {
  error: 'confirmation_required'
  /** The rescan verdict — always `"dangerous"` (the only gated level). */
  risk: InjectionRisk
  /** Every pattern that fired — the "why was I blocked" list. */
  matches: InjectionMatch[]
  /** Total number of distinct patterns triggered. */
  match_count: number
  /** The gesture the UI must perform — always `"type_to_confirm"`. */
  required: string
  /** The entry name the user must type back exactly. */
  name: string
}

function parseIfJsonObject(text: string): Record<string, unknown> | null {
  // Fast path: plain error strings never start with `{` — skip the
  // (throwing) JSON.parse entirely for the overwhelmingly common case.
  if (!text.trim().startsWith('{')) return null
  try {
    const parsed: unknown = JSON.parse(text)
    if (parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>
    }
    return null
  } catch {
    return null
  }
}

/**
 * Detect a gate refusal among install-command rejections. The backend
 * rejects with the raw JSON string (Tauri v2 `Err(String)`); the demo mock
 * throws the same string, and layers in between may wrap it in an `Error` —
 * all three shapes are accepted. Non-gate errors (plain strings, other JSON,
 * real Error objects) return null so the caller's existing error handling
 * runs unchanged.
 */
export function parseConfirmationRequired(err: unknown): ConfirmationRequiredPayload | null {
  const candidates: string[] = []
  if (typeof err === 'string') {
    candidates.push(err)
  } else if (err instanceof Error) {
    candidates.push(err.message)
  } else if (
    err !== null &&
    typeof err === 'object' &&
    typeof (err as { message?: unknown }).message === 'string'
  ) {
    // `{ message }`-shaped rejections (some invoke layers wrap the string).
    candidates.push((err as { message: string }).message)
  }
  for (const text of candidates) {
    const parsed = parseIfJsonObject(text)
    if (parsed?.error === 'confirmation_required') {
      return parsed as unknown as ConfirmationRequiredPayload
    }
  }
  return null
}

/** Build the override payload the retry passes back to the command. */
export function buildInstallConfirmation(typedName: string): InstallConfirmationPayload {
  return { acknowledged_risk: 'dangerous', typed_name: typedName }
}
