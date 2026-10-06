/**
 * P1-5 D — `terminal:output` / `terminal:exit` subscriptions.
 *
 * Production: the Rust pump coalesces PTY bytes (≤16 ms per emit) and
 * emits the Tauri event `terminal:output` with `{ terminalId, data }`
 * where `data` is base64 of the raw byte stream (byte-preserving — a pty
 * is not UTF-8; escape sequences and partial multi-byte sequences must
 * survive). Subscribers decode with `atob` before feeding xterm.js.
 * `terminal:exit` (`{ terminalId }`) is the authoritative process-exit
 * signal (P3-6) — the in-stream "[shannon: process exited …" text is for
 * humans and is never parsed.
 *
 * Demo/mock mode (`VITE_MOCK_MODE=1`): there is no Tauri runtime, so the
 * mock terminal in `lib/mock/handlers.ts` re-dispatches the same payloads
 * as window CustomEvents (`MOCK_TERMINAL_*_EVENT`). This module is the
 * single place that knows about both transports — components stay
 * transport-agnostic and tests can drive either path.
 */
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { EVENT_NAMES, type TerminalExitPayload, type TerminalOutputPayload } from '@/types';

/**
 * Window CustomEvent name carrying `{ terminalId, data }` in demo mode.
 * Declared here (not in the mock folder) so production code never imports
 * from the mock layer.
 */
export const MOCK_TERMINAL_OUTPUT_EVENT = 'shannon:mock-terminal-output';

/** Window CustomEvent name carrying `{ terminalId }` in demo mode. */
export const MOCK_TERMINAL_EXIT_EVENT = 'shannon:mock-terminal-exit';

export type TerminalOutputHandler = (payload: TerminalOutputPayload) => void;

export type TerminalExitHandler = (payload: TerminalExitPayload) => void;

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
 *
 * P3-5: a malformed payload (invalid base64 — `atob` throws) must not
 * break the handler invocation; it degrades to zero bytes with a warning.
 */
export function decodeTerminalOutput(base64: string): Uint8Array {
  let binary: string;
  try {
    binary = atob(base64);
  } catch (e) {
    console.warn('terminal:output payload is not valid base64 — dropped', e);
    return new Uint8Array(0);
  }
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
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

/**
 * Subscribe to terminal process exit (`terminal:exit`, P3-6) — the
 * authoritative "this tab ended" signal. Same transports and failure
 * semantics as `listenTerminalOutput`; the backend emission itself lands
 * with the Task-4 pump change, so until then this simply never fires.
 */
export async function listenTerminalExit(
  handler: TerminalExitHandler,
  opts: { mock?: boolean } = {},
): Promise<() => void> {
  const mock = opts.mock ?? isMockMode();
  if (mock || typeof window === 'undefined') {
    if (typeof window === 'undefined') return () => {};
    const windowHandler = (event: Event) => {
      const detail = (event as CustomEvent<TerminalExitPayload>).detail;
      if (detail) handler(detail);
    };
    window.addEventListener(MOCK_TERMINAL_EXIT_EVENT, windowHandler);
    return () => window.removeEventListener(MOCK_TERMINAL_EXIT_EVENT, windowHandler);
  }
  let unlisten: UnlistenFn | undefined;
  try {
    unlisten = await listen<TerminalExitPayload>(EVENT_NAMES.TERMINAL_EXIT, (e) => {
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
