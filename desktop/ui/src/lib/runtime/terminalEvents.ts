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
 * Decode a `terminal:output` payload's base64 `data` into the **raw PTY
 * bytes** for `xterm.write`. Deliberately NOT decoded to a string here:
 * the ≤16 ms pump slices the pty stream at arbitrary byte boundaries, so
 * a multi-byte UTF-8 sequence can be split across two events — a fresh
 * non-streaming TextDecoder per event would turn both halves into
 * U+FFFD. xterm's write buffer accepts Uint8Array and decodes UTF-8
 * incrementally, keeping an incomplete trailing sequence buffered until
 * the next write completes it, so bytes must reach it untouched.
 */
export function decodeTerminalOutput(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/**
 * ASCII-marker search over raw pty bytes (the backend's in-stream notices
 * — exit/truncation — are pure ASCII and always emitted contiguously
 * within a single event, so a per-event byte search is exact).
 */
export function bytesContainAscii(bytes: Uint8Array, marker: string): boolean {
  const needle = new TextEncoder().encode(marker);
  if (needle.length === 0 || bytes.length < needle.length) return false;
  outer: for (let i = 0; i <= bytes.length - needle.length; i += 1) {
    for (let j = 0; j < needle.length; j += 1) {
      if (bytes[i + j] !== needle[j]) continue outer;
    }
    return true;
  }
  return false;
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
