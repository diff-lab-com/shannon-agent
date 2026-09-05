/**
 * P1-5 D — `terminal:output` subscription.
 *
 * Production: the Rust pump coalesces PTY bytes (≤16 ms per emit) and
 * emits the Tauri event `terminal:output` with `{ terminalId, data }`
 * where `data` is base64 of the raw byte stream (byte-preserving — a pty
 * is not UTF-8; escape sequences and partial multi-byte sequences must
 * survive). Subscribers decode with `atob` before feeding xterm.js.
 *
 * Demo/mock mode (`VITE_MOCK_MODE=1`): there is no Tauri runtime, so the
 * mock terminal in `lib/mock/handlers.ts` re-dispatches the same payload
 * as a window CustomEvent (`MOCK_TERMINAL_OUTPUT_EVENT`). This module is
 * the single place that knows about both transports — components stay
 * transport-agnostic and tests can drive either path.
 */
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { EVENT_NAMES, type TerminalOutputPayload } from '@/types';

/**
 * Window CustomEvent name carrying `{ terminalId, data }` in demo mode.
 * Declared here (not in the mock folder) so production code never imports
 * from the mock layer.
 */
export const MOCK_TERMINAL_OUTPUT_EVENT = 'shannon:mock-terminal-output';

export type TerminalOutputHandler = (payload: TerminalOutputPayload) => void;

function isMockMode(): boolean {
  if (typeof import.meta !== 'undefined' && (import.meta as { env?: Record<string, string> }).env) {
    const env = (import.meta as { env: Record<string, string> }).env;
    if (env.VITE_MOCK_MODE === '1' || env.MODE === 'demo') return true;
  }
  return false;
}

/**
 * Decode a `terminal:output` payload's base64 `data` into a binary-safe
 * string suitable for `xterm.write` (latin1 round-trip keeps every byte;
 * multi-byte UTF-8 sequences are reconstructed by xterm's own decoder).
 */
export function decodeTerminalOutput(base64: string): string {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return new TextDecoder('utf-8', { fatal: false }).decode(bytes);
}

/** Encode raw bytes the same way the Rust side does (test/demo helper). */
export function encodeTerminalOutput(raw: string): string {
  const bytes = new TextEncoder().encode(raw);
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

/**
 * Subscribe to terminal output. Returns an unsubscribe function. Never
 * throws when the Tauri runtime is absent (plain-browser dev) — the
 * subscription just stays silent.
 */
export async function listenTerminalOutput(
  handler: TerminalOutputHandler,
  opts: { mock?: boolean } = {},
): Promise<() => void> {
  const mock = opts.mock ?? isMockMode();
  if (mock || typeof window === 'undefined') {
    if (typeof window === 'undefined') return () => {};
    const windowHandler = (event: Event) => {
      const detail = (event as CustomEvent<TerminalOutputPayload>).detail;
      if (detail) handler(detail);
    };
    window.addEventListener(MOCK_TERMINAL_OUTPUT_EVENT, windowHandler);
    return () => window.removeEventListener(MOCK_TERMINAL_OUTPUT_EVENT, windowHandler);
  }
  let unlisten: UnlistenFn | undefined;
  try {
    unlisten = await listen<TerminalOutputPayload>(EVENT_NAMES.TERMINAL_OUTPUT, (e) => {
      handler(e.payload);
    });
  } catch {
    // No Tauri runtime (plain-browser dev): stay silent like useTauriEvent.
    return () => {};
  }
  return () => {
    try {
      unlisten?.();
    } catch {
      // listener occasionally throws on teardown — nothing to do
    }
  };
}
